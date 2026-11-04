# hydro-craft performance implementation log

4 November 2026. Version 0.3.0, based on the 0.2.0 delivery imported at
`f4cc0a3`. The ZIP contains the updated source, runnable examples, an
ordered patch series, a new [user manual](docs/user-manual.md), the
reviewed [architecture](docs/architecture.md) with measured performance,
the [tutorial](docs/tutorial.md), raw evidence and build artifacts. The
[0.2.0 log](hydro-craft-performance-log-0.2.0.md) is unchanged; its
patches now live in `patches/0.2.0/`.

All measurements below come from one host: 2 vCPUs, Intel(R) Xeon(R)
Processor @ 2.10GHz (KVM), Rust 1.97.0, CPython 3.12.3. Every build and
timing ran inside a shared CPU lock so that no other build or timing
overlapped it. The 0.2.0 log's numbers came from another host (AMD EPYC
9V74, 4 CPUs, Rust 1.88) and are not combined with these in any ratio.

## Ten incremental changes

Each row compares a round's benchmark binary with the previous kept
round's binary using `validation/performance_pair.py --samples 11`, with
the target workload and expected effect written to
[`plan.md`](evidence/performance-0.3.0/plan.md) before the timing. The
fixed rule kept a round at a median speedup of at least 1.03x with the
paired output check passing. Times are median milliseconds per prepared
call. "Lean" rows use `scores_json` on both sides; round 1 compares 0.2.0
lean `scores_json` with the new matrix API on the same candidates.

Host: 2 vCPUs, Intel(R) Xeon(R) Processor @ 2.10GHz (KVM), Rust 1.97.0, CPython 3.12.3.

| Round | Commit | Change | Case | Before ms | After ms | Before / after |
| ---: | --- | --- | --- | ---: | ---: | ---: |
| 1 | `1d536de` | Add `Prepared::score_matrix` and its Python binding: row-major untraced parameters in, typed status, steps and score out, with no JSON in either direction. | `serial`, lean before, matrix after | 0.225 | 0.186 | 1.21x |
| 2 | `b8e5de1` | Replace per-batch scoped threads with a persistent worker pool shared by all batches; the caller works alongside up to `workers - 1` helpers. | `parallel` | 0.648 | 0.493 | 1.31x |
| 3 | `e9a908b` | Serialize typed full-record metrics structs, declared in sorted key order, instead of nested `serde_json::Value` maps. | `serial` | 0.595 | 0.359 | 1.66x |
| 4 | `4c80d9a` | Record traces as typed day rows in a preallocated vector instead of one `json!` map per day. | `trace` | 1.183 | 0.414 | 2.85x |
| 5 | `79ab1b6` | Interleave up to eight untraced reservoir candidates over the shared inputs through one shared `reservoir_day` step when the budget cannot stop a candidate. | `domain2` | 2.479 | 0.802 | 3.09x |
| 6 | `f3a7277` | Interleave up to four untraced catchment candidates through shared catchment step functions. | `domain` | 2.726 | 1.168 | 2.33x |
| 7 | `d9147d0` | Borrow candidate IDs from the request text and read parameters into exactly sized vectors through a 64-value stack buffer. | `invalid` | 1.465 | 1.262 | 1.16x |
| 8 | `5313dbe` | Take the frozen or thawed branch once per day for the whole group; give exponents of exactly 1 or 2 branch-free eight-lane kernels. | lean `domain` | 0.805 | 0.511 | 1.58x |
| 9 | `6f94fd9` | Run sixteen lanes for lean reservoir scoring. | lean `domain2` | 0.511 | 0.405 | 1.26x |
| 10 | `c8e3aeb` | Run eight lanes for full catchment records of a known exponent class. | `domain` | 1.178 | 0.776 | 1.52x |

Each round passed `cargo fmt --check`, Clippy with the `python` feature
and warnings denied, the release tests and the paired output check for
every workload in full, lean and matrix modes. The comparisons ignore the
package-version string and require matching record order, status,
detail, steps, provenance and numeric values within the existing
tolerances; lean comparisons normalize metrics to null and matrix
comparisons normalize metrics and detail to null. No round changes the
floating-point operations a candidate performs: the lane kernels and the
scalar path share one step function per model, and the logical step count
is unchanged. The probe outputs of the 0.2.0 and round 10 binaries are
byte-identical for every workload in full and lean mode, and the
independent domain validation (`validation/validate.py` against the
release CLI) reproduced the 0.2.0 `evidence/validation.json` byte for
byte. New tests cover the matrix API's equivalence with `scores_json`
(invalid rows, `max_steps`, cancel file, deadline, several workers,
dimension 0 and every rejection), the pool (every index once, concurrent
batches, panics), sorted typed metrics, lane-versus-scalar equality for
both models across widths, exponent classes, snow and dry spells, and
candidate decoding against the plain derive including error messages.
One non-round commit between rounds 6 and 7 adds the feature-gated
`profiling` phase timer and `examples/breakdown.rs`; it does not change
default builds.

## Rejected attempts

Host: 2 vCPUs, Intel(R) Xeon(R) Processor @ 2.10GHz (KVM), Rust 1.97.0, CPython 3.12.3.

| Attempt | Change | Case | Before ms | After ms | Before / after |
| --- | --- | --- | ---: | ---: | ---: |
| Round 8, first attempt | Size worker dispatch by available CPUs and by one evaluator per 8,192 simulated candidate-days | `parallel` | 0.186 | 0.188 | 0.99x |

The attempt was reverted; its timing is in
[`rejected/`](evidence/performance-0.3.0/rejected/). Exploratory builds
that were measured informally and never declared as rounds (reciprocal
multiplication in the catchment, about 3% on full records and a
last-bits change; carrying the store total across days; eight lanes for
the old catchment kernel; singletons through the lane kernel) were
discarded without a commit.

## Cumulative native results

0.2.0 baseline benchmark binary (built from `f4cc0a3`) against the round
10 binary, `--samples 11` each
([`cumulative/`](evidence/performance-0.3.0/cumulative/)). The
`parallel` workload keeps its four requested workers on this 2-vCPU host.
The two matrix rows compare 0.2.0 lean scores with 0.3.0 matrix calls,
because 0.2.0 has no matrix API.

Host: 2 vCPUs, Intel(R) Xeon(R) Processor @ 2.10GHz (KVM), Rust 1.97.0, CPython 3.12.3.

| Workload | 0.2.0 ms | 0.3.0 ms | Before / after |
| --- | ---: | ---: | ---: |
| Single prepared evaluation (`one`) | 0.005 | 0.003 | 1.55x |
| 96 full records, one worker (`serial`) | 0.482 | 0.163 | 2.96x |
| 96 full records, four workers (`parallel`) | 0.611 | 0.179 | 3.41x |
| 12 traced records (`trace`) | 1.714 | 0.330 | 5.19x |
| 1,024 invalid candidates (`invalid`) | 1.410 | 1.259 | 1.12x |
| 48 domain cases (`domain`) | 2.819 | 0.781 | 3.61x |
| 48 secondary domain cases (`domain2`) | 2.490 | 0.804 | 3.10x |
| 96 lean scores | 0.224 | 0.084 | 2.66x |
| 48 lean domain scores | 2.488 | 0.511 | 4.87x |
| 48 lean secondary scores | 2.372 | 0.396 | 5.99x |
| 96 matrix scores (0.2.0: lean) | 0.234 | 0.045 | 5.18x |
| 48 domain matrix scores (0.2.0: lean) | 2.475 | 0.493 | 5.03x |

The paired comparisons alternate process order, exclude start-up and
model preparation, warm up each prepared evaluator and measure JSON
decode, evaluation, checks and JSON encode (matrix: typed rows only).
Ratios from individual rounds do not multiply into the cumulative ratio
because the timings were rerun. The phase breakdown, per-call costs from
Python and worker scaling are in the architecture's
[Performance](docs/architecture.md#performance) section.

## Installed Python optimization

Seven runs per configuration of `validation/optimize_performance.py`,
interleaved, on one logical CPU (`taskset -c 0`), each configuration in a
fresh virtual environment: the 0.2.0 wheel from the 0.2.0 `packages/`,
and the 0.3.0 wheel built with maturin from the 0.3.0 sdist. Times
include preparation, the seeded SciPy search and the traced replay of the
winner, excluding Python start-up and imports. The total median is
calculated per run, separately from the column medians
([`optimization/runs.json`](evidence/performance-0.3.0/optimization/runs.json)).

Host: 2 vCPUs, Intel(R) Xeon(R) Processor @ 2.10GHz (KVM), Rust 1.97.0, CPython 3.12.3.

| Configuration | Prepare ms | Search ms | Replay ms | Total ms | v0.2.0-full / total |
| --- | ---: | ---: | ---: | ---: | ---: |
| v0.2.0-full | 0.386 | 11.683 | 0.527 | 12.578 | 1.00x |
| v0.2.0-scores | 0.366 | 10.443 | 0.722 | 11.825 | 1.06x |
| v0.3.0-full | 0.382 | 11.137 | 0.363 | 11.873 | 1.06x |
| v0.3.0-scores | 0.415 | 12.920 | 0.403 | 13.658 | 0.92x |
| v0.3.0-matrix | 0.375 | 8.451 | 0.412 | 9.268 | 1.36x |

The run made **168** objective calls and classified **72** failures in
every configuration. The selected vector was
`[10.0, 1.0, 1.0, 0.5, 0.3, 2.0, 3.0]` with replay score
**4.965068306494546e-16**; all five configurations agreed on evaluation
and failure counts, final parameters and replay score exactly. This
search evaluates one candidate per call, so SciPy and the Python wrapper
dominate (about 50-70 us per call against 2-3 us of native work), and the
run-to-run spread (search 7.9-18.0 ms for v0.2.0-scores, 10.0-15.3 ms for
v0.3.0-full) is larger than the difference between the full and scores
modes. The matrix mode is the only one clearly faster (search 7.5-9.2 ms
over its seven runs). With whole populations per call, the matrix API is
5x cheaper than `scores` and 14x cheaper than `batch` from Python for 96
candidates ([`python-call-costs-v030.json`](evidence/performance-0.3.0/python-call-costs-v030.json)).

The timed wheel was built from the sdist of the version-bump commit; the
release wheel's files, including `_native.abi3.so`, are byte-identical to
it ([`installed-checks.txt`](evidence/release-0.3.0/installed-checks.txt)).

## Release checks

All checks ran inside the CPU lock on the host above
([`evidence/release-0.3.0/`](evidence/release-0.3.0/)).

- `cargo fmt --check`, Clippy with `--features python` and with
  `--features python,profiling` (warnings denied, `PYO3_PYTHON` set to
  CPython 3.12) and `cargo test --release --locked` (11 tests) passed.
- The CLI request (`models/request.json`, exit 0), `describe` and the
  native sweep ran; the sweep output and the CLI records are identical to
  0.2.0's apart from the version string.
- The independent domain validation passed against the release CLI and
  reproduced 0.2.0's `evidence/validation.json` byte for byte.
- The sdist was built, extracted, and the wheel built from it with
  maturin 1.9.6 and installed into a fresh CPython 3.12 venv. Its ABI is
  CPython 3.10+ and its target Linux x86-64.
- `validation/installed.py` and `validation/release_checks.py` passed
  against the installed wheel: lean, full and matrix parity with mixed
  valid and invalid candidates, worker count, fixed work limits and
  cancellation; NumPy input and rejected matrix shapes; the tutorial's
  5 and the user manual's 17 Python blocks.
- SQLite cancellation and resume retained 21 attempts and recovered 12
  completed rows. Two full manifest runs against the same database
  produced identical output.
- All six Python examples ran against the installed wheel; outputs match
  the saved ones apart from timing values. The manual's shell install
  commands ran in a scratch directory.
- The Rust crate archive passed `cargo package --locked` verification.

## Reproduction

Build each benchmark binary from its commit with
`cargo build --release --locked --examples` and keep
`target/release/examples/performance`, then compare two binaries with

```sh
python validation/performance_pair.py BEFORE AFTER models/example.json WORKLOAD --samples 11
```

adding `--before-scores`/`--after-scores` for lean calls and
`--before-matrix`/`--after-matrix` for the matrix API. `CRAFT_WORKERS=N`
overrides a workload's worker count; `validation/worker_scaling.py` and
`cargo run --release --features profiling --example breakdown -- models/example.json`
reproduce the scaling and phase tables. Run
`validation/optimize_performance.py hydro_craft` with no flag, `--scores`
or `--matrix` under `taskset -c 0` for the Python table and
`validation/python_call_costs.py hydro_craft` for per-call costs.

To reconstruct the history in Git, apply `patches/0.3.0/*.patch` in
filename order with `git am` to the 0.2.0 source (the import commit). The
series has 18 patches: the ten rounds, the profiling-tool commit between
rounds 6 and 7, the measurement commit, the version bump, the examples,
the documentation, the native release evidence, a documentation commit
that labels each performance table with its host, and the release
commit. The series leaves out the
binary files in `packages/`, which ship in that directory, and the
final commit that adds the series itself and `SHA256SUMS`. The 0.2.0
series is in `patches/0.2.0/` and applies to the 0.1.0 source as its log
describes.
