# hydro-craft 0.3.0

A bounded research evaluator for a synthetic HBV-style daily catchment and
a one-reservoir release rule. See the [user manual](docs/user-manual.md)
for installation, the complete model reference and every API, the
[model card](docs/model.md) for units, equations and scope, the
[tutorial](docs/tutorial.md) for a guided tour and the
[architecture](docs/architecture.md) for the engine and measured
performance. The [0.3.0 performance log](hydro-craft-performance-log-0.3.0.md)
records the ten performance rounds; the
[0.2.0 log](hydro-craft-performance-log-0.2.0.md) and the
[0.1.0 implementation record](docs/implementation-0.1.0.md) preserve the
earlier releases.

## Rust CLI and native examples

```sh
cargo run --release --locked -- describe
cargo run --release --locked -- run models/request.json
cargo run --release --locked --example sweep -- models/example.json
cargo run --release --locked --example matrix
```

The CLI uses versioned JSON with exact model-byte SHA-256, candidate IDs and
explicit work and worker limits. `run -` accepts stdin. Exit 1 indicates a
rejected request. Exit 2 retains classified candidate records when at least
one candidate is incomplete or infeasible. Output traces are opt-in.

## Python

Install the matching wheel from `packages/` in a clean environment.
The tested wheel target is Linux x86-64 with a CPython 3.10+ ABI.
Actual clean-environment verification used CPython 3.12.

```python
from hydro_craft import Prepared, load_example

case = load_example()
prepared = Prepared(case["model"])
row = prepared.evaluate(case["parameters"], trace=True)
assert row["status"] == "complete"
print(row["score"])
scores, statuses, steps = prepared.score_matrix([case["parameters"]] * 4)
print(scores, statuses, steps)
```

`Prepared.batch` returns full records and `Prepared.scores` records with
null metrics, each with one native crossing per batch. `Prepared.score_matrix`
takes a list of parameter rows or a 2-D NumPy array and returns scores,
statuses and steps with no JSON in either direction; use it inside
optimizers. Untraced candidates are simulated in interleaved groups and
records always return in input order. The local Python campaign commits
results to SQLite, preserves interrupted attempts and reuses completed
records by candidate, scenario, settings and build identity. Run
`validation/run_campaign.py MODULE models/campaign.json campaign.sqlite`
with the installed package to evaluate or resume the included manifest.

[`examples/python/`](examples/python/README.md) has application examples
(sensitivity, warming scenarios, a reservoir policy study) and
optimization examples with SciPy and fcmaes, each with saved output.

## Verify and rebuild

```sh
cargo test --release --locked
cargo build --release --locked
python validation/validate.py ./target/release/hydro-craft
maturin sdist --out dist
maturin build --release --locked --out dist
```

Independent numerical scripts use Python 3.12. The optional
`validation/optimize.py hydro_craft` demonstrates an in-process SciPy DE
objective, and `validation/optimize_performance.py` times it in full,
`--scores` and `--matrix` modes. The native sweep needs only the Rust
crate. See `evidence/` for actual runs and `patches/` for the patch
series of each release (`patches/0.2.0/`, `patches/0.3.0/`).
