# hydro-craft user manual

Version 0.3.0. hydro-craft evaluates two small daily water models for
research and optimization: a synthetic HBV-style **catchment** that turns
precipitation, temperature and potential evapotranspiration into runoff,
and a **reservoir** operated by a two-parameter release rule. Both are
exposed through one prepared, batch-oriented evaluator in Rust, a Python
package and a command-line tool. Every evaluation is a minimization: lower
`score` is better.

This manual is for researchers and engineers who use the model. It covers
installation, concepts, the complete model and request reference, every
API, budgets, reproducibility, performance and troubleshooting. All Python
blocks in this manual are executed by `validation/release_checks.py`
against the installed wheel, in order and in one namespace. Shell
outputs shown here were copied from actual runs.

The catchment is an independently specified synthetic fixture, not a
port of a published HBV code; see the [model card](model.md) and the
[source notes](sources.md) for scope.

## Contents

1. [Install](#1-install)
2. [Concepts](#2-concepts)
3. [Model reference](#3-model-reference)
4. [Candidates, budgets and results](#4-candidates-budgets-and-results)
5. [Statuses and error messages](#5-statuses-and-error-messages)
6. [Budgets](#6-budgets)
7. [Python API](#7-python-api)
8. [Scenario cookbook](#8-scenario-cookbook)
9. [Optimization](#9-optimization)
10. [Campaigns](#10-campaigns)
11. [Rust API](#11-rust-api)
12. [Command line](#12-command-line)
13. [Reproducibility and provenance](#13-reproducibility-and-provenance)
14. [Performance tips](#14-performance-tips)
15. [Troubleshooting](#15-troubleshooting)
16. [Examples index](#16-examples-index)

## 1. Install

### Wheel

The release ZIP contains a wheel for Linux x86-64 with the CPython 3.10+
stable ABI (tested with CPython 3.12). Install it into a virtual
environment; it has no runtime dependencies.

```sh
python3 -m venv .venv
. .venv/bin/activate
python -m pip install --no-deps packages/hydro_craft-0.3.0-cp310-abi3-manylinux_2_34_x86_64.whl
```

The optimization examples additionally use NumPy, SciPy and fcmaes:

```sh
python -m pip install numpy==2.3.5 scipy==1.17.0 fcmaes
```

### From source

On other platforms, build the wheel from the source distribution or the
source tree with Rust 1.88 or newer and maturin 1.9.6. Cargo needs the
locked dependencies from its registry or cache.

```sh
python -m pip install maturin==1.9.6
tar -xzf packages/hydro_craft-0.3.0.tar.gz
cd hydro_craft-0.3.0
maturin build --release --locked --out dist
python -m pip install --no-deps dist/*.whl
```

### Command-line tool and Rust crate

The CLI is a Rust binary built from the same crate:

```sh
cargo build --release --locked
target/release/hydro-craft describe
```

```text
{"batch":true,"models":["synthetic_daily_hbv_style","masked_runoff_calibration","one_reservoir"],"schema":"hydro-craft.capabilities/0.1","version":"0.3.0","worker_limit":64}
```

To use the library from another Rust project, depend on the crate by
path or from the packaged `hydro-craft-0.3.0.crate` (see
[section 11](#11-rust-api)).

Check the installation:

```python
import hydro_craft
from hydro_craft import Prepared, load_example

case = load_example()
prepared = Prepared(case["model"])
row = prepared.evaluate(case["parameters"])
print(row["status"], row["steps"], row["score"])
```

```text
complete 90 4.965068306494546e-16
```

## 2. Concepts

- **Model.** A JSON object with a `kind` of `"catchment"` or
  `"reservoir"` that fixes every input that does not change between
  candidates: forcing or inflows, fold boundaries, capacities.
- **Prepared model.** `Prepared(model)` validates and stores the model
  once and records the SHA-256 of its exact JSON bytes
  (`model_sha256`). Every later call reuses it. A malformed model raises
  `ValueError` (Rust: `Err(String)`).
- **Candidate.** One parameter vector to evaluate: seven catchment
  parameters or two reservoir controls. JSON calls wrap it with an `id`
  and an optional `trace` flag; the matrix API takes bare rows.
- **Batch.** Up to 4,096 candidates evaluated in one native call.
  Records come back in input order whatever the worker count.
- **Record.** The outcome of one candidate: `status`, logical `steps`,
  `score` (only when complete), `metrics` (full records only) and
  `detail` (only when not complete).
- **Status.** `complete`, or a classified failure such as `invalid`
  (bad parameters) or `work_limit` (budget). A failed candidate never
  fails the batch; a malformed batch is rejected as a whole.
- **Steps.** One logical step per simulated day. A complete candidate
  takes exactly as many steps as the model has days.
- **Budget.** Per-call limits: `workers`, `max_steps`, `wall_ms`,
  `cancel_file`.
- **Three result levels.** `batch` returns full records with metrics and
  optional daily traces; `scores` returns the same records with
  `metrics: null`; `score_matrix` returns only scores, statuses and
  steps as three lists, with no JSON in either direction. All three give
  identical statuses, steps and scores.
- **Folds.** The catchment splits its days into warm-up, train,
  validation and test. The score is the training RMSE; validation and
  test fits are reported in full records for model selection and the
  final check.

## 3. Model reference

Both models reject unknown fields and missing required fields. Numbers
are JSON numbers; the Python wrapper refuses NaN and infinity in models
and JSON calls.

### 3.1 Catchment (`"kind": "catchment"`)

| Field | Type | Unit | Bounds | Default |
| --- | --- | --- | --- | --- |
| `kind` | string | - | `"catchment"` | required |
| `area_km2` | number | km² | `0 < area_km2 <= 1e7` | required |
| `forcing` | array of days | - | 3 to 200,000 days | required |
| `warmup` | integer | day index | `warmup < train_end` | required |
| `train_end` | integer | day index | `train_end < validation_end` | required |
| `validation_end` | integer | day index | `validation_end < len(forcing)` | required |

Each forcing day:

| Field | Type | Unit | Bounds | Default |
| --- | --- | --- | --- | --- |
| `day` | integer | day number | consecutive: each day is the previous plus 1 (any start, 0 to 2³²-1) | required |
| `precipitation_mm` | number | mm/day | 0 to 2,000 | required |
| `pet_mm` | number | mm/day | 0 to 100 | required |
| `temperature_c` | number | °C | -100 to 100 | required |
| `observed_mm` | number or null | mm/day | 0 to 2,000 | null (may be omitted) |

Days `0..warmup` are warm-up: simulated but never scored. Days
`warmup..train_end` are the **train** fold, `train_end..validation_end`
the **validation** fold and `validation_end..` the **test** fold (index
ranges are half-open). Every scored fold needs at least one non-null
observation. A violated rule raises one of
`invalid daily forcing, units, fold or catchment area` or
`every scored fold needs observed data`.

Parameters, in this order:

| Index | Name | Unit | Valid range | Meaning |
| ---: | --- | --- | --- | --- |
| 0 | `capacity` | mm | 1 to 2,000 | soil moisture capacity |
| 1 | `exponent` | - | 0.1 to 10 | recharge shape exponent |
| 2 | `lp` | - | 0.05 to 1 | fraction of capacity above which evapotranspiration is unrestricted |
| 3 | `kfast` | 1/day | 0 to 1 | upper-store recession fraction |
| 4 | `kbase` | 1/day | 0 to 1 | lower-store recession fraction |
| 5 | `percolation` | mm/day | 0 to 100 | upper-to-lower transfer limit |
| 6 | `degree` | mm/(°C day) | 0 to 30 | degree-day melt factor |

A vector of the wrong length is `invalid` with detail
`expected capacity, exponent, lp, kfast, kbase, percolation, degree`;
a value outside its range (or NaN in the matrix API) is `invalid` with
`catchment parameter out of range`.

Daily update (stores start empty: snow, soil, upper, lower):

1. At or below 0 °C precipitation joins the snow store. Above 0 °C,
   melt is `min(snow, degree * temperature)` and liquid water is
   precipitation plus melt.
2. Recharge is `liquid * (soil / capacity) ** exponent`, using the soil
   store before today's input; the rest of the liquid water enters the
   soil. Soil above capacity overflows.
3. Evapotranspiration is `min(soil, pet * min(soil / (lp * capacity), 1))`.
4. Recharge and overflow enter the upper store; at most `percolation`
   moves on to the lower store.
5. Runoff is `kfast * upper + kbase * lower`, removed from the stores.

Score: root-mean-square error in mm/day between runoff and the non-null
observations of the train fold. Full-record metrics:

| Metric | Unit | Meaning |
| --- | --- | --- |
| `train`, `validation`, `test` | object | `observations` (count), `rmse_mm` (mm/day) and `volume_bias_mm` (sum of runoff minus observed, mm) per fold |
| `balance_mm` | mm | sum of daily mass-balance residuals over all days |
| `max_daily_balance_mm` | mm | largest absolute daily residual |
| `trace` | array or null | daily rows when `trace: true` (at most 10,000 days) |

Trace rows: `day`, `runoff_mm`, `discharge_m3_s` (runoff × area ×
1,000 / 86,400), `et_mm`, `stores_mm` (`[snow, soil, upper, lower]` at
the end of the day) and `balance_mm` (that day's residual).

### 3.2 Reservoir (`"kind": "reservoir"`)

| Field | Type | Unit | Bounds | Default |
| --- | --- | --- | --- | --- |
| `kind` | string | - | `"reservoir"` | required |
| `capacity_m3` | number | m³ | `0 < capacity_m3 <= 1e12` | required |
| `initial_m3` | number | m³ | 0 to `capacity_m3` | required |
| `outlet_m3_per_day` | number | m³/day | 0 to 1e12 | required |
| `reserve_m3` | number | m³ | 0 to `capacity_m3` | required |
| `inputs` | array of days | - | 1 to 200,000 days | required |

Each input day has `inflow_m3`, `evaporation_m3` and `demand_m3`, all
required numbers in m³ from 0 to 1e12. A violation raises
`invalid reservoir storage, capacity, outlet, or daily inputs`.

Parameters: `[target, gain]`, both in `[0, 1]`. A wrong length is
`invalid` with `expected target fraction and release gain`; an
out-of-range value is `invalid` with `reservoir controls must be in [0,1]`.

Daily operation, in order:

1. Discretionary release request: `gain * max(previous storage - target * capacity, 0)`,
   decided from the previous day's storage only (non-anticipatory).
2. Add inflow; evaporate `min(storage, evaporation)`.
3. Available outlet water: `min(max(storage - reserve, 0), outlet)`.
   Deliver `min(available, demand)`; shortage is demand minus delivery.
4. Release `min(available - delivered, request)`.
5. Storage above capacity spills.

Score: total shortage plus 0.05 × total spill (m³). Full-record metrics:
`shortage_m3`, `spill_m3`, `shortage_days`, `longest_shortage_days`,
`max_balance_m3` (largest absolute daily mass-balance residual) and
`trace`. Trace rows: `storage_m3` (end of day), `delivered_m3`,
`release_m3`, `shortage_m3`, `spill_m3` and `evaporation_m3`.

```python
reservoir_model = {
    "kind": "reservoir", "capacity_m3": 100, "initial_m3": 40,
    "outlet_m3_per_day": 20, "reserve_m3": 10,
    "inputs": [{"inflow_m3": 8, "evaporation_m3": 1, "demand_m3": 15},
               {"inflow_m3": 4, "evaporation_m3": 1, "demand_m3": 16},
               {"inflow_m3": 90, "evaporation_m3": 2, "demand_m3": 5}],
}
reservoir = Prepared(reservoir_model)
row = reservoir.evaluate([0.3, 0.5], trace=True)
print(row["status"], row["steps"], row["score"])
for day in row["metrics"]["trace"]:
    print(day)
print({k: v for k, v in row["metrics"].items() if k != "trace"})
```

```text
complete 3 0.0
{'delivered_m3': 15.0, 'evaporation_m3': 1.0, 'release_m3': 5.0, 'shortage_m3': 0.0, 'spill_m3': 0.0, 'storage_m3': 27.0}
{'delivered_m3': 16.0, 'evaporation_m3': 1.0, 'release_m3': 0.0, 'shortage_m3': 0.0, 'spill_m3': 0.0, 'storage_m3': 14.0}
{'delivered_m3': 5.0, 'evaporation_m3': 2.0, 'release_m3': 0.0, 'shortage_m3': 0.0, 'spill_m3': 0.0, 'storage_m3': 97.0}
{'longest_shortage_days': 0, 'max_balance_m3': 0.0, 'shortage_days': 0, 'shortage_m3': 0.0, 'spill_m3': 0.0}
```

## 4. Candidates, budgets and results

### Candidates (JSON calls)

| Field | Type | Rules | Default |
| --- | --- | --- | --- |
| `id` | string | 1 to 128 bytes, unique within the batch | required |
| `parameters` | array of numbers | at most 64 values | required |
| `trace` | boolean | at most 16 traced candidates per batch; not allowed in `scores` | `false` |

A batch holds 1 to 4,096 candidates. The matrix API takes 1 to 4,096
rows of equal length (0 to 64 values) and has no IDs.

### Budget

| Field | Type | Range | Default | Meaning |
| --- | --- | --- | --- | --- |
| `workers` | integer | 1 to 64 | 1 | upper bound on concurrent evaluators |
| `max_steps` | integer | 1 to 100,000,000 | 1,000,000 | step limit per candidate |
| `wall_ms` | integer or null | 1 to 86,400,000 | null | batch deadline in milliseconds |
| `cancel_file` | string or null | path | null | stop when this file exists |

In Python the budget is passed as keyword arguments to `batch`,
`scores`, `score_matrix`, `evaluate` and `campaign.run`.

### Result envelope

`batch` and `scores` return:

| Field | Meaning |
| --- | --- |
| `model_sha256` | SHA-256 of the exact model JSON bytes |
| `records` | one record per candidate, in input order |
| `schema` | `"hydro-craft.result/0.1"` |
| `version` | package version, `"0.3.0"` |

Record fields: `id`, `status`, `steps`, `score` (number, only for
`complete`), `metrics` (object for complete full records, otherwise
null) and `detail` (string for every status except `complete`).

### CLI request

The command line reads one request object:

| Field | Type | Default |
| --- | --- | --- |
| `schema` | `"hydro-craft.request/0.1"` | required |
| `model` | model object | required |
| `candidates` | array of candidates | required |
| `budget` | budget object | defaults above |

The CLI always returns full records; its output has sorted keys.

## 5. Statuses and error messages

### Record statuses

| Status | Meaning | Steps | Typical detail |
| --- | --- | --- | --- |
| `complete` | simulated every day; `score` is set | number of days | null |
| `invalid` | parameters rejected before the first step, or a traced run longer than 10,000 days | 0 | see below |
| `work_limit` | the candidate reached `max_steps` | `max_steps` | `candidate step budget reached` |
| `wall_limit` | the batch deadline passed | steps taken | `batch deadline reached` |
| `cancelled` | the cancel file existed at a step | steps taken | `cancel file exists` |

`infeasible` is part of the shared result schema but neither model
produces it. Invalid details:
`expected capacity, exponent, lp, kfast, kbase, percolation, degree`,
`catchment parameter out of range`,
`expected target fraction and release gain`,
`reservoir controls must be in [0,1]`,
`trace requires at most 10,000 days` and `nonfinite objective`.

### Rejected calls

These raise `ValueError` in Python, return `Err(String)` in Rust and
exit with status 1 on the command line:

| Message | Cause |
| --- | --- |
| `invalid budget or batch size` | workers, `max_steps` or `wall_ms` out of range, or 0 or more than 4,096 candidates |
| `invalid or duplicate candidate id, or parameter vector exceeds limit` | empty, long or repeated ID, or more than 64 parameters |
| `at most 16 traced candidates per batch` | too many `trace: true` |
| `scores mode accepts untraced candidates only` | `trace: true` passed to `scores` |
| `matrix rows must be between 1 and 4096` | empty or oversized matrix |
| `parameter count is not a multiple of the row count` | Rust `score_matrix` with a ragged flat buffer |
| `parameter vector exceeds limit of 64` | matrix rows longer than 64 |
| `score_matrix rows must all have the same length` | ragged Python rows |
| `score_matrix expects a 2-D array of parameter rows` | NumPy array with another shape |
| `unknown field ...`, `missing field ...`, `invalid type ...` | JSON shape errors, with position |
| model messages from section 3 | invalid model |
| `unsupported request schema` | CLI request with another schema tag |
| `request exceeds 16 MiB` | CLI input too large |

```python
for bad in ([{"id": "a", "parameters": []}] * 2, []):
    try:
        prepared.batch(bad)
    except ValueError as error:
        print("rejected:", error)
row = prepared.evaluate([1.0, 2.0])
print(row["status"], row["steps"], row["detail"])
row = prepared.evaluate([0.5] + case["parameters"][1:])
print(row["status"], row["detail"])
```

```text
rejected: invalid or duplicate candidate id, or parameter vector exceeds limit
rejected: invalid budget or batch size
invalid 0 expected capacity, exponent, lp, kfast, kbase, percolation, degree
invalid catchment parameter out of range
```

## 6. Budgets

`workers` is an upper bound. One worker, or a single candidate, runs on
the calling thread. More workers use persistent pool threads, created
once per process, with the calling thread taking part; records keep
input order and results do not depend on the worker count.

`max_steps` limits each candidate separately. A candidate that needs
more steps stops with `work_limit` after exactly `max_steps` steps.

`wall_ms` sets one deadline for the whole batch, measured from when the
batch starts. `cancel_file` stops every candidate whose next step finds
the file present. Both are cooperative and checked before each step, so
they can change which candidates finish; fixed-work results do not
depend on timing. Monitored budgets (`wall_ms` or `cancel_file`) and a
`max_steps` below the model's day count make the engine run candidates
one at a time instead of in interleaved groups, which is slower (see
[section 14](#14-performance-tips)).

```python
from pathlib import Path
from tempfile import TemporaryDirectory

x = case["parameters"]
print(prepared.evaluate(x, max_steps=10)["status"], prepared.evaluate(x, max_steps=90)["status"])
same = prepared.batch([{"id": str(i), "parameters": x} for i in range(8)], workers=4)
print({r["status"] for r in same["records"]}, prepared.evaluate(x, wall_ms=60_000)["score"])
with TemporaryDirectory() as folder:
    flag = Path(folder) / "stop"
    flag.touch()
    print(prepared.score_matrix([x, x], cancel_file=str(flag)))
```

```text
work_limit complete
{'complete'} 4.965068306494546e-16
([nan, nan], ['cancelled', 'cancelled'], [0, 0])
```

## 7. Python API

```python
from hydro_craft import Prepared, load_example
from hydro_craft import campaign
```

### `load_example() -> dict`

Returns the packaged 90-day catchment fixture as
`{"model": ..., "parameters": [...]}`. The parameters reproduce its
synthetic observations.

### `Prepared(model: dict)`

Validates and stores a model. The wrapper encodes the dict as compact
JSON with sorted keys, so equal dicts give equal `model_sha256`.

### `Prepared.batch(candidates, **budget) -> dict`

Full records with metrics and optional traces. One native call per batch.

### `Prepared.scores(candidates, **budget) -> dict`

The same envelope and records with `metrics: null`; skips diagnostic
work. Rejects traced candidates.

### `Prepared.score_matrix(rows, **budget) -> (scores, statuses, steps)`

`rows` is a list of equal-length lists or a 2-D NumPy array. Returns
three lists in row order; `scores` holds NaN for every row that is not
complete. No IDs, no JSON in either direction, and the GIL is released
during evaluation.

### `Prepared.evaluate(parameters, *, trace=False, **budget) -> dict`

One full record (ID `"one"`), convenient for replay and inspection.

```python
candidates = [{"id": f"cap-{c}", "parameters": [c] + case["parameters"][1:]}
              for c in (8.0, 10.0, 12.0)]
full = prepared.batch(candidates)
lean = prepared.scores(candidates)
scores, statuses, steps = prepared.score_matrix([c["parameters"] for c in candidates])
for f, l, s in zip(full["records"], lean["records"], scores):
    assert f["score"] == l["score"] == s and l["metrics"] is None
    print(f["id"], f["status"], f["steps"], round(f["score"], 6),
          round(f["metrics"]["validation"]["rmse_mm"], 6))
print(full["schema"], full["version"], full["model_sha256"] == lean["model_sha256"])
```

```text
cap-8.0 complete 90 0.043187 0.055452
cap-10.0 complete 90 0.0 0.0
cap-12.0 complete 90 0.043985 0.05243
hydro-craft.result/0.1 0.3.0 True
```

NumPy arrays go straight to `score_matrix`:

```python
import numpy as np

population = np.array([case["parameters"]] * 3, dtype=float)
population[1, 0] *= 1.05  # capacity +5 %
population[2, 2] = 1.2    # lp outside its range
scores, statuses, steps = prepared.score_matrix(population, workers=2)
print([round(v, 5) for v in scores], statuses, steps)
```

```text
[0.0, 0.00896, nan] ['complete', 'complete', 'invalid'] [90, 90, 0]
```

The third row sets `lp` to 1.2, outside its range, so it is invalid and
its score is NaN.

## 8. Scenario cookbook

### Calibrate on train, select on validation, report test

```python
grid = [0.2, 0.25, 0.3, 0.35, 0.4]
rows = [case["parameters"][:4] + [k] + case["parameters"][5:] for k in grid]
scores, _, _ = prepared.score_matrix(rows)
best = rows[min(range(len(rows)), key=scores.__getitem__)]
final = prepared.evaluate(best)["metrics"]
print("kbase", best[4], {fold: round(final[fold]["rmse_mm"], 12) for fold in ("train", "validation", "test")})
```

```text
kbase 0.3 {'train': 0.0, 'validation': 0.0, 'test': 0.0}
```

### Missing observations and fold boundaries

Null observations are skipped by the score and the fold metrics, but the
day is still simulated. Moving a boundary changes which observations
count.

```python
model = case["model"]
masked = model | {"forcing": [d | {"observed_mm": None} if d["day"] % 3 == 0 else d
                              for d in model["forcing"]]}
shifted = model | {"warmup": 20, "train_end": 45}
for name, variant in (("packaged", model), ("masked", masked), ("shifted", shifted)):
    m = Prepared(variant).evaluate([12.0] + case["parameters"][1:])["metrics"]
    print(name, [m[f]["observations"] for f in ("train", "validation", "test")], round(m["train"]["rmse_mm"], 4))
```

```text
packaged [20, 27, 27] 0.044
masked [13, 18, 18] 0.0475
shifted [22, 14, 27] 0.0371
```

### Snow, melt and the water balance

```python
cold = model | {"forcing": [d | {"temperature_c": -5.0 if d["day"] < 30 else 6.0}
                            for d in model["forcing"]]}
traced = Prepared(cold).evaluate(case["parameters"], trace=True)["metrics"]
snow = [day["stores_mm"][0] for day in traced["trace"]]
print("peak snow", round(max(snow), 2), "mm on day", snow.index(max(snow)),
      "| melted by day", next(i for i, s in enumerate(snow) if i > 30 and s == 0))
print("max daily balance", traced["max_daily_balance_mm"] < 1e-12)
```

```text
peak snow 219.32 mm on day 29 | melted by day 42
max daily balance True
```

### Discharge and runoff traces

```python
trace = prepared.evaluate(case["parameters"], trace=True)["metrics"]["trace"]
day = max(trace, key=lambda d: d["runoff_mm"])
print(day["day"], round(day["runoff_mm"], 3), "mm =", round(day["discharge_m3_s"], 3),
      "m3/s over", model["area_km2"], "km2")
```

```text
17 17.393 mm = 17.393 m3/s over 86.4 km2
```

### Recharge exponent classes

Exponents of exactly 1 or 2 use direct arithmetic and the fastest lane
kernels; other values call the power function. Results are the same
formula either way.

```python
for exponent in (1.0, 1.5, 2.0):
    x = case["parameters"][:1] + [exponent] + case["parameters"][2:]
    print(exponent, round(prepared.evaluate(x)["score"], 6))
```

```text
1.0 0.0
1.5 0.089461
2.0 0.134889
```

### Reservoir rules and scenario comparison

Dry inflows favor keeping water (no discretionary release); wet inflows
favor releasing above a low target to avoid spill.

```python
def reservoir_case(scale):
    """90 days with three wet days in every ten; scale shifts the inflow."""
    inputs = [{"inflow_m3": scale * (40 if d % 10 < 3 else 4), "evaporation_m3": 1,
               "demand_m3": 14} for d in range(90)]
    return Prepared({"kind": "reservoir", "capacity_m3": 150, "initial_m3": 75,
                     "outlet_m3_per_day": 25, "reserve_m3": 20, "inputs": inputs})

rules = [[t / 4, g / 4] for t in range(5) for g in range(5)]
for scale in (0.8, 1.0, 1.6):
    scores, _, _ = reservoir_case(scale).score_matrix(rules)
    i = min(range(len(rules)), key=scores.__getitem__)
    print(scale, rules[i], round(scores[i], 2))
```

```text
0.8 [0.0, 0.0] 229.4
1.0 [0.0, 0.0] 0.0
1.6 [0.25, 0.75] 1.95
```

### Many scenarios, one candidate set

Prepare one model per scenario and reuse each:

```python
scenarios = {f"x{scale}": reservoir_case(scale) for scale in (0.8, 1.0, 1.6)}
policy = [[0.5, 0.25]]
print({name: round(p.score_matrix(policy)[0][0], 2) for name, p in scenarios.items()})
print(scenarios["x0.8"].evaluate(policy[0])["metrics"]["shortage_days"])
```

```text
{'x0.8': 255.7, 'x1.0': 58.37, 'x1.6': 15.4}
25
```

## 9. Optimization

Optimizers minimize `score`. Give failed candidates a penalty that is
worse than any real score and, preferably, grows with the distance
outside the valid ranges, so that the penalty points back into the
valid region:

```python
LIMITS = [(1, 2000), (0.1, 10), (0.05, 1), (0, 1), (0, 1), (0, 100), (0, 30)]

def objective_rows(rows):
    """Penalized scores for a population of catchment parameter rows."""
    scores, statuses, _ = prepared.score_matrix(rows)
    out = []
    for x, score, status in zip(rows, scores, statuses):
        if status == "complete":
            out.append(score)
        else:
            outside = sum(max(lo - v, 0, v - hi) for v, (lo, hi) in zip(x, LIMITS))
            out.append(1e3 * (1 + outside))
    return out

print(objective_rows([case["parameters"], [10, 1, 1.3, 0.5, 0.3, 2, 3]]))
```

```text
[4.965068306494546e-16, 1300.0]
```

A short SciPy differential evolution with a vectorized objective sends
one generation per native call:

```python
from scipy.optimize import differential_evolution

bounds = [(5, 15), (0.5, 2), (0.5, 1.2), (0.2, 0.8), (0.1, 0.5), (1, 3), (1, 5)]
result = differential_evolution(lambda pop: objective_rows(pop.T), bounds, vectorized=True,
                                updating="deferred", popsize=8, maxiter=40, seed=1, polish=False)
print(round(result.fun, 4), result.nit, "generations")
```

```text
0.0128 40 generations
```

The same pattern works with fcmaes ask/tell (`fcmaes.cmaes.Cmaes`,
`fcmaes.de.DE`) and with fcmaes minimizers that call a scalar objective
(`bitecpp.minimize`, `cmaescpp.minimize`, `de.minimize`); wrap a single
vector as `prepared.score_matrix([x])`. Complete, longer examples are in
[`examples/python/`](../examples/python/README.md): SciPy calibration,
CMA-ES with whole populations and native workers, and robust reservoir
rules with BiteOpt and C++ CMA-ES.

`validation/optimize_performance.py` times one seeded SciPy search with
`evaluate`, `scores` or `score_matrix` (`--scores`, `--matrix`); all
three give the same evaluations, failures and result.

## 10. Campaigns

`campaign.run(database, scenarios, candidates, *, workers=1, chunk_size=16, **budget)`
evaluates every candidate under every scenario, commits each chunk to
SQLite and returns rows in manifest order. Completed, invalid and
infeasible records are cached by build, scenario, model, candidate and
work settings; canceled and time-limited attempts stay in the attempt
journal and are retried on the next run. A new wheel build uses its own
cache namespace.

```python
import sqlite3

with TemporaryDirectory() as folder:
    database = Path(folder) / "campaign.sqlite"
    flag = Path(folder) / "stop"
    scenario_list = [{"id": "packaged", "model": model}, {"id": "masked", "model": masked}]
    trials = [{"id": f"k{k}", "parameters": case["parameters"][:4] + [k] + case["parameters"][5:]}
              for k in (0.2, 0.3, 0.4)]
    flag.touch()
    stopped = campaign.run(database, scenario_list, trials, cancel_file=str(flag))
    flag.unlink()
    resumed = campaign.run(database, scenario_list, trials, workers=2)
    again = campaign.run(database, scenario_list, trials)
    print([r["record"]["status"] for r in stopped][:2], [r["record"]["status"] for r in resumed][:2])
    print(resumed == again, sorted({r["scenario"] for r in resumed}))
    with sqlite3.connect(database) as conn:
        print(conn.execute("SELECT count(*) FROM results").fetchone()[0],
              conn.execute("SELECT count(*) FROM attempts").fetchone()[0])
```

```text
['cancelled', 'cancelled'] ['complete', 'complete']
True ['masked', 'packaged']
6 12
```

The repository's `validation/run_campaign.py MODULE models/campaign.json DB`
runs a JSON manifest with the same function.

## 11. Rust API

```toml
[dependencies]
hydro-craft = { path = "../hydro-craft" }
serde_json = "1"
```

| Item | Signature | Notes |
| --- | --- | --- |
| `Prepared::new` | `fn new(model: &str) -> Result<Prepared, String>` | validates, stores and hashes the model JSON |
| `Prepared::model_sha256` | `fn model_sha256(&self) -> &str` | SHA-256 of the model bytes |
| `Prepared::evaluate_json` | `fn evaluate_json(&self, candidates: &str, budget: &str) -> Result<String, String>` | full records |
| `Prepared::scores_json` | `fn scores_json(&self, candidates: &str, budget: &str) -> Result<String, String>` | records with null metrics |
| `Prepared::score_matrix` | `fn score_matrix(&self, rows: usize, parameters: &[f64], budget: &str) -> Result<Vec<MatrixScore>, String>` | row-major parameters; dimension `parameters.len() / rows`, 0 allowed |
| `MatrixScore` | `{ status: &'static str, steps: u64, score: Option<f64> }` | `score` is `Some` only for `complete` |
| `run` | `fn run(text: &str) -> Result<serde_json::Value, String>` | one CLI request |
| `capabilities` | `fn capabilities() -> serde_json::Value` | the `describe` output |

```rust
use hydro_craft::Prepared;

let prepared = Prepared::new(&model_json)?;
let rows = vec![0.5, 0.1, 0.9, 0.0]; // two reservoir rules, row-major
for row in prepared.score_matrix(2, &rows, r#"{"workers": 1}"#)? {
    println!("{} {} {:?}", row.status, row.steps, row.score);
}
```

`examples/matrix.rs` is a complete program
(`cargo run --release --example matrix`); `examples/sweep.rs` scales the
packaged parameters and prints the best full record. Each `score_matrix`
row gives the same status, steps and bit-identical score as
`scores_json` for the same parameters.

## 12. Command line

```text
hydro-craft describe
hydro-craft run FILE     (use - for standard input)
```

Exit status 0 means every record is complete, 2 means a well-formed
request has at least one record that is not complete, and 1 means the
request was rejected (the reason is printed to standard error as JSON).
Requests are limited to 16 MiB.

```sh
echo '{"schema":"hydro-craft.request/0.1","model":{"kind":"reservoir","capacity_m3":10,"initial_m3":5,"outlet_m3_per_day":4,"reserve_m3":2,"inputs":[{"inflow_m3":2,"evaporation_m3":1,"demand_m3":8}]},"candidates":[{"id":"ok","parameters":[1,0]},{"id":"bad","parameters":[2,0]}]}' | target/release/hydro-craft run -; echo "exit $?"
```

```text
{"model_sha256":"14f6eaaa4904340c1270bdb79a0813dde08cc385c5e677709ac190aaec80be99","records":[{"detail":null,"id":"ok","metrics":{"longest_shortage_days":1,"max_balance_m3":0.0,"shortage_days":1,"shortage_m3":4.0,"spill_m3":0.0,"trace":null},"score":4.0,"status":"complete","steps":1},{"detail":"reservoir controls must be in [0,1]","id":"bad","metrics":null,"score":null,"status":"invalid","steps":0}],"schema":"hydro-craft.result/0.1","version":"0.3.0"}
exit 2
```

```sh
echo '{"schema":"hydro-craft.request/0.1"}' | target/release/hydro-craft run -; echo "exit $?"
```

```text
{"detail":"missing field `model` at line 1 column 36","status":"rejected"}
exit 1
```

`models/request.json` is a complete traced catchment request:
`target/release/hydro-craft run models/request.json`.

## 13. Reproducibility and provenance

- Results are deterministic for fixed inputs and fixed work limits: the
  same model bytes, parameters and `max_steps` give the same statuses,
  steps and bit-identical scores for any worker count and any of the
  three result levels. 0.3.0 produces byte-identical JSON to 0.2.0 on
  every benchmark workload (see the
  [0.3.0 performance log](../hydro-craft-performance-log-0.3.0.md)).
- `wall_ms` and `cancel_file` depend on timing and can change which
  candidates finish.
- `model_sha256` hashes the exact model bytes. The Python wrapper
  encodes models with sorted keys, so two dicts with equal content hash
  equally; hand-written JSON with other spacing hashes differently.
- Every envelope carries `schema` (`hydro-craft.result/0.1`) and the
  package `version`. The request and result schemas are unchanged
  since 0.1.
- Campaign rows store the SHA-256 of the native extension, the model
  hash and every record, so a database documents which build produced
  each result.
- Keep the model JSON, the parameter vectors, the budget and the
  package version with any published result.

## 14. Performance tips

Measured on a 2-vCPU Intel Xeon @ 2.10 GHz with Rust 1.97 and CPython
3.12; see [architecture: performance](architecture.md#performance) for
the tables and evidence.

- **Use `score_matrix` inside optimizers.** From Python, 96 candidates
  of the 90-day fixture took 53 µs with `score_matrix`, 286 µs with
  `scores` and 728 µs with `batch`; one candidate took 4.5 µs, 10 µs
  with `scores` and 16 µs with `evaluate`. Most of the difference is
  JSON work in Python; natively the three calls took 0.045, 0.084 and
  0.163 ms for 96 candidates.
- **Evaluate populations, not single vectors.** Untraced candidates are
  simulated in interleaved groups (up to 8 catchment or 16 reservoir
  candidates), so a whole generation per call is several times cheaper
  per candidate than one call per candidate.
- **Leave `wall_ms` and `cancel_file` unset when you can.** They, and a
  `max_steps` below the number of days, switch evaluation to one
  candidate at a time with a checked step counter.
- **Workers help only for large batches.** On the 2-vCPU host two
  workers gave 1.3-1.7x for 48 candidates of 2,400 days, but made 96
  candidates of 90 days slower. Start with `workers=1` and measure; four
  workers on two CPUs gave no consistent gain over two.
- **Traces are expensive.** A traced run is simulated alone and encodes
  one JSON object per day; trace only the candidates you inspect.
- **Exponents of exactly 1 or 2** run the fastest catchment kernels.
  Other exponents call `powf` on every wet day.

## 15. Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| `ValueError: every scored fold needs observed data` | a fold has only null observations; move `warmup`, `train_end` or `validation_end`, or add observations |
| `ValueError: invalid daily forcing, units, fold or catchment area` | a day number is not consecutive, a value is out of range, or the fold indexes are not increasing and inside the forcing |
| `ValueError: unknown field ...` | a misspelled or extra key; models, days, candidates and budgets reject unknown keys (`fold` is computed, not an input) |
| `ValueError: Out of range float values are not JSON compliant` | NaN or infinity in a model or JSON call; `score_matrix` accepts NaN and reports the row as `invalid` |
| every record `invalid` with `expected ...` | wrong parameter count: 7 for the catchment, 2 for the reservoir |
| every record `work_limit` | `max_steps` is below the number of days |
| all records `cancelled` | the cancel file still exists; delete it before the next call |
| `trace requires at most 10,000 days` | traced runs are limited; trace a shorter model or use untraced metrics |
| `parallel` calls slower than serial | the batch is too small for helper threads; use `workers=1` |
| different `model_sha256` for "the same" model | the JSON bytes differ (key order, spacing, number formatting) |
| campaign re-evaluates everything after an upgrade | expected: the cache key includes the native build hash |

## 16. Examples index

| File | Scenario |
| --- | --- |
| [`examples/python/catchment_sensitivity.py`](../examples/python/catchment_sensitivity.py) | one-at-a-time and Morris sensitivity of the catchment |
| [`examples/python/warming_scenarios.py`](../examples/python/warming_scenarios.py) | snow and runoff timing under +1 to +4 °C |
| [`examples/python/reservoir_policy_study.py`](../examples/python/reservoir_policy_study.py) | minimax-regret release rule over dry, normal and wet years |
| [`examples/python/scipy_calibration.py`](../examples/python/scipy_calibration.py) | SciPy vectorized DE calibration with penalties and holdout |
| [`examples/python/fcmaes_cmaes_population.py`](../examples/python/fcmaes_cmaes_population.py) | fcmaes CMA-ES ask/tell with whole populations |
| [`examples/python/fcmaes_robust_policy.py`](../examples/python/fcmaes_robust_policy.py) | fcmaes BiteOpt and C++ CMA-ES on a robust reservoir objective |
| [`examples/matrix.rs`](../examples/matrix.rs) | Rust `score_matrix` screening and full replay |
| [`examples/sweep.rs`](../examples/sweep.rs) | Rust parameter sweep with full records |
| [`validation/optimize_performance.py`](../validation/optimize_performance.py) | timed SciPy search in full, scores and matrix modes |
| [`validation/run_campaign.py`](../validation/run_campaign.py) | campaign manifest runner |

The [tutorial](tutorial.md) is a shorter guided tour; the
[architecture guide](architecture.md) explains the engine.
