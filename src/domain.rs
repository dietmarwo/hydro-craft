use crate::{Evaluation, Work, invalid};
use serde::{Deserialize, Serialize};

pub const CAPABILITIES: &[&str] = &[
    "synthetic_daily_hbv_style",
    "masked_runoff_calibration",
    "one_reservoir",
];

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Model {
    Catchment {
        area_km2: f64,
        forcing: Vec<Day>,
        warmup: usize,
        train_end: usize,
        validation_end: usize,
    },
    Reservoir {
        capacity_m3: f64,
        initial_m3: f64,
        outlet_m3_per_day: f64,
        reserve_m3: f64,
        inputs: Vec<ReservoirInput>,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Day {
    day: u32,
    #[serde(skip)]
    fold: u8,
    precipitation_mm: f64,
    pet_mm: f64,
    temperature_c: f64,
    observed_mm: Option<f64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReservoirInput {
    inflow_m3: f64,
    evaporation_m3: f64,
    demand_m3: f64,
}

fn bounded(value: f64, max: f64) -> bool {
    (0.0..=max).contains(&value)
}

pub fn prepare(mut model: Model) -> Result<Model, String> {
    match &mut model {
        Model::Catchment {
            area_km2,
            forcing,
            warmup,
            train_end,
            validation_end,
        } => {
            if !(*area_km2 > 0.0
                && *area_km2 <= 1e7
                && *warmup < *train_end
                && *train_end < *validation_end
                && *validation_end < forcing.len()
                && forcing.len() <= 200_000)
                || forcing
                    .windows(2)
                    .any(|w| w[0].day.checked_add(1) != Some(w[1].day))
                || forcing.iter().any(|d| {
                    !bounded(d.precipitation_mm, 2000.0)
                        || !bounded(d.pet_mm, 100.0)
                        || !(-100.0..=100.0).contains(&d.temperature_c)
                        || d.observed_mm.is_some_and(|v| !bounded(v, 2000.0))
                })
            {
                return Err("invalid daily forcing, units, fold or catchment area".into());
            }
            let end = forcing.len();
            for (fold, range) in [
                *warmup..*train_end,
                *train_end..*validation_end,
                *validation_end..end,
            ]
            .into_iter()
            .enumerate()
            {
                if !forcing[range.clone()]
                    .iter()
                    .any(|d| d.observed_mm.is_some())
                {
                    return Err("every scored fold needs observed data".into());
                }
                for day in &mut forcing[range] {
                    day.fold = (fold + 1) as u8;
                }
            }
        }
        Model::Reservoir {
            capacity_m3,
            initial_m3,
            outlet_m3_per_day,
            reserve_m3,
            inputs,
        } => {
            if !(*capacity_m3 > 0.0
                && bounded(*capacity_m3, 1e12)
                && bounded(*initial_m3, *capacity_m3)
                && bounded(*reserve_m3, *capacity_m3)
                && bounded(*outlet_m3_per_day, 1e12)
                && (1..=200_000).contains(&inputs.len()))
                || inputs.iter().any(|d| {
                    ![d.inflow_m3, d.evaporation_m3, d.demand_m3]
                        .into_iter()
                        .all(|v| bounded(v, 1e12))
                })
            {
                return Err("invalid reservoir storage, capacity, outlet, or daily inputs".into());
            }
        }
    }
    Ok(model)
}

#[derive(Clone, Copy, Default)]
struct Score {
    squares: f64,
    bias: f64,
    n: u64,
}
impl Score {
    fn record(&mut self, runoff: f64, observed: f64) {
        let delta = runoff - observed;
        self.squares += delta * delta;
        self.bias += delta;
        self.n += 1;
    }
    fn rmse(&self) -> f64 {
        (self.squares / self.n as f64).sqrt()
    }
    fn result(&self) -> FoldMetrics {
        FoldMetrics {
            observations: self.n,
            rmse_mm: self.rmse(),
            volume_bias_mm: self.bias,
        }
    }
}

/// Fit of one scored fold. Fields are declared in key order, matching the
/// sorted maps of earlier releases.
#[derive(Debug, Serialize)]
pub struct FoldMetrics {
    observations: u64,
    rmse_mm: f64,
    volume_bias_mm: f64,
}

#[derive(Debug, Serialize)]
pub struct CatchmentMetrics {
    balance_mm: f64,
    max_daily_balance_mm: f64,
    test: FoldMetrics,
    trace: Option<Vec<CatchmentDay>>,
    train: FoldMetrics,
    validation: FoldMetrics,
}

#[derive(Debug, Serialize)]
pub struct ReservoirMetrics {
    longest_shortage_days: u64,
    max_balance_m3: f64,
    shortage_days: u64,
    shortage_m3: f64,
    spill_m3: f64,
    trace: Option<Vec<ReservoirDay>>,
}

/// One traced catchment day, fields in sorted key order.
#[derive(Debug, Serialize)]
pub struct CatchmentDay {
    balance_mm: f64,
    day: u32,
    discharge_m3_s: f64,
    et_mm: f64,
    runoff_mm: f64,
    stores_mm: [f64; 4],
}

/// One traced reservoir day, fields in sorted key order.
#[derive(Debug, Serialize)]
pub struct ReservoirDay {
    delivered_m3: f64,
    evaporation_m3: f64,
    release_m3: f64,
    shortage_m3: f64,
    spill_m3: f64,
    storage_m3: f64,
}

/// Full-record metrics, serialized as the model's metrics object.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Metrics {
    Catchment(CatchmentMetrics),
    Reservoir(ReservoirMetrics),
}

pub fn evaluate(
    model: &Model,
    parameters: &[f64],
    traced: bool,
    work: &mut Work<'_>,
) -> Evaluation {
    let days = match model {
        Model::Catchment { forcing, .. } => forcing.len(),
        Model::Reservoir { inputs, .. } => inputs.len(),
    };
    if traced && days > 10_000 {
        return Err(invalid("trace requires at most 10,000 days"));
    }
    match model {
        Model::Catchment {
            area_km2, forcing, ..
        } => {
            let p = catchment_parameters(parameters)?;
            let mut state = CatchmentState::default();
            let mut trace = Vec::with_capacity(if traced { forcing.len() } else { 0 });
            if work.metrics_enabled {
                for d in forcing {
                    work.step()?;
                    let flows = catchment_day::<true>(&mut state, &p, d);
                    if traced {
                        trace.push(CatchmentDay {
                            balance_mm: flows.residual,
                            day: d.day,
                            discharge_m3_s: flows.runoff * area_km2 * 1000.0 / 86_400.0,
                            et_mm: flows.evaporation,
                            runoff_mm: flows.runoff,
                            stores_mm: [state.snow, state.soil, state.upper, state.lower],
                        });
                    }
                    score_day::<true>(&mut state.folds, d, flows.runoff);
                }
            } else {
                for d in forcing {
                    work.step()?;
                    let flows = catchment_day::<false>(&mut state, &p, d);
                    score_day::<false>(&mut state.folds, d, flows.runoff);
                }
            }
            Ok((
                state.score(),
                work.metrics(|| state.metrics(traced.then_some(trace))),
            ))
        }
        Model::Reservoir {
            capacity_m3,
            initial_m3,
            outlet_m3_per_day,
            reserve_m3,
            inputs,
        } => {
            let (target, gain) = reservoir_controls(parameters)?;
            let site = Site {
                capacity: *capacity_m3,
                outlet: *outlet_m3_per_day,
                reserve: *reserve_m3,
            };
            let threshold = target * capacity_m3;
            let mut state = ReservoirState::new(*initial_m3);
            let mut trace = Vec::with_capacity(if traced { inputs.len() } else { 0 });
            if work.metrics_enabled {
                for d in inputs {
                    work.step()?;
                    let flows = reservoir_day::<true>(&mut state, threshold, gain, &site, d);
                    if traced {
                        trace.push(ReservoirDay {
                            delivered_m3: flows.delivered,
                            evaporation_m3: flows.evaporation,
                            release_m3: flows.release,
                            shortage_m3: flows.shortage,
                            spill_m3: flows.spill,
                            storage_m3: state.store,
                        });
                    }
                }
            } else {
                for d in inputs {
                    work.step()?;
                    reservoir_day::<false>(&mut state, threshold, gain, &site, d);
                }
            }
            Ok((
                state.score(),
                work.metrics(|| state.metrics(traced.then_some(trace))),
            ))
        }
    }
}

fn reservoir_controls(parameters: &[f64]) -> Result<(f64, f64), crate::Failure> {
    let &[target, gain] = parameters else {
        return Err(invalid("expected target fraction and release gain"));
    };
    if !bounded(target, 1.0) || !bounded(gain, 1.0) {
        return Err(invalid("reservoir controls must be in [0,1]"));
    }
    Ok((target, gain))
}

/// Fixed reservoir limits shared by every candidate.
struct Site {
    capacity: f64,
    outlet: f64,
    reserve: f64,
}

/// Candidate-local reservoir state and running totals.
#[derive(Clone, Copy)]
struct ReservoirState {
    store: f64,
    deficit: f64,
    spill_total: f64,
    streak: u64,
    longest: u64,
    short_days: u64,
    max_balance: f64,
}

impl ReservoirState {
    fn new(initial: f64) -> Self {
        Self {
            store: initial,
            deficit: 0.0,
            spill_total: 0.0,
            streak: 0,
            longest: 0,
            short_days: 0,
            max_balance: 0.0,
        }
    }
    fn score(&self) -> f64 {
        self.deficit + 0.05 * self.spill_total
    }
    fn metrics(&self, trace: Option<Vec<ReservoirDay>>) -> Metrics {
        Metrics::Reservoir(ReservoirMetrics {
            longest_shortage_days: self.longest,
            max_balance_m3: self.max_balance,
            shortage_days: self.short_days,
            shortage_m3: self.deficit,
            spill_m3: self.spill_total,
            trace,
        })
    }
}

struct Flows {
    evaporation: f64,
    delivered: f64,
    release: f64,
    shortage: f64,
    spill: f64,
}

/// Advance one reservoir day. Scalar and interleaved evaluation share this
/// function, so every candidate performs the same floating-point operations
/// in the same order on either path.
#[inline(always)]
fn reservoir_day<const METRICS: bool>(
    s: &mut ReservoirState,
    threshold: f64,
    gain: f64,
    site: &Site,
    d: &ReservoirInput,
) -> Flows {
    let before = s.store;
    // Decide from prior storage. Today's realized inflow is only
    // used when executing withdrawals and closing the balance.
    let discretionary = gain * (before - threshold).max(0.0);
    s.store += d.inflow_m3;
    let evaporation = s.store.min(d.evaporation_m3);
    s.store -= evaporation;
    let available = (s.store - site.reserve).max(0.0).min(site.outlet);
    let delivered = available.min(d.demand_m3);
    let release = (available - delivered).min(discretionary);
    let shortage = d.demand_m3 - delivered;
    s.store -= delivered + release;
    let spill = (s.store - site.capacity).max(0.0);
    s.store -= spill;
    s.deficit += shortage;
    s.spill_total += spill;
    if METRICS {
        // Branch-free streak bookkeeping: a dry day resets the streak and
        // leaves the longest streak unchanged.
        let short = shortage > 0.0;
        s.short_days += u64::from(short);
        s.streak = if short { s.streak + 1 } else { 0 };
        s.longest = s.longest.max(s.streak);
        let balance = before + d.inflow_m3 - evaporation - delivered - release - spill - s.store;
        s.max_balance = s.max_balance.max(balance.abs());
    }
    Flows {
        evaporation,
        delivered,
        release,
        shortage,
        spill,
    }
}

/// Steps a complete candidate takes: one per simulated day.
pub fn steps(model: &Model) -> u64 {
    match model {
        Model::Catchment { forcing, .. } => forcing.len() as u64,
        Model::Reservoir { inputs, .. } => inputs.len() as u64,
    }
}

/// Number of candidates the interleaved kernel advances together, or 1 when
/// the model has no interleaved kernel.
pub fn group_width(model: &Model) -> usize {
    match model {
        Model::Catchment { .. } => CATCHMENT_LANES,
        Model::Reservoir { .. } => RESERVOIR_LANES,
    }
}

const CATCHMENT_LANES: usize = 8;
const RESERVOIR_LANES: usize = 16;

/// Evaluate untraced candidates together over the shared daily inputs.
///
/// The caller guarantees that the work budget cannot stop a candidate: no
/// deadline or cancel file is monitored and `max_steps >= steps(model)`.
/// Each candidate then either fails validation before its first step or
/// simulates every day, so its logical step count is 0 or `steps(model)`.
/// Results match `evaluate` bit for bit.
pub fn evaluate_lanes(
    model: &Model,
    parameters: &[&[f64]],
    metrics: bool,
) -> Vec<(Evaluation, u64)> {
    let days = steps(model);
    match model {
        Model::Catchment { forcing, .. } => {
            let mut results: Vec<(Evaluation, u64)> = Vec::with_capacity(parameters.len());
            let mut valid = Vec::with_capacity(parameters.len());
            for (i, p) in parameters.iter().enumerate() {
                match catchment_parameters(p) {
                    Ok(p) => {
                        valid.push((i, p));
                        results.push((Ok((0.0, None)), days));
                    }
                    Err(e) => results.push((Err(e), 0)),
                }
            }
            // Lanes of one exponent class share a kernel. A known class uses up
            // to eight lanes; the dynamic class, with its power call, four.
            for class in [LINEAR, QUADRATIC, DYNAMIC] {
                let lanes: Vec<_> = valid
                    .iter()
                    .filter(|(_, p)| exponent_class(p.exponent) == class)
                    .copied()
                    .collect();
                let widest = if class == DYNAMIC { 4 } else { 8 };
                let mut rest = lanes.as_slice();
                while !rest.is_empty() {
                    let width = [8, 4, 2, 1]
                        .into_iter()
                        .find(|&w| w <= widest && w <= rest.len())
                        .unwrap_or(1);
                    let (chunk, tail) = rest.split_at(width);
                    let states = catchment_chunk(forcing, chunk, metrics, class);
                    for (&(i, _), state) in chunk.iter().zip(states) {
                        results[i].0 = Ok((state.score(), metrics.then(|| state.metrics(None))));
                    }
                    rest = tail;
                }
            }
            results
        }
        Model::Reservoir {
            capacity_m3,
            initial_m3,
            outlet_m3_per_day,
            reserve_m3,
            inputs,
        } => {
            let site = Site {
                capacity: *capacity_m3,
                outlet: *outlet_m3_per_day,
                reserve: *reserve_m3,
            };
            let mut results: Vec<(Evaluation, u64)> = Vec::with_capacity(parameters.len());
            let mut valid = Vec::with_capacity(parameters.len());
            for (i, p) in parameters.iter().enumerate() {
                match reservoir_controls(p) {
                    Ok((target, gain)) => {
                        valid.push((i, target * capacity_m3, gain));
                        results.push((Ok((0.0, None)), days));
                    }
                    Err(e) => results.push((Err(e), 0)),
                }
            }
            let mut rest = valid.as_slice();
            while !rest.is_empty() {
                // Lean runs use up to sixteen lanes; full records, with more
                // state per lane, use up to eight.
                let width = match rest.len() {
                    16.. if !metrics => 16,
                    8.. => 8,
                    4.. => 4,
                    _ => 1,
                };
                let (chunk, tail) = rest.split_at(width);
                let states = match (width, metrics) {
                    (16, _) => reservoir_lanes::<16, false>(&site, *initial_m3, inputs, chunk),
                    (8, false) => reservoir_lanes::<8, false>(&site, *initial_m3, inputs, chunk),
                    (8, true) => reservoir_lanes::<8, true>(&site, *initial_m3, inputs, chunk),
                    (4, false) => reservoir_lanes::<4, false>(&site, *initial_m3, inputs, chunk),
                    (4, true) => reservoir_lanes::<4, true>(&site, *initial_m3, inputs, chunk),
                    (_, false) => reservoir_lanes::<1, false>(&site, *initial_m3, inputs, chunk),
                    (_, true) => reservoir_lanes::<1, true>(&site, *initial_m3, inputs, chunk),
                };
                for (&(i, _, _), state) in chunk.iter().zip(states) {
                    results[i].0 = Ok((state.score(), metrics.then(|| state.metrics(None))));
                }
                rest = tail;
            }
            results
        }
    }
}

/// Advance `L` independent reservoir candidates day by day. Their update
/// chains do not depend on each other, so the processor overlaps them.
fn reservoir_lanes<const L: usize, const METRICS: bool>(
    site: &Site,
    initial: f64,
    inputs: &[ReservoirInput],
    controls: &[(usize, f64, f64)],
) -> Vec<ReservoirState> {
    let threshold: [f64; L] = std::array::from_fn(|l| controls[l].1);
    let gain: [f64; L] = std::array::from_fn(|l| controls[l].2);
    let mut states = [ReservoirState::new(initial); L];
    for d in inputs {
        for l in 0..L {
            reservoir_day::<METRICS>(&mut states[l], threshold[l], gain[l], site, d);
        }
    }
    states.to_vec()
}

/// Candidate parameters with the derived evapotranspiration scale.
#[derive(Clone, Copy)]
struct CatchmentParameters {
    capacity: f64,
    exponent: f64,
    kfast: f64,
    kbase: f64,
    percolation: f64,
    degree: f64,
    evap_scale: f64,
}

fn catchment_parameters(parameters: &[f64]) -> Result<CatchmentParameters, crate::Failure> {
    let &[capacity, exponent, lp, kfast, kbase, percolation, degree] = parameters else {
        return Err(invalid(
            "expected capacity, exponent, lp, kfast, kbase, percolation, degree",
        ));
    };
    if !(1.0..=2000.0).contains(&capacity)
        || !(0.1..=10.0).contains(&exponent)
        || !(0.05..=1.0).contains(&lp)
        || !bounded(kfast, 1.0)
        || !bounded(kbase, 1.0)
        || !bounded(percolation, 100.0)
        || !bounded(degree, 30.0)
    {
        return Err(invalid("catchment parameter out of range"));
    }
    Ok(CatchmentParameters {
        capacity,
        exponent,
        kfast,
        kbase,
        percolation,
        degree,
        evap_scale: lp * capacity,
    })
}

/// Candidate-local catchment stores, fold scores and balance diagnostics.
#[derive(Clone, Copy, Default)]
struct CatchmentState {
    snow: f64,
    soil: f64,
    upper: f64,
    lower: f64,
    folds: [Score; 3],
    balance: f64,
    max_balance: f64,
}

impl CatchmentState {
    fn score(&self) -> f64 {
        self.folds[0].rmse()
    }
    fn metrics(&self, trace: Option<Vec<CatchmentDay>>) -> Metrics {
        Metrics::Catchment(CatchmentMetrics {
            balance_mm: self.balance,
            max_daily_balance_mm: self.max_balance,
            test: self.folds[2].result(),
            trace,
            train: self.folds[0].result(),
            validation: self.folds[1].result(),
        })
    }
}

struct CatchmentFlows {
    runoff: f64,
    evaporation: f64,
    residual: f64,
}

/// Exponent classes with dedicated lane steps. `DYNAMIC` checks the
/// exponent on every wet day, exactly as the scalar path does.
const DYNAMIC: u8 = 0;
const LINEAR: u8 = 1;
const QUADRATIC: u8 = 2;

fn exponent_class(exponent: f64) -> u8 {
    if exponent == 1.0 {
        LINEAR
    } else if exponent == 2.0 {
        QUADRATIC
    } else {
        DYNAMIC
    }
}

/// Store total at the start of a day, for the full-record balance.
#[inline(always)]
fn day_start<const METRICS: bool>(s: &CatchmentState) -> f64 {
    if METRICS {
        s.snow + s.soil + s.upper + s.lower
    } else {
        0.0
    }
}

/// Frozen day: precipitation joins the snow store and no liquid water
/// reaches the soil. Returns liquid water and recharge.
#[inline(always)]
fn freeze(s: &mut CatchmentState, d: &Day) -> (f64, f64) {
    s.snow += d.precipitation_mm;
    (0.0, 0.0)
}

/// Thawed day: snowmelt and recharge. Returns liquid water and recharge.
/// For a known exponent class the recharge is computed and then selected,
/// which gives the same value as skipping it on days without liquid water;
/// `DYNAMIC` skips the power call on such days.
#[inline(always)]
fn thaw<const EXP: u8>(s: &mut CatchmentState, p: &CatchmentParameters, d: &Day) -> (f64, f64) {
    let melt = s.snow.min(p.degree * d.temperature_c);
    s.snow -= melt;
    let liquid = d.precipitation_mm + melt;
    let recharge = match EXP {
        LINEAR => {
            let recharge = liquid * (s.soil / p.capacity);
            if liquid == 0.0 { 0.0 } else { recharge }
        }
        QUADRATIC => {
            let moisture = s.soil / p.capacity;
            let recharge = liquid * (moisture * moisture);
            if liquid == 0.0 { 0.0 } else { recharge }
        }
        _ => {
            if liquid == 0.0 {
                0.0
            } else {
                let moisture = s.soil / p.capacity;
                let fraction = if p.exponent == 1.0 {
                    moisture
                } else if p.exponent == 2.0 {
                    moisture * moisture
                } else {
                    moisture.powf(p.exponent)
                };
                liquid * fraction
            }
        }
    };
    (liquid, recharge)
}

/// Soil overflow, evapotranspiration, percolation and recession after the
/// day's liquid water and recharge are known.
#[inline(always)]
fn day_finish<const METRICS: bool>(
    s: &mut CatchmentState,
    p: &CatchmentParameters,
    d: &Day,
    before: f64,
    liquid: f64,
    recharge: f64,
) -> CatchmentFlows {
    s.soil += liquid - recharge;
    let overflow = (s.soil - p.capacity).max(0.0);
    s.soil -= overflow;
    let evaporation = s.soil.min(d.pet_mm * (s.soil / p.evap_scale).min(1.0));
    s.soil -= evaporation;
    s.upper += recharge + overflow;
    let transfer = s.upper.min(p.percolation);
    s.upper -= transfer;
    s.lower += transfer;
    let fast = s.upper * p.kfast;
    s.upper -= fast;
    let base = s.lower * p.kbase;
    s.lower -= base;
    let runoff = fast + base;
    let residual = if METRICS {
        let residual = before + d.precipitation_mm
            - evaporation
            - runoff
            - (s.snow + s.soil + s.upper + s.lower);
        s.balance += residual;
        s.max_balance = s.max_balance.max(residual.abs());
        residual
    } else {
        0.0
    };
    CatchmentFlows {
        runoff,
        evaporation,
        residual,
    }
}

/// Advance one catchment day on the scalar path. The lane kernels compose
/// the same steps, so every candidate performs the same floating-point
/// operations in the same order on either path.
#[inline(always)]
fn catchment_day<const METRICS: bool>(
    s: &mut CatchmentState,
    p: &CatchmentParameters,
    d: &Day,
) -> CatchmentFlows {
    let before = day_start::<METRICS>(s);
    let (liquid, recharge) = if d.temperature_c <= 0.0 {
        freeze(s, d)
    } else {
        thaw::<DYNAMIC>(s, p, d)
    };
    day_finish::<METRICS>(s, p, d, before, liquid, recharge)
}

/// Score one simulated day. Lean runs score the training fold only; full
/// runs also score validation and test.
#[inline(always)]
fn score_day<const METRICS: bool>(folds: &mut [Score; 3], d: &Day, runoff: f64) {
    if d.fold != 0
        && (METRICS || d.fold == 1)
        && let Some(y) = d.observed_mm
    {
        folds[usize::from(d.fold - 1)].record(runoff, y);
    }
}

/// Advance `L` independent catchment candidates day by day over the shared
/// forcing. The frozen or thawed branch is taken once per day for all lanes;
/// each lane then runs its day steps and its scoring, as the scalar path
/// does. Every lane's exponent must belong to class `EXP`.
fn catchment_lanes<const L: usize, const METRICS: bool, const EXP: u8>(
    forcing: &[Day],
    lanes: &[(usize, CatchmentParameters)],
) -> Vec<CatchmentState> {
    let p: [CatchmentParameters; L] = std::array::from_fn(|l| lanes[l].1);
    let mut states = [CatchmentState::default(); L];
    for d in forcing {
        let mut runoff = [0.0; L];
        if d.temperature_c <= 0.0 {
            for l in 0..L {
                let before = day_start::<METRICS>(&states[l]);
                let (liquid, recharge) = freeze(&mut states[l], d);
                runoff[l] =
                    day_finish::<METRICS>(&mut states[l], &p[l], d, before, liquid, recharge)
                        .runoff;
            }
        } else {
            for l in 0..L {
                let before = day_start::<METRICS>(&states[l]);
                let (liquid, recharge) = thaw::<EXP>(&mut states[l], &p[l], d);
                runoff[l] =
                    day_finish::<METRICS>(&mut states[l], &p[l], d, before, liquid, recharge)
                        .runoff;
            }
        }
        for l in 0..L {
            score_day::<METRICS>(&mut states[l].folds, d, runoff[l]);
        }
    }
    states.to_vec()
}

/// Run one chunk of same-class lanes through the matching kernel.
fn catchment_chunk(
    forcing: &[Day],
    chunk: &[(usize, CatchmentParameters)],
    metrics: bool,
    class: u8,
) -> Vec<CatchmentState> {
    macro_rules! widths {
        ($metrics:literal, $class:expr) => {
            match chunk.len() {
                8 => catchment_lanes::<8, $metrics, $class>(forcing, chunk),
                4 => catchment_lanes::<4, $metrics, $class>(forcing, chunk),
                2 => catchment_lanes::<2, $metrics, $class>(forcing, chunk),
                _ => catchment_lanes::<1, $metrics, $class>(forcing, chunk),
            }
        };
    }
    match (metrics, class) {
        (false, LINEAR) => widths!(false, LINEAR),
        (false, QUADRATIC) => widths!(false, QUADRATIC),
        (false, _) => widths!(false, DYNAMIC),
        (true, LINEAR) => widths!(true, LINEAR),
        (true, QUADRATIC) => widths!(true, QUADRATIC),
        (true, _) => widths!(true, DYNAMIC),
    }
}
