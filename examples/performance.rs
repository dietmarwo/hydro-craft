use hydro_craft::Prepared;
use serde_json::{Value, json};
use std::{hint::black_box, time::Instant};

fn main() {
    let mut args = std::env::args().skip(1);
    let example = std::fs::read_to_string(args.next().expect("example JSON")).unwrap();
    let example: Value = serde_json::from_str(&example).unwrap();
    let mode = args.next().expect("workload");
    let mut model = example["model"].clone();
    #[allow(unused_mut)]
    let mut parameters = example["parameters"].clone();
    let (count, workers, traced, repeats) = match mode.as_str() {
        "one" => (1, 1, false, 40),
        "serial" => (96, 1, false, 4),
        "parallel" => (96, 4, false, 4),
        "trace" => (12, 1, true, 6),
        "invalid" => (1024, 1, false, 6),
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
            (48, 1, false, 5)
        }
        "domain2" => {
            model = json!({"kind":"reservoir","capacity_m3":1200,"initial_m3":300,"outlet_m3_per_day":20,"reserve_m3":120,"inputs":(0..2400).map(|i|json!({"inflow_m3":i%50,"evaporation_m3":1,"demand_m3":22})).collect::<Vec<_>>()});
            parameters = json!([0.5, 0.1]);
            (48, 1, false, 5)
        }
        _ => panic!("unknown workload"),
    };
    let candidates: Vec<_> = (0..count)
        .map(|i| {
            let mut values = parameters.clone();
            if mode == "invalid" {
                values = json!(vec![1.0; 63]);
            } else if let Some(first) = values.as_array_mut().unwrap().first_mut() {
                *first = json!(first.as_f64().unwrap() * (1.0 + (i % 13) as f64 * 0.003));
            }
            json!({"id": format!("candidate-{i:04}"), "parameters": values, "trace": traced})
        })
        .collect();
    // Row-major parameters for the matrix API, built before timing like the JSON text.
    let flat: Vec<f64> = candidates
        .iter()
        .flat_map(|c| c["parameters"].as_array().unwrap().clone())
        .map(|v| v.as_f64().unwrap())
        .collect();
    let candidates = serde_json::to_string(&candidates).unwrap();
    let budget = json!({"workers":workers}).to_string();
    let evaluator = Prepared::new(&model.to_string()).unwrap();
    let lean = std::env::var_os("CRAFT_SCORES").is_some();
    let matrix = std::env::var_os("CRAFT_MATRIX").is_some();
    let probe = args.next().as_deref() == Some("probe");
    if matrix {
        assert!(!traced, "the matrix API evaluates untraced candidates only");
        let evaluate = || evaluator.score_matrix(count, &flat, &budget).unwrap();
        let first = evaluate();
        if probe {
            let records: Vec<_> = first
                .iter()
                .enumerate()
                .map(|(i, row)| {
                    json!({"id": format!("candidate-{i:04}"), "status": row.status,
                           "steps": row.steps, "score": row.score, "metrics": null, "detail": null})
                })
                .collect();
            println!(
                "{}",
                json!({"model_sha256": evaluator.model_sha256(), "records": records,
                       "schema": concat!(env!("CARGO_PKG_NAME"), ".result/0.1"),
                       "version": env!("CARGO_PKG_VERSION")})
            );
            return;
        }
        black_box(first);
        let start = Instant::now();
        for _ in 0..repeats {
            black_box(evaluate());
        }
        println!("{}", start.elapsed().as_secs_f64() / f64::from(repeats));
        return;
    }
    let evaluate = || {
        if lean {
            evaluator.scores_json(&candidates, &budget)
        } else {
            evaluator.evaluate_json(&candidates, &budget)
        }
    };
    let first = evaluate().unwrap();
    if probe {
        println!("{first}");
        return;
    }
    black_box(first);
    let start = Instant::now();
    for _ in 0..repeats {
        black_box(evaluate().unwrap());
    }
    println!("{}", start.elapsed().as_secs_f64() / f64::from(repeats));
}
