mod domain;
mod pool;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json, value::RawValue};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    ops::Range,
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

/// A domain result: the objective and, when metrics are enabled, typed metrics.
pub(crate) type Evaluation = Result<(f64, Option<domain::Metrics>), Failure>;

#[derive(Debug, Clone)]
pub struct Failure {
    status: &'static str,
    detail: String,
}
pub fn invalid(detail: impl Into<String>) -> Failure {
    Failure {
        status: "invalid",
        detail: detail.into(),
    }
}
pub fn infeasible(detail: impl Into<String>) -> Failure {
    Failure {
        status: "infeasible",
        detail: detail.into(),
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub id: String,
    pub parameters: Vec<f64>,
    #[serde(default)]
    pub trace: bool,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Budget {
    workers: usize,
    max_steps: u64,
    wall_ms: Option<u64>,
    cancel_file: Option<PathBuf>,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            workers: 1,
            max_steps: 1_000_000,
            wall_ms: None,
            cancel_file: None,
        }
    }
}

pub struct Work<'a> {
    budget: &'a Budget,
    deadline: Option<Instant>,
    used: u64,
    monitored: bool,
    metrics_enabled: bool,
}
impl Work<'_> {
    /// Build metrics only for full records.
    pub fn metrics<M>(&self, make: impl FnOnce() -> M) -> Option<M> {
        self.metrics_enabled.then(make)
    }

    #[inline]
    pub fn step(&mut self) -> Result<(), Failure> {
        if self.used < self.budget.max_steps && !self.monitored {
            self.used += 1;
            return Ok(());
        }
        self.checked_step()
    }

    #[cold]
    fn checked_step(&mut self) -> Result<(), Failure> {
        if self.budget.cancel_file.as_ref().is_some_and(|p| p.exists()) {
            return Err(Failure {
                status: "cancelled",
                detail: "cancel file exists".into(),
            });
        }
        if self.deadline.is_some_and(|d| Instant::now() >= d) {
            return Err(Failure {
                status: "wall_limit",
                detail: "batch deadline reached".into(),
            });
        }
        if self.used == self.budget.max_steps {
            return Err(Failure {
                status: "work_limit",
                detail: "candidate step budget reached".into(),
            });
        }
        self.used += 1;
        Ok(())
    }
}

#[derive(Serialize)]
struct Record<'a> {
    id: &'a str,
    status: &'static str,
    steps: u64,
    score: Option<f64>,
    metrics: Option<domain::Metrics>,
    detail: Option<String>,
}

#[derive(Serialize)]
struct Batch<'a> {
    model_sha256: &'a str,
    records: Vec<Record<'a>>,
    schema: &'static str,
    version: &'static str,
}

/// One untraced candidate's outcome from `Prepared::score_matrix`.
#[derive(Clone, Debug, PartialEq)]
pub struct MatrixScore {
    /// Same status strings as the JSON records.
    pub status: &'static str,
    pub steps: u64,
    /// `Some` only for `"complete"`.
    pub score: Option<f64>,
}

const BATCH_LIMIT: usize = 4096;
const DIMENSION_LIMIT: usize = 64;

fn check_budget(budget: &Budget) -> Result<(), String> {
    if !(1..=64).contains(&budget.workers)
        || !(1..=100_000_000).contains(&budget.max_steps)
        || budget.wall_ms.is_some_and(|n| n == 0 || n > 86_400_000)
    {
        return Err("invalid budget or batch size".into());
    }
    Ok(())
}

/// Split `0..count` into consecutive groups of at most `group` indexes, run
/// `f` on each group inline or on up to `workers` concurrent evaluators from
/// the persistent pool, and return the results in index order.
fn dispatch<T: Send + Sync>(
    count: usize,
    group: usize,
    workers: usize,
    f: impl Fn(Range<usize>) -> Vec<T> + Sync,
) -> Vec<T> {
    let groups = count.div_ceil(group);
    let range = |g: usize| g * group..((g + 1) * group).min(count);
    if workers == 1 || groups == 1 {
        let mut out = Vec::with_capacity(count);
        for g in 0..groups {
            out.extend(f(range(g)));
        }
        return out;
    }
    let next = AtomicUsize::new(0);
    let slots: Vec<_> = (0..groups).map(|_| OnceLock::new()).collect();
    pool::run(workers.min(groups) - 1, &|| {
        loop {
            let g = next.fetch_add(1, Ordering::Relaxed);
            if g >= groups {
                break;
            }
            assert!(slots[g].set(f(range(g))).is_ok(), "duplicate work index");
        }
    });
    slots
        .into_iter()
        .flat_map(|slot| slot.into_inner().expect("missing evaluation"))
        .collect()
}

/// Reject non-finite objectives, as for every evaluation path.
fn finite(result: Evaluation) -> Evaluation {
    result.and_then(|(score, metrics)| {
        if score.is_finite() {
            Ok((score, metrics))
        } else {
            Err(invalid("nonfinite objective"))
        }
    })
}

pub struct Prepared {
    model: domain::Model,
    model_sha256: String,
}
impl Prepared {
    pub fn new(model: &str) -> Result<Self, String> {
        let model_sha256 = format!("{:x}", Sha256::digest(model.as_bytes()));
        let model = domain::prepare(serde_json::from_str(model).map_err(|e| e.to_string())?)?;
        Ok(Self {
            model,
            model_sha256,
        })
    }

    /// SHA-256 of the exact model bytes passed to `Prepared::new`.
    pub fn model_sha256(&self) -> &str {
        &self.model_sha256
    }

    pub fn evaluate_json(&self, candidates: &str, budget: &str) -> Result<String, String> {
        let candidates: Vec<Candidate> =
            serde_json::from_str(candidates).map_err(|e| e.to_string())?;
        let budget = serde_json::from_str(budget).map_err(|e| e.to_string())?;
        self.evaluate_encoded(candidates, budget, true)
    }

    /// Evaluate untraced candidates with full feasibility checks and null metrics.
    pub fn scores_json(&self, candidates: &str, budget: &str) -> Result<String, String> {
        let candidates: Vec<Candidate> =
            serde_json::from_str(candidates).map_err(|e| e.to_string())?;
        let budget = serde_json::from_str(budget).map_err(|e| e.to_string())?;
        if candidates.iter().any(|c| c.trace) {
            return Err("scores mode accepts untraced candidates only".into());
        }
        self.evaluate_encoded(candidates, budget, false)
    }

    /// Evaluate `rows` untraced candidates whose parameters are stored row-major in
    /// `parameters`. Each row has `parameters.len() / rows` values; dimension 0 is
    /// allowed. Budget JSON, limits, worker dispatch, deadlines, cancellation and
    /// failure classification are the same as `scores_json`.
    pub fn score_matrix(
        &self,
        rows: usize,
        parameters: &[f64],
        budget: &str,
    ) -> Result<Vec<MatrixScore>, String> {
        if !(1..=BATCH_LIMIT).contains(&rows) {
            return Err(format!("matrix rows must be between 1 and {BATCH_LIMIT}"));
        }
        if !parameters.len().is_multiple_of(rows) {
            return Err("parameter count is not a multiple of the row count".into());
        }
        let dimension = parameters.len() / rows;
        if dimension > DIMENSION_LIMIT {
            return Err(format!(
                "parameter vector exceeds limit of {DIMENSION_LIMIT}"
            ));
        }
        let budget: Budget = serde_json::from_str(budget).map_err(|e| e.to_string())?;
        check_budget(&budget)?;
        Ok(self.evaluate_groups(
            rows,
            &budget,
            false,
            |row| (&parameters[row * dimension..(row + 1) * dimension], false),
            |_, result, steps| match result {
                Ok((score, _)) => MatrixScore {
                    status: "complete",
                    steps,
                    score: Some(score),
                },
                Err(e) => MatrixScore {
                    status: e.status,
                    steps,
                    score: None,
                },
            },
        ))
    }

    /// Evaluate `count` candidates given by `input(index) -> (parameters,
    /// traced)` and map each classified result through `output`.
    ///
    /// When no deadline or cancel file is monitored and the step budget cannot
    /// stop a complete candidate, untraced candidates go through the domain's
    /// interleaved kernel in groups; otherwise each candidate runs alone with
    /// a guarded work counter. Both paths produce identical results.
    fn evaluate_groups<'p, T: Send + Sync>(
        &self,
        count: usize,
        budget: &Budget,
        metrics_enabled: bool,
        input: impl Fn(usize) -> (&'p [f64], bool) + Sync,
        output: impl Fn(usize, Evaluation, u64) -> T + Sync,
    ) -> Vec<T> {
        let deadline = budget
            .wall_ms
            .map(|n| Instant::now() + Duration::from_millis(n));
        let unbounded = deadline.is_none()
            && budget.cancel_file.is_none()
            && budget.max_steps >= domain::steps(&self.model);
        let group = if unbounded {
            domain::group_width(&self.model)
        } else {
            1
        };
        dispatch(count, group, budget.workers, |range| {
            if range.len() == 1 {
                let (parameters, traced) = input(range.start);
                let (result, steps) =
                    self.run(parameters, traced, budget, deadline, metrics_enabled);
                return vec![output(range.start, result, steps)];
            }
            let untraced: Vec<&[f64]> = range
                .clone()
                .map(&input)
                .filter(|(_, traced)| !traced)
                .map(|(parameters, _)| parameters)
                .collect();
            let mut lanes =
                domain::evaluate_lanes(&self.model, &untraced, metrics_enabled).into_iter();
            range
                .map(|index| {
                    let (parameters, traced) = input(index);
                    let (result, steps) = if traced {
                        self.run(parameters, traced, budget, deadline, metrics_enabled)
                    } else {
                        let (result, steps) = lanes.next().expect("one lane per candidate");
                        (finite(result), steps)
                    };
                    output(index, result, steps)
                })
                .collect()
        })
    }

    /// Time one JSON call's phases: candidate and budget decode, evaluation
    /// with record assembly, and result encoding. Used by the breakdown
    /// example; not part of the stable API.
    #[cfg(feature = "profiling")]
    #[doc(hidden)]
    pub fn profile_json(
        &self,
        candidates: &str,
        budget: &str,
        lean: bool,
    ) -> Result<[Duration; 3], String> {
        let start = Instant::now();
        let candidates: Vec<Candidate> =
            serde_json::from_str(candidates).map_err(|e| e.to_string())?;
        let budget = serde_json::from_str(budget).map_err(|e| e.to_string())?;
        let decoded = Instant::now();
        let batch = self.evaluate(&candidates, budget, !lean)?;
        let evaluated = Instant::now();
        let text = serde_json::to_string(&batch).map_err(|e| e.to_string())?;
        let encoded = Instant::now();
        std::hint::black_box(text);
        Ok([decoded - start, evaluated - decoded, encoded - evaluated])
    }

    fn evaluate_encoded(
        &self,
        candidates: Vec<Candidate>,
        budget: Budget,
        metrics_enabled: bool,
    ) -> Result<String, String> {
        serde_json::to_string(&self.evaluate(&candidates, budget, metrics_enabled)?)
            .map_err(|e| e.to_string())
    }

    /// Evaluate one parameter vector and return its classified result and steps.
    fn run(
        &self,
        parameters: &[f64],
        traced: bool,
        budget: &Budget,
        deadline: Option<Instant>,
        metrics_enabled: bool,
    ) -> (Evaluation, u64) {
        let mut work = Work {
            budget,
            deadline,
            used: 0,
            metrics_enabled,
            monitored: deadline.is_some() || budget.cancel_file.is_some(),
        };
        let result = finite(domain::evaluate(&self.model, parameters, traced, &mut work));
        (result, work.used)
    }

    fn record(c: &Candidate, result: Evaluation, steps: u64) -> Record<'_> {
        match result {
            Ok((score, metrics)) => Record {
                id: &c.id,
                status: "complete",
                steps,
                score: Some(score),
                metrics,
                detail: None,
            },
            Err(e) => Record {
                id: &c.id,
                status: e.status,
                steps,
                score: None,
                metrics: None,
                detail: Some(e.detail),
            },
        }
    }

    fn evaluate<'a>(
        &'a self,
        candidates: &'a [Candidate],
        budget: Budget,
        metrics_enabled: bool,
    ) -> Result<Batch<'a>, String> {
        check_budget(&budget)?;
        if candidates.is_empty() || candidates.len() > BATCH_LIMIT {
            return Err("invalid budget or batch size".into());
        }
        let mut seen = HashSet::with_capacity(candidates.len());
        for c in candidates {
            if c.id.is_empty()
                || c.id.len() > 128
                || !seen.insert(&c.id)
                || c.parameters.len() > DIMENSION_LIMIT
            {
                return Err(
                    "invalid or duplicate candidate id, or parameter vector exceeds limit".into(),
                );
            }
        }
        if candidates.iter().filter(|c| c.trace).count() > 16 {
            return Err("at most 16 traced candidates per batch".into());
        }
        let records = self.evaluate_groups(
            candidates.len(),
            &budget,
            metrics_enabled,
            |i| (candidates[i].parameters.as_slice(), candidates[i].trace),
            |i, result, steps| Self::record(&candidates[i], result, steps),
        );
        Ok(Batch {
            model_sha256: &self.model_sha256,
            records,
            schema: concat!(env!("CARGO_PKG_NAME"), ".result/0.1"),
            version: env!("CARGO_PKG_VERSION"),
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    model: Box<RawValue>,
    candidates: Vec<Candidate>,
    #[serde(default)]
    budget: Budget,
}

pub fn run(text: &str) -> Result<Value, String> {
    let req: Request = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if req.schema != concat!(env!("CARGO_PKG_NAME"), ".request/0.1") {
        return Err("unsupported request schema".into());
    }
    let prepared = Prepared::new(req.model.get())?;
    serde_json::to_value(prepared.evaluate(&req.candidates, req.budget, true)?)
        .map_err(|e| e.to_string())
}

pub fn capabilities() -> Value {
    json!({"schema": concat!(env!("CARGO_PKG_NAME"), ".capabilities/0.1"),
           "version": env!("CARGO_PKG_VERSION"), "models": domain::CAPABILITIES,
           "batch": true, "worker_limit": 64})
}

#[cfg(feature = "python")]
mod python {
    use super::*;
    use pyo3::{exceptions::PyValueError, prelude::*};

    #[pyclass(name = "Prepared")]
    struct PyPrepared(Prepared);

    #[pymethods]
    impl PyPrepared {
        #[new]
        fn new(model: &str) -> PyResult<Self> {
            Prepared::new(model)
                .map(Self)
                .map_err(PyValueError::new_err)
        }
        fn scores(&self, py: Python<'_>, candidates: String, budget: String) -> PyResult<String> {
            py.detach(|| self.0.scores_json(&candidates, &budget))
                .map_err(PyValueError::new_err)
        }
        fn batch(&self, py: Python<'_>, candidates: String, budget: String) -> PyResult<String> {
            py.detach(|| self.0.evaluate_json(&candidates, &budget))
                .map_err(PyValueError::new_err)
        }
        /// Score untraced parameter rows without JSON. Returns scores (NaN unless
        /// complete), statuses and steps in row order.
        #[allow(clippy::type_complexity)]
        fn score_matrix(
            &self,
            py: Python<'_>,
            rows: Vec<Vec<f64>>,
            budget: String,
        ) -> PyResult<(Vec<f64>, Vec<&'static str>, Vec<u64>)> {
            let dimension = rows.first().map_or(0, Vec::len);
            if rows.iter().any(|row| row.len() != dimension) {
                return Err(PyValueError::new_err(
                    "score_matrix rows must all have the same length",
                ));
            }
            let count = rows.len();
            let flat = rows.concat();
            let scores = py
                .detach(|| self.0.score_matrix(count, &flat, &budget))
                .map_err(PyValueError::new_err)?;
            Ok((
                scores.iter().map(|s| s.score.unwrap_or(f64::NAN)).collect(),
                scores.iter().map(|s| s.status).collect(),
                scores.iter().map(|s| s.steps).collect(),
            ))
        }
    }

    #[pymodule]
    fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_class::<PyPrepared>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        serde_json::from_str(include_str!("../models/example.json")).unwrap()
    }
    #[test]
    fn prepared_workers_return_same_completed_records() {
        let example = fixture();
        let prepared = Prepared::new(&example["model"].to_string()).unwrap();
        let candidates = json!([
            {"id":"first","parameters":example["parameters"]},
            {"id":"second","parameters":example["parameters"]},
        ]);
        let serial = prepared
            .evaluate_json(&candidates.to_string(), r#"{"workers":1}"#)
            .unwrap();
        let parallel = prepared
            .evaluate_json(&candidates.to_string(), r#"{"workers":4}"#)
            .unwrap();
        assert_eq!(serial, parallel);
        assert!(serial.contains(r#""status":"complete""#));
    }
    #[test]
    fn scores_keep_order_status_steps_and_objective() {
        let example = fixture();
        let prepared = Prepared::new(&example["model"].to_string()).unwrap();
        let mut candidates = json!([
            {"id":"valid","parameters":example["parameters"]},
            {"id":"invalid","parameters": vec![1.0; 63]},
        ]);
        for budget in ["{}", r#"{"workers":4}"#, r#"{"max_steps":1}"#] {
            let mut full: Value = serde_json::from_str(
                &prepared
                    .evaluate_json(&candidates.to_string(), budget)
                    .unwrap(),
            )
            .unwrap();
            for row in full["records"].as_array_mut().unwrap() {
                row["metrics"] = Value::Null;
            }
            let scores: Value = serde_json::from_str(
                &prepared
                    .scores_json(&candidates.to_string(), budget)
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(full, scores);
        }
        candidates[0]["trace"] = json!(true);
        assert!(prepared.scores_json(&candidates.to_string(), "{}").is_err());
    }

    #[test]
    fn model_and_candidate_validation_are_independent() {
        let mut example = fixture();
        example["model"]["unexpected"] = json!(true);
        assert!(Prepared::new(&example["model"].to_string()).is_err());
        let example = fixture();
        let prepared = Prepared::new(&example["model"].to_string()).unwrap();
        let duplicate = json!([
            {"id":"a","parameters":example["parameters"]},
            {"id":"a","parameters":example["parameters"]},
        ]);
        assert!(
            prepared
                .evaluate_json(&duplicate.to_string(), "{}")
                .is_err()
        );
    }

    fn reservoir() -> Value {
        json!({"kind":"reservoir","capacity_m3":100,"initial_m3":40,"outlet_m3_per_day":20,
               "reserve_m3":10,"inputs":(0..30).map(|i| json!({"inflow_m3":i % 9,
               "evaporation_m3":1,"demand_m3":12})).collect::<Vec<_>>()})
    }

    /// Compare `score_matrix` with `scores_json` row by row, score bit for bit.
    fn assert_matrix_matches(prepared: &Prepared, rows: &[Vec<f64>], budget: &str) {
        let candidates: Vec<_> = rows
            .iter()
            .enumerate()
            .map(|(i, p)| json!({"id": format!("row-{i}"), "parameters": p}))
            .collect();
        let lean: Value = serde_json::from_str(
            &prepared
                .scores_json(&Value::from(candidates).to_string(), budget)
                .unwrap(),
        )
        .unwrap();
        let flat: Vec<f64> = rows.concat();
        let matrix = prepared.score_matrix(rows.len(), &flat, budget).unwrap();
        let records = lean["records"].as_array().unwrap();
        assert_eq!(records.len(), matrix.len());
        for (record, row) in records.iter().zip(&matrix) {
            assert_eq!(record["status"], row.status, "{budget}");
            assert_eq!(record["steps"], row.steps, "{budget}");
            assert_eq!(
                record["score"].as_f64().map(f64::to_bits),
                row.score.map(f64::to_bits),
                "{budget}"
            );
            assert_eq!(row.score.is_some(), row.status == "complete");
        }
    }

    #[test]
    fn score_matrix_matches_scores_json() {
        let example = fixture();
        let catchment = Prepared::new(&example["model"].to_string()).unwrap();
        let base: Vec<f64> = serde_json::from_value(example["parameters"].clone()).unwrap();
        let mut rows: Vec<Vec<f64>> = (0..9)
            .map(|i| {
                let mut p = base.clone();
                p[0] *= 1.0 + f64::from(i) * 0.05;
                p[1] = [1.0, 2.0, 1.37][i as usize % 3];
                p
            })
            .collect();
        rows[4][0] = 0.5; // capacity out of range
        rows[6][2] = 1.5; // evapotranspiration fraction out of range
        let dir = std::env::temp_dir().join(format!("hydro-matrix-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let flag = dir.join("cancel");
        std::fs::write(&flag, b"").unwrap();
        let cancel = json!({"cancel_file": flag}).to_string();
        let res = Prepared::new(&reservoir().to_string()).unwrap();
        let policies = vec![
            vec![0.5, 0.1],
            vec![1.5, 0.1],
            vec![0.2, 0.9],
            vec![0.9, 0.0],
        ];
        for budget in [
            "{}",
            r#"{"workers":4}"#,
            r#"{"workers":3,"max_steps":40}"#,
            r#"{"max_steps":1}"#,
            r#"{"wall_ms":60000}"#,
            cancel.as_str(),
        ] {
            assert_matrix_matches(&catchment, &rows, budget);
            assert_matrix_matches(&catchment, std::slice::from_ref(&base), budget);
            assert_matrix_matches(&catchment, &[vec![1.0; 3], vec![2.0; 3]], budget);
            assert_matrix_matches(&res, &policies, budget);
        }
        let limited = catchment
            .score_matrix(1, &base, r#"{"max_steps":1}"#)
            .unwrap();
        assert_eq!((limited[0].status, limited[0].steps), ("work_limit", 1));
        let cancelled = res.score_matrix(1, &policies[0], &cancel).unwrap();
        assert_eq!((cancelled[0].status, cancelled[0].steps), ("cancelled", 0));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn score_matrix_accepts_dimension_zero_and_rejects_bad_shapes() {
        let res = Prepared::new(&reservoir().to_string()).unwrap();
        let empty = res.score_matrix(3, &[], "{}").unwrap();
        assert_eq!(empty.len(), 3);
        assert!(
            empty
                .iter()
                .all(|r| r.status == "invalid" && r.score.is_none())
        );
        assert_matrix_matches(&res, &[vec![], vec![]], "{}");
        assert!(res.score_matrix(0, &[], "{}").is_err());
        assert!(res.score_matrix(4097, &[], "{}").is_err());
        assert!(res.score_matrix(4096, &[], "{}").is_ok());
        assert!(res.score_matrix(2, &[0.5, 0.1, 0.2], "{}").is_err());
        assert!(res.score_matrix(1, &[0.5; 65], "{}").is_err());
        assert_eq!(
            res.score_matrix(1, &[0.5; 64], "{}").unwrap()[0].status,
            "invalid"
        );
        for budget in [
            "",
            "null",
            r#"{"workers":0}"#,
            r#"{"workers":65}"#,
            r#"{"max_steps":0}"#,
            r#"{"wall_ms":0}"#,
            r#"{"unknown":1}"#,
        ] {
            assert!(
                res.score_matrix(1, &[0.5, 0.1], budget).is_err(),
                "{budget}"
            );
        }
    }

    /// Full records serialize metrics with sorted keys, exactly as the
    /// `serde_json::Value` maps of 0.2.0 did, and the CLI view agrees.
    #[test]
    fn typed_metrics_keep_sorted_json_and_cli_values() {
        let example = fixture();
        for (model, parameters) in [
            (example["model"].clone(), example["parameters"].clone()),
            (reservoir(), json!([0.4, 0.3])),
        ] {
            let prepared = Prepared::new(&model.to_string()).unwrap();
            let candidates = json!([
                {"id":"plain","parameters":parameters},
                {"id":"traced","parameters":parameters,"trace":true},
            ]);
            let text = prepared
                .evaluate_json(&candidates.to_string(), "{}")
                .unwrap();
            #[derive(Deserialize)]
            struct Row<'a> {
                #[serde(borrow)]
                metrics: &'a RawValue,
            }
            #[derive(Deserialize)]
            struct Rows<'a> {
                #[serde(borrow)]
                records: Vec<Row<'a>>,
            }
            let rows: Rows = serde_json::from_str(&text).unwrap();
            for row in rows.records {
                let sorted: Value = serde_json::from_str(row.metrics.get()).unwrap();
                assert!(sorted.is_object());
                assert_eq!(row.metrics.get(), sorted.to_string());
            }
            let request = json!({"schema":"hydro-craft.request/0.1","model":model,
                                 "candidates":candidates});
            let cli = run(&request.to_string()).unwrap();
            assert_eq!(cli, serde_json::from_str::<Value>(&text).unwrap());
        }
    }

    /// Interleaved groups (unmonitored budgets) and guarded single-candidate
    /// runs (a far deadline forces them) give identical JSON text, including
    /// invalid and traced candidates mixed into a group, partial groups and
    /// several workers.
    fn assert_lanes_match_scalar(model: &Value, rows: &[Value]) {
        let prepared = Prepared::new(&model.to_string()).unwrap();
        let days = domain::steps(&prepared.model);
        for count in [2, 3, 5, 8, 9, 13, 17, rows.len()] {
            let candidates: Vec<_> = (0..count)
                .map(|i| {
                    json!({"id": format!("c{i}"), "parameters": rows[i % rows.len()],
                           "trace": i % 7 == 3})
                })
                .collect();
            let text = Value::from(candidates.clone()).to_string();
            for workers in [1, 3] {
                let scalar = json!({"workers": workers, "wall_ms": 3_600_000}).to_string();
                let lanes = json!({"workers": workers}).to_string();
                let exact = json!({"workers": workers, "max_steps": days}).to_string();
                let full = prepared.evaluate_json(&text, &scalar).unwrap();
                assert_eq!(full, prepared.evaluate_json(&text, &lanes).unwrap());
                assert_eq!(full, prepared.evaluate_json(&text, &exact).unwrap());
                let untraced: Vec<_> = candidates
                    .iter()
                    .map(|c| json!({"id": c["id"], "parameters": c["parameters"]}))
                    .collect();
                let text = Value::from(untraced).to_string();
                let lean = prepared.scores_json(&text, &scalar).unwrap();
                assert_eq!(lean, prepared.scores_json(&text, &lanes).unwrap());
                let short = json!({"workers": workers, "max_steps": days - 1}).to_string();
                let limited: Value =
                    serde_json::from_str(&prepared.scores_json(&text, &short).unwrap()).unwrap();
                for row in limited["records"].as_array().unwrap() {
                    assert!(row["status"] == "work_limit" || row["status"] == "invalid");
                }
            }
        }
    }

    #[test]
    fn reservoir_lanes_match_scalar_runs() {
        let rows: Vec<Value> = (0..23)
            .map(|i| match i % 6 {
                4 => json!([1.5, 0.2]),
                5 => json!([0.3]),
                _ => json!([f64::from(i) / 23.0, f64::from(i % 5) / 4.0]),
            })
            .collect();
        assert_lanes_match_scalar(&reservoir(), &rows);
        let spill = json!({"kind":"reservoir","capacity_m3":50,"initial_m3":50,"outlet_m3_per_day":3,
            "reserve_m3":0,"inputs":(0..40).map(|i| json!({"inflow_m3":(i * 7) % 20,
            "evaporation_m3":0.5,"demand_m3":if i % 3 == 0 {0} else {6}})).collect::<Vec<_>>()});
        assert_lanes_match_scalar(&spill, &rows);
    }

    #[test]
    fn catchment_lanes_match_scalar_runs() {
        let example = fixture();
        let base: Vec<f64> = serde_json::from_value(example["parameters"].clone()).unwrap();
        let rows: Vec<Value> = (0..23)
            .map(|i| {
                let mut p = base.clone();
                p[0] *= 1.0 + f64::from(i) * 0.04;
                p[1] = [1.0, 2.0, 1.37, 0.6][i as usize % 4];
                p[3] = f64::from(i % 5) / 5.0;
                match i % 9 {
                    7 => json!([1.0, 2.0]),
                    8 => {
                        p[2] = 0.01;
                        json!(p)
                    }
                    _ => json!(p),
                }
            })
            .collect();
        assert_lanes_match_scalar(&example["model"], &rows);
        // Snow and dry spells exercise the frozen, melt and zero-liquid branches.
        let mut cold = example["model"].clone();
        for (i, day) in cold["forcing"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            day["temperature_c"] = json!(if i % 10 < 4 { -3.0 } else { 2.5 });
            if i % 6 == 0 {
                day["precipitation_mm"] = json!(0.0);
            }
        }
        assert_lanes_match_scalar(&cold, &rows);
    }
}
