# hydro-craft examples

Runnable examples for the installed `hydro_craft` package (0.3.0). All
inputs are seeded synthetic data from [`synthetic.py`](synthetic.py): a
snowy catchment with four years of daily forcing and noisy synthetic
observations, and an irrigation reservoir with dry, normal and wet
inflow scenarios. Run any example from the repository root, for example
`python examples/python/warming_scenarios.py`, or run all of them and
refresh the saved outputs with `python examples/python/run_examples.py`.

The optimization examples need SciPy 1.17 and fcmaes 2.1.0
(`pip install scipy==1.17.0 fcmaes`); the application examples need
only the package. Each example finishes in about a second on a 2-vCPU
host. The saved outputs in [`outputs/`](outputs/) come from actual runs
with CPython 3.12; timing lines vary from run to run.

## Application examples

| Example | Question | APIs | Output |
| --- | --- | --- | --- |
| [`catchment_sensitivity.py`](catchment_sensitivity.py) | Which catchment parameters control the fit and the water balance? One-at-a-time perturbations and Morris elementary effects. | `batch`, `score_matrix` | [txt](outputs/catchment_sensitivity.txt) |
| [`warming_scenarios.py`](warming_scenarios.py) | How do +1, +2 and +4 degC shift peak snow, melt timing, runoff and peak discharge? | `batch` with `trace` | [txt](outputs/warming_scenarios.txt) |
| [`reservoir_policy_study.py`](reservoir_policy_study.py) | Which release rule has the smallest worst-case regret across dry, normal and wet years? | `score_matrix(workers=2)`, `evaluate` | [txt](outputs/reservoir_policy_study.txt) |

## Optimization examples

| Example | Optimizer | Failure handling | Output |
| --- | --- | --- | --- |
| [`scipy_calibration.py`](scipy_calibration.py) | SciPy `differential_evolution(vectorized=True)`; one `score_matrix` call per generation | Search box allows `lp` up to 1.2; invalid rows get `1e3 * (1 + distance outside limits)` | [txt](outputs/scipy_calibration.txt) |
| [`fcmaes_cmaes_population.py`](fcmaes_cmaes_population.py) | fcmaes `cmaes.Cmaes` ask/tell; each population is one `score_matrix` call with two native workers | Same graded penalty before `tell` | [txt](outputs/fcmaes_cmaes_population.txt) |
| [`fcmaes_robust_policy.py`](fcmaes_robust_policy.py) | fcmaes `bitecpp.minimize` (BiteOpt) and `cmaescpp.minimize` on the mean cost over three scenarios | Controls searched in [0, 1.25]; invalid rows get `1e8 * (1 + distance outside [0, 1])` | [txt](outputs/fcmaes_robust_policy.txt) |

## Rust

[`../matrix.rs`](../matrix.rs) screens a 41 x 41 grid of reservoir rules
plus one invalid row through `Prepared::score_matrix` and replays the
winner as a full JSON record: `cargo run --release --example matrix`.
Its output is in [`../outputs/matrix.txt`](../outputs/matrix.txt).

See the [user manual](../../docs/user-manual.md) for the full API.
