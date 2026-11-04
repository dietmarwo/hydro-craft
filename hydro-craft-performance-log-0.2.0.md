# hydro-craft performance implementation log

3 November 2026. Version 0.2.0, based on the prior 0.1.0 delivery
at `fec5c8f0277c`. The ZIP contains the updated source, runnable
examples, an ordered patch series, [tutorial](docs/tutorial.md),
[architecture](docs/architecture.md), raw evidence and build artifacts.
The [earlier implementation record](docs/implementation-0.1.0.md)
documents the original research scope and remaining external work.

## Ten incremental changes

Each row compares the indicated stage with its immediate predecessor.
Times are median milliseconds per prepared call. A ratio below 1.00×
means the new median was slower for that measurement. Small differences
vary with thread scheduling, code layout and host noise.

| Round | Commit | Change | Case | Before ms | After ms | Before / after |
| ---: | --- | --- | --- | ---: | ---: | ---: |
| 1 | `74bc168` | Evaluate singleton and one-worker batches on the calling thread, removing worker startup and join. | `one` | 0.050 | 0.009 | 5.40× |
| 2 | `c53d968` | Replace worker-local row collections and final sorting with one result slot per candidate index. Keep dynamic work assignment. | `parallel` | 0.838 | 1.128 | 0.74× |
| 3 | `c1d5333` | Validate unique candidate IDs through a pre-sized hash set. The large invalid batch isolates ID checks and response overhead from most domain computation. | `invalid` | 2.172 | 2.283 | 0.95× |
| 4 | `f1ce39d` | Serialize a typed prepared batch directly and keep the public CLI result adapter. | `trace` | 2.990 | 1.947 | 1.54× |
| 5 | `2444c10` | Borrow candidate IDs until the batch is serialized, eliminating output-side string clones. | `invalid` | 1.578 | 1.512 | 1.04× |
| 6 | `8f6d65f` | Inline the unmonitored work counter. Keep cancellation, deadline and exhausted-budget checks on the guarded path. | `domain` | 5.846 | 5.639 | 1.04× |
| 7 | `7b5f533` | Add Rust and Python score-only entry points, preserving status, step counts, score and record order while skipping diagnostic construction. This row compares the earlier full path with the new lean path. | `lean-serial` | 0.762 | 0.479 | 1.59× |
| 8 | `704b961` | Validate observation coverage and assign a fold tag to each prepared day. | `domain` | 5.388 | 5.413 | 1.00× |
| 9 | `d65159e` | Avoid the recharge power call on dry days and use direct arithmetic for exponents 1 and 2. | `domain` | 5.605 | 2.236 | 2.51× |
| 10 | `8bf5cce` | Skip diagnostic-only balance and holdout accumulation in lean catchment runs. Bind the reservoir target once and skip diagnostic-only streak and balance work in lean reservoir runs. | `lean-domain` | 2.157 | 2.012 | 1.07× |

Each stage passed locked Rust release tests and paired output checks
for the supplied workloads. The result comparisons
ignore the package-version string and require matching record order,
status, detail, steps, provenance and numeric values within tolerance.
Lean comparisons also normalize metrics to null. The benchmark commit
precedes the ten rounds. Documentation and version changes follow them.

## Cumulative native results

| Workload | 0.1.0 ms | Final ms | Before / after |
| --- | ---: | ---: | ---: |
| Single prepared evaluation | 0.043 | 0.007 | 6.35× |
| 96 full records, one worker | 1.198 | 0.615 | 1.95× |
| 96 full records, four workers | 0.824 | 0.390 | 2.11× |
| 12 traced records | 3.247 | 1.918 | 1.69× |
| 1,024 invalid candidates | 3.069 | 1.617 | 1.90× |
| 48 domain cases | 6.596 | 2.629 | 2.51× |
| 48 secondary domain cases | 2.423 | 2.279 | 1.06× |
| 96 lean scores | 1.226 | 0.187 | 6.56× |
| 48 lean domain scores | 6.376 | 2.051 | 3.11× |
| 48 lean secondary scores | 2.513 | 2.090 | 1.20× |

The lean API was introduced in 0.2.0. The baseline uses full
records for the three lean rows. The paired comparisons alternate process order,
exclude startup and model preparation, warm up each prepared evaluator,
and measure JSON decode, evaluation, checks and JSON encoding. They
use five observations per side on an AMD EPYC 9V74 with Rust 1.88.0,
using logical CPUs 0–3. These are fixed test workloads, with four
workers only in the parallel row. Ratios from individual stages do
not multiply into the cumulative ratio because the timings were rerun.

Supplemental workloads use 2,400 forcing days with the catchment model and 2,400 days of one-reservoir inputs. The catchment remains a synthetic HBV-style fixture. Exact threshold behavior uses normal floating-point arithmetic, and the synthetic references checked recurrence and mass balance.

## Installed Python optimization

Seven runs per configuration used the same seed, bounds, starting point,
optimizer budget and final traced replay. CPython 3.12 and SciPy 1.17.0
ran on one logical CPU. Times include preparation, objective evaluation
and traced winner replay, excluding Python imports and wheel compilation.
The total median is calculated per run, separately from the column medians.

| Configuration | Prepare ms | Search ms | Replay ms | Total ms | Baseline / total |
| --- | ---: | ---: | ---: | ---: | ---: |
| v0.1.0-full | 0.323 | 21.728 | 0.539 | 22.582 | 1.00× |
| v0.2.0-full | 0.304 | 14.382 | 0.370 | 15.102 | 1.50× |
| v0.2.0-scores | 0.312 | 13.742 | 0.353 | 14.481 | 1.56× |

The run made **168** objective calls and classified
**72** failures. The selected vector was
`[10.0, 1.0, 1.0, 0.5, 0.3, 2.0, 3.0]` with replay score **4.96506830649e-16**.
All configurations agreed on evaluation and failure counts, final
parameters and full replay score within the recorded tolerance.

## Release checks

- Rust formatting, Clippy with Python enabled and warnings denied,
  release tests, CLI request and native sweep passed.
- The independent domain validation passed against the release CLI.
- The wheel built from the extracted source distribution and installed
  into a fresh CPython 3.12 environment. Its ABI is CPython 3.10+ and
  its bundled target is Linux x86-64.
- The installed lean and full paths agreed on results, including mixed
  valid/invalid candidates, worker count, fixed work limits and
  cancellation. The tutorial's 4 Python blocks ran.
- SQLite cancellation/resume retained 21
  attempts and recovered 12 completed rows.
  Two full manifest runs against the same database produced identical output.
- The Rust crate archive passed Cargo package verification.

Run the packaged `validation/performance_pair.py` with saved native
benchmark binaries to reproduce comparisons. To reconstruct the ten
rounds in Git, apply `patches/*.patch` in filename order to the source
from the 0.1.0 ZIP. A baseline may first be committed locally. The patch
series includes the benchmark and release documentation commits around
the ten implementation commits.
