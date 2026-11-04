# hydro-craft implementation log

3 November 2026. Version 0.1.0. This delivery uses the bounded research
scope recorded below. The independent references and optional comparisons
are identified separately for each increment.

## Increment results

| Increment | Status | Implementation and evidence |
| --- | --- | --- |
| 0 | Original reference blocked | The pinned AWR repository and forcing could not be retrieved. The model card and independent synthetic oracle specify the fallback. There is no reproduced HBV or current AWR runtime comparison. |
| 1 | Bounded fallback | Native CLI and Python prepared evaluator own immutable forcing. One local worker owns each candidate's stores. Invalid daily grids and missing score folds are rejected. |
| 2 | Implemented on synthetic inputs | Dry, isolated-storm recession, snowfall/melt and day-by-day water-balance oracles. An independent scalar recurrence checks all runoff on the seeded fixture. |
| 3 | Bounded fallback | Implemented the declared synthetic HBV-style Rust formulation. Its store semantics are tested. A port or wrapper matching the unavailable AWR implementation remains open. |
| 4 | Implemented on synthetic inputs | Native batches, candidate-order replay, work and cancellation limits, and cold versus prepared timing. The timings cover this evaluator only. |
| 5 | Implemented on synthetic inputs | Chronological warm-up, train, validation and final test splits with masked observations. Frozen one-parameter grid recovers a known synthetic prediction. A seven-parameter vector remains potentially nonidentifiable. |
| 6 | Implemented | Separate one-reservoir mass-balance evaluator. Withdrawals, discretionary releases, spill, evaporation and shortages are tested on a hand-solvable case. |
| 7 | Implemented on synthetic inputs | Nine rule-curve/gain candidates across seeded inflows. Held-out shortages and shortage duration are reported. A market or LP allocation formulation remains optional and was deferred. |
| 8 | Local fallback | Resumable SQLite campaign and independent replay included. External integration and public catchment review require matching current contracts and permissioned or public reference data. |
| 9 | Implemented for tested platform | Source, Rust crate, sdist and wheel tested for the synthetic catchment and reservoir. |

## Numerical results

The maximum difference against the independent synthetic runoff recurrence was **1.78e-15 mm/day**. Maximum daily mass-balance error was **1.42e-14 mm**. Test RMSE was 7.93e-16 mm/day. This fixture identifies the searched recession fraction on its frozen grid.

The optional in-process SciPy differential-evolution demonstration made
168 evaluations and replayed its selected candidate.
It used the packaged example and its own local bounds. The domain-specific
grid and holdout evidence are in `evidence/validation.json`.

## Installed performance and replay

The following are medians of three 96-candidate batch observations,
measured in a clean CPython 3.12 environment with the installed wheel.
The scenarios and candidate inputs are identical within a row pair.
Times include native crossing and compact JSON result conversion.

| Boundary | Time |
| --- | ---: |
| Cold preparation and batch, one worker | 2.062 ms |
| Prepared batch, one worker | 1.731 ms |
| Prepared batch, four workers | 1.285 ms |

The replay test reversed candidate order and changed worker count. It
recovered 12 completed SQLite records across cancellation and retained
21 attempts. This campaign uses a local database and package-build
identity. Cross-machine determinism and distributed worker behavior
were outside the test scope.

## Release evidence

- `cargo fmt`, Clippy with warnings denied and locked Rust release tests passed.
- The native sweep and numerical validation ran against a release build.
- A wheel built from the extracted sdist and installed offline into a fresh
  environment. The wheel target is Linux x86-64, CPython 3.10+ ABI. Actual
  verification used CPython 3.12 on x86_64.
- The standalone Rust crate passed Cargo package verification.
- Model and source manifests are in `docs/`. Raw timing, numerical and
  clean-install results are in `evidence/`.

The source was staged in successive local commits. Each increment above
records its own outcome, while related implementation changes share
commits.

- `6399181` Define versioned evaluation and packaging boundaries
- `2a09f87` Implement bounded domain evaluator and resumable local execution
- `220f821` Validate model laws and add bounded search and installation examples
- `0b124ee` Prepare source distributions and document validated research scope
- `fec5c8f` Canonicalize campaign replay output
