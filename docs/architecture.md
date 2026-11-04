# Hydro Craft architecture

Version 0.2.0. Calibrate a synthetic HBV-style daily catchment and compare non-anticipatory policies for one reservoir.

## Evaluation pipeline

```mermaid
flowchart TD
    P["Prepared model"] --> B["Candidate batch"]
    B --> V["Validate IDs and budgets"]
    V -->|Single worker or candidate| S["Inline evaluation"]
    V -->|Multiple workers| W["Scoped worker evaluation"]
    S --> D["Domain kernel and work counter"]
    W --> D
    D --> R["Ordered typed result"]
    R -->|Full| F["Metrics and optional trace"]
    R -->|Lean| L["Score and status"]
```

| Boundary | Location | Responsibility |
| --- | --- | --- |
| Request and CLI | `src/main.rs`, `src/lib.rs`, `models/request.json` | Versioned request schema, exact model-byte hash, typed validation, JSON output and exit code |
| Python boundary | `python/hydro_craft/__init__.py`, PyO3 in `src/lib.rs` | Encode once per batch, release the Python GIL during native evaluation, decode the typed result envelope |
| Prepared engine | `src/lib.rs` | Own immutable domain data, enforce batch rules, dispatch candidates and retain ordered result rows |
| Domain | `src/domain.rs` | Bind the candidate, execute model logic, classify status and compute a minimization objective |
| Campaign | `python/hydro_craft/campaign.py` | Local SQLite replay, attempt journal and complete-result caching |
| Verification | `validation/`, `examples/`, `evidence/` | Independent checks, executable examples and paired performance observations |

## Data and execution contracts

`Prepared::new` hashes the model bytes, deserializes the model and
validates its bounds before building domain caches. The Python
wrapper encodes models with sorted keys and finite JSON numbers.
Two inputs that differ in their serialized representation can
have different provenance hashes. Result and request schema tags
remain at 0.1. Package version is reported separately.

Every batch validates candidate count (1–4096), unique IDs, parameter
vector length (up to 64), requested workers (1–64) and trace count
(up to 16). Parallel workers claim indexes through an atomic cursor.
Each assigned index owns a result slot written once. After workers
join, output is serialized in input order. Single-worker or singleton
calls run inline. The typed prepared envelope borrows candidate IDs
until serialization completes. The CLI retains its full `Value` API.

`Work` counts domain-defined steps per candidate. Its fast branch
handles unmonitored steps below the limit. Monitored steps check
the cancel file, then batch deadline, then work budget. Domain
checks can still reject an invalid candidate before it takes a step.
Cancellation and deadlines are cooperative. The wall clock can
change which candidates finish, while fixed-work replays should agree
independent of worker count. Candidate parallelism creates scoped
threads per batch. Concurrent callers should account for total
worker demand when choosing batch settings.

Full records contain domain metrics and any requested trace.
`scores_json` in Rust and `Prepared.scores` in Python return the
same record keys with null metrics and reject traced candidates.
Both retain per-candidate limits, diagnostics needed for feasibility,
numeric score checks and failure status. Full replay restores
inspection details for the selected candidate.

## Domain internals

| Model | Input fixed at preparation | Candidate-local computation |
| --- | --- | --- |
| Catchment | Area, daily forcing, observations and day-to-fold tags | Snow accumulation/melt, soil recharge, evapotranspiration, fast and slow stores, runoff and train RMSE |
| Reservoir | Capacity, reserve, outlet and daily inputs | Prior-storage policy, physical withdrawals and releases, spill, shortage and objective |

The catchment recurrence updates each store in daily order, with
separate precipitation and observed flow masks. Preparation validates
the contiguous day grid and observations in every scored fold, then
writes compact fold tags into days. Dry recharge avoids a power
call. Exponents exactly 1 and 2 use direct arithmetic. Other values
use the general power function. The reservoir binds target storage
once and applies the same release ordering for every candidate.

Lean catchment scores skip validation/test accumulation and balance
reporting. Lean reservoir scores skip shortage streak and diagnostic
balance. Both paths simulate all days and enforce the same input,
work and cancellation rules. Daily water balance is a full-output
diagnostic and is independently validated. Both modes carry warm-up stores into the scored days. See the [model card](model.md) for
units and the limited synthetic calibration scope.

## Persistence and boundaries of evidence

The local campaign hashes the installed extension bytes plus each
scenario, model, candidate and work settings to locate reusable
records. SQLite stores completed, invalid and infeasible results,
and keeps an append-only journal of attempts. Cancelled or time-
limited attempts remain in the journal and can be retried. SQLite
commits each chunk atomically. A new extension build changes the
cache namespace. The current workflow is local and exposes no
distributed worker protocol.

The package contains source, a source distribution, and a validated
Linux x86-64 wheel. The included performance log distinguishes the
native prepared boundary, complete Python search and clean build.
See the [tutorial](tutorial.md) for exact commands and
[the model card](model.md) for equations and research scope.
