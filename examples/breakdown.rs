//! Phase breakdown of prepared JSON calls: decode, evaluate and encode, plus
//! the typed matrix call on the same candidates. Run with
//! `cargo run --release --features profiling --example breakdown -- models/example.json`.
use hydro_craft::Prepared;
use serde_json::{Value, json};
use std::{hint::black_box, time::Instant};

/// Build the same models and candidates as `examples/performance.rs`.
fn workload(example: &Value, mode: &str) -> (Value, Vec<Value>, usize) {
    let mut model = example["model"].clone();
    let mut parameters = example["parameters"].clone();
    let (count, workers) = match mode {
        "one" => (1, 1),
        "serial" => (96, 1),
        "parallel" => (96, 4),
        "invalid" => (1024, 1),
        "domain" => {
            let year = model["forcing"].as_array().unwrap().clone();
            model["forcing"] = json!(
                (0..2400)
                    .map(|i| {
                        let mut day = year[i % year.len()].clone();
                        day["day"] = json!(i);
                        day
                    })
                    .collect::<Vec<_>>()
            );
            model["warmup"] = json!(300);
            model["train_end"] = json!(1200);
            model["validation_end"] = json!(1800);
            (48, 1)
        }
        "domain2" => {
            model = json!({"kind":"reservoir","capacity_m3":1200,"initial_m3":300,"outlet_m3_per_day":20,"reserve_m3":120,"inputs":(0..2400).map(|i|json!({"inflow_m3":i%50,"evaporation_m3":1,"demand_m3":22})).collect::<Vec<_>>()});
            parameters = json!([0.5, 0.1]);
            (48, 1)
        }
        _ => panic!("unknown workload"),
    };
    let candidates = (0..count)
        .map(|i| {
            let mut values = parameters.clone();
            if mode == "invalid" {
                values = json!(vec![1.0; 63]);
            } else if let Some(first) = values.as_array_mut().unwrap().first_mut() {
                *first = json!(first.as_f64().unwrap() * (1.0 + (i % 13) as f64 * 0.003));
            }
            json!({"id": format!("candidate-{i:04}"), "parameters": values})
        })
        .collect();
    (model, candidates, workers)
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn main() {
    let path = std::env::args().nth(1).expect("example JSON");
    let example: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let repeats = 201;
    let mut rows = Vec::new();
    for mode in ["one", "serial", "parallel", "invalid", "domain", "domain2"] {
        let (model, candidates, workers) = workload(&example, mode);
        let prepared = Prepared::new(&model.to_string()).unwrap();
        let budget = json!({"workers": workers}).to_string();
        let text = Value::from(candidates.clone()).to_string();
        let flat: Vec<f64> = candidates
            .iter()
            .flat_map(|c| c["parameters"].as_array().unwrap().clone())
            .map(|v| v.as_f64().unwrap())
            .collect();
        for api in ["full", "lean", "matrix"] {
            let mut phases = [Vec::new(), Vec::new(), Vec::new()];
            let mut totals = Vec::new();
            for _ in 0..repeats {
                if api == "matrix" {
                    let start = Instant::now();
                    black_box(
                        prepared
                            .score_matrix(candidates.len(), &flat, &budget)
                            .unwrap(),
                    );
                    totals.push(start.elapsed().as_secs_f64());
                } else {
                    let t = prepared
                        .profile_json(&text, &budget, api == "lean")
                        .unwrap();
                    for (phase, time) in phases.iter_mut().zip(t) {
                        phase.push(time.as_secs_f64());
                    }
                    totals.push(t.iter().map(|d| d.as_secs_f64()).sum());
                }
            }
            let us = |v: Vec<f64>| {
                if v.is_empty() {
                    Value::Null
                } else {
                    json!(median(v) * 1e6)
                }
            };
            let [decode, evaluate, encode] = phases;
            rows.push(
                json!({"workload": mode, "api": api, "candidates": candidates.len(),
                "workers": workers, "decode_us": us(decode), "evaluate_us": us(evaluate),
                "encode_us": us(encode), "total_us": us(totals)}),
            );
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"repeats": repeats, "rows": rows})).unwrap()
    );
}
