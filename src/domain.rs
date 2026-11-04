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

#[derive(Default)]
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
            let (mut snow, mut soil, mut upper, mut lower) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
            let mut folds: [Score; 3] = Default::default();
            let mut balance = 0.0_f64;
            let mut max_balance = 0.0_f64;
            let mut trace = Vec::with_capacity(if traced { forcing.len() } else { 0 });
            let evap_scale = lp * capacity;
            for d in forcing {
                work.step()?;
                let before = if work.metrics_enabled {
                    snow + soil + upper + lower
                } else {
                    0.0
                };
                let liquid = if d.temperature_c <= 0.0 {
                    snow += d.precipitation_mm;
                    0.0
                } else {
                    let melt = snow.min(degree * d.temperature_c);
                    snow -= melt;
                    d.precipitation_mm + melt
                };
                let recharge = if liquid == 0.0 {
                    0.0
                } else {
                    let moisture = soil / capacity;
                    let fraction = if exponent == 1.0 {
                        moisture
                    } else if exponent == 2.0 {
                        moisture * moisture
                    } else {
                        moisture.powf(exponent)
                    };
                    liquid * fraction
                };
                soil += liquid - recharge;
                let overflow = (soil - capacity).max(0.0);
                soil -= overflow;
                let evap = soil.min(d.pet_mm * (soil / evap_scale).min(1.0));
                soil -= evap;
                upper += recharge + overflow;
                let transfer = upper.min(percolation);
                upper -= transfer;
                lower += transfer;
                let fast = upper * kfast;
                upper -= fast;
                let base = lower * kbase;
                lower -= base;
                let runoff = fast + base;
                if work.metrics_enabled {
                    let residual =
                        before + d.precipitation_mm - evap - runoff - (snow + soil + upper + lower);
                    balance += residual;
                    max_balance = max_balance.max(residual.abs());
                    if traced {
                        trace.push(CatchmentDay {
                            balance_mm: residual,
                            day: d.day,
                            discharge_m3_s: runoff * area_km2 * 1000.0 / 86_400.0,
                            et_mm: evap,
                            runoff_mm: runoff,
                            stores_mm: [snow, soil, upper, lower],
                        });
                    }
                }
                if d.fold != 0
                    && (work.metrics_enabled || d.fold == 1)
                    && let Some(y) = d.observed_mm
                {
                    folds[usize::from(d.fold - 1)].record(runoff, y);
                }
            }
            Ok((
                folds[0].rmse(),
                work.metrics(|| {
                    Metrics::Catchment(CatchmentMetrics {
                        balance_mm: balance,
                        max_daily_balance_mm: max_balance,
                        test: folds[2].result(),
                        trace: traced.then_some(trace),
                        train: folds[0].result(),
                        validation: folds[1].result(),
                    })
                }),
            ))
        }
        Model::Reservoir {
            capacity_m3,
            initial_m3,
            outlet_m3_per_day,
            reserve_m3,
            inputs,
        } => {
            let &[target, gain] = parameters else {
                return Err(invalid("expected target fraction and release gain"));
            };
            if !bounded(target, 1.0) || !bounded(gain, 1.0) {
                return Err(invalid("reservoir controls must be in [0,1]"));
            }
            let mut store = *initial_m3;
            let mut deficit = 0.0;
            let mut spill_total = 0.0;
            let (mut streak, mut longest, mut short_days) = (0_u64, 0_u64, 0_u64);
            let mut max_balance = 0.0_f64;
            let mut trace = Vec::with_capacity(if traced { inputs.len() } else { 0 });
            let threshold = target * capacity_m3;
            for d in inputs {
                work.step()?;
                let before = store;
                // Decide from prior storage. Today's realized inflow is only
                // used when executing withdrawals and closing the balance.
                let discretionary = gain * (before - threshold).max(0.0);
                store += d.inflow_m3;
                let evap = store.min(d.evaporation_m3);
                store -= evap;
                let available = (store - reserve_m3).max(0.0).min(*outlet_m3_per_day);
                let delivered = available.min(d.demand_m3);
                let release = (available - delivered).min(discretionary);
                let shortage = d.demand_m3 - delivered;
                store -= delivered + release;
                let spill = (store - capacity_m3).max(0.0);
                store -= spill;
                deficit += shortage;
                spill_total += spill;
                if work.metrics_enabled {
                    if shortage > 0.0 {
                        short_days += 1;
                        streak += 1;
                        longest = longest.max(streak);
                    } else {
                        streak = 0;
                    }
                    let balance = before + d.inflow_m3 - evap - delivered - release - spill - store;
                    max_balance = max_balance.max(balance.abs());
                    if traced {
                        trace.push(ReservoirDay {
                            delivered_m3: delivered,
                            evaporation_m3: evap,
                            release_m3: release,
                            shortage_m3: shortage,
                            spill_m3: spill,
                            storage_m3: store,
                        });
                    }
                }
            }
            Ok((
                deficit + 0.05 * spill_total,
                work.metrics(|| {
                    Metrics::Reservoir(ReservoirMetrics {
                        longest_shortage_days: longest,
                        max_balance_m3: max_balance,
                        shortage_days: short_days,
                        shortage_m3: deficit,
                        spill_m3: spill_total,
                        trace: traced.then_some(trace),
                    })
                }),
            ))
        }
    }
}
