//! Screen reservoir release rules through the typed score-matrix API.
//!
//! Run: `cargo run --release --example matrix`
use hydro_craft::Prepared;
use serde_json::{Value, json};

fn main() -> Result<(), String> {
    // Two years of seasonal inflow, demand and evaporation in cubic metres.
    let inputs: Vec<Value> = (0..730)
        .map(|day| {
            let phase = f64::from(day % 365) / 365.0 * std::f64::consts::TAU;
            json!({
                "inflow_m3": (58_000.0 + 45_000.0 * (phase - 1.0).sin()).max(0.0),
                "evaporation_m3": 1_500.0 + 1_200.0 * (-phase.cos()).max(0.0),
                "demand_m3": 42_000.0 + 22_000.0 * (-phase.cos()).max(0.0),
            })
        })
        .collect();
    let model = json!({"kind": "reservoir", "capacity_m3": 4.0e6, "initial_m3": 2.0e6,
                       "outlet_m3_per_day": 70_000.0, "reserve_m3": 0.6e6, "inputs": inputs});
    let prepared = Prepared::new(&model.to_string())?;

    // A 41 x 41 grid of (target fraction, release gain) rows, stored row-major,
    // plus one row outside the valid controls.
    let mut parameters = Vec::new();
    for target in 0..=40 {
        for gain in 0..=40 {
            parameters.extend([f64::from(target) / 40.0, f64::from(gain) / 40.0]);
        }
    }
    parameters.extend([1.5, 0.5]);
    let rows = parameters.len() / 2;
    let scores = prepared.score_matrix(rows, &parameters, r#"{"workers": 2}"#)?;

    let invalid = scores.iter().filter(|s| s.status == "invalid").count();
    let (best, score) = scores
        .iter()
        .enumerate()
        .filter_map(|(row, s)| s.score.map(|score| (row, score)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .ok_or("no complete row")?;
    let rule = &parameters[2 * best..2 * best + 2];
    println!(
        "{rows} rows, {invalid} invalid; best row {best}: target {:.3}, gain {:.3}, cost {score:.1}, steps {}",
        rule[0], rule[1], scores[best].steps
    );

    // Replay the winner as a full JSON record for its diagnostics.
    let replay = prepared.evaluate_json(
        &json!([{"id": "best", "parameters": rule}]).to_string(),
        "{}",
    )?;
    let replay: Value = serde_json::from_str(&replay).map_err(|e| e.to_string())?;
    let record = &replay["records"][0];
    assert_eq!(record["score"].as_f64(), Some(score));
    println!("metrics: {}", record["metrics"]);
    Ok(())
}
