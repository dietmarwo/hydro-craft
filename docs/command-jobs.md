# A bounded evaluator as a trusted command job

Calibrate a synthetic catchment, retain the inputs and outputs, and replay the
winner. Then use the same driver for a Rankine-cycle evaluator. The
[shared index](https://github.com/dietmarwo/optimization-jobs/blob/main/tutorials/domain-evaluators/README.md)
and [campaign example](https://github.com/dietmarwo/experiment-craft/blob/main/examples/campaigns/domain-evaluators/README.md)
are the companion entry points.

Four limits apply throughout:

- Commands are trusted programs under the runner's user. They are not a sandbox.
- A command result records exit status and output hashes. It carries no
  objective, feasibility, claim or evaluation accounting. Scientific statements
  below come from the tutorial's own checked records, not from the runner.
- The MCP server refuses command jobs. An agent needs a shell for this route.
- The catchment and reservoir are synthetic. Completion establishes numbers
  inside these models only, not behavior of a real catchment.

## 1. Setup

Use Linux x86-64, CPython 3.12 and Rust 1.88 or newer. Start in a new parent
workspace. Every later shell block starts in this same workspace with its
virtual environment active. Keep the sibling checkout names below.

```sh
for repo in hydro-craft thermo-craft optimization-jobs experiment-craft; do
  git clone https://github.com/dietmarwo/$repo
done
python3 -m venv .venv
. .venv/bin/activate
python -m pip install -r optimization-jobs/tutorials/domain-evaluators/requirements.txt
python - <<'PY'
from pathlib import Path
import hashlib
for repo in ['hydro-craft', 'thermo-craft']:
    root = Path(repo)
    sums = dict((name.lstrip('*'), sha) for sha, name in
                (line.split(maxsplit=1) for line in (root/'SHA256SUMS').read_text().splitlines()))
    for wheel in (root/'packages').glob('*.whl'):
        if hashlib.sha256(wheel.read_bytes()).hexdigest() != sums[str(wheel.relative_to(root))]:
            raise ValueError('wheel checksum mismatch')
PY
python -m pip install hydro-craft/packages/*.whl thermo-craft/packages/*.whl
python -m pip install -e experiment-craft
cargo build --release --locked --manifest-path optimization-jobs/Cargo.toml
cargo build --release --locked --manifest-path hydro-craft/Cargo.toml --bin hydro-craft
cargo build --release --locked --manifest-path thermo-craft/Cargo.toml --bin thermo-craft
export RUNNER="$PWD/optimization-jobs/target/release/optimization-run"
export NODE="$PWD/job-demo-node"
"$RUNNER" --root "$NODE" node init --capacity 2 --allow-commands
```

The wheels are checked-in artifacts, not PyPI packages. Other platforms need a
source build as described in the [user manual](user-manual.md), followed by a
new descriptor wheel path/hash and fresh numerical baseline. The examples
reserve `cpu_slots` equal to native `workers`; the optimizer itself is serial.
Check `node doctor` before requesting more slots than the host provides.
The scripts expect each CLI in its repository's `target/release`; pass
`--binary` for a build in another target directory.

## 2. One evaluation as a job

From the workspace of section 1:

```sh
python hydro-craft/examples/jobs/job_spec.py --sources . --output evaluation-spec \
  --kind evaluation --workers 2 --request-key evaluation-1
"$RUNNER" --root "$NODE" validate --spec evaluation-spec/job.json
"$RUNNER" --root "$NODE" run --spec evaluation-spec/job.json > evaluation-handle.json
JOB_ID=$(python -c 'import json; print(json.load(open("evaluation-handle.json"))["id"])')
JOB_DIR=$(python -c 'import json; print(json.load(open("evaluation-handle.json"))["directory"])')
"$RUNNER" --root "$NODE" result "$JOB_DIR"
"$RUNNER" --root "$NODE" jobs collect "$JOB_ID" result --destination evaluation-result.json
"$RUNNER" --root "$NODE" jobs collect "$JOB_ID" status --destination evaluation-status.json
```

`job_spec.py` stages the CLI as an executable input, the request and wrapper as
hashed inputs, and declares `result.json` and `status.json` as outputs. The CLI
has no output flag. The [wrapper](../examples/jobs/wrap_cli.py) captures stdout;
there is no shell redirection inside a job. CLI exit 2 means a well-formed
request with an incomplete or infeasible row. The wrapper records it and exits
0, allowing a dependent checker to inspect the rows. Rejected requests (exit 1)
and malformed output still fail the wrapper. A completed wrapper therefore
does **not** mean every evaluation completed.

The runner also seals valid declared outputs after a raw exit 2, but marks that
command failed and blocks dependent campaign steps. Tests exercise both paths.
Its cleared environment has no HOME, PATH is `/usr/bin:/bin`, and native thread
variables default to 1. An absolute venv interpreter still finds its packages.

## 3. One optimization as a job

From the same workspace, stage a different command and key:

```sh
python hydro-craft/examples/jobs/job_spec.py --sources . --output optimize-spec \
  --kind optimization --seed 1 --evals 400 --optimizer cmaes --workers 2 \
  --request-key calibration-1
"$RUNNER" --root "$NODE" validate --spec optimize-spec/job.json
"$RUNNER" --root "$NODE" run --spec optimize-spec/job.json > optimize-handle.json
JOB_ID=$(python -c 'import json; print(json.load(open("optimize-handle.json"))["id"])')
for output in best history identity; do
  case "$output" in history) suffix=csv;; *) suffix=json;; esac
  "$RUNNER" --root "$NODE" jobs collect "$JOB_ID" "$output" \
    --destination "collected-$output.$suffix"
done
```

`optimize.py --seed S --evals E` runs fcmaes CMA-ES or DE ask/tell. Each
population is one `Prepared.score_matrix` call. The score is training RMSE;
validation and test observations never enter the optimizer's objective.
The model in the saved [case](../examples/jobs/case.json) is the output of
`observe(catchment(forcing()))` with the default arguments of the repository's
[synthetic example](../examples/python/synthetic.py): four years of seeded
forcing, noisy observations with gaps, and fixed folds. The shared test suite
regenerates it and compares. The search box is the example's `BOUNDS`, with
the `lp` bound extended to 1.2 so that invalid rows occur. Invalid rows receive
`1000 * (1 + distance outside the model limits)`; they cannot become the best
complete candidate. History keeps every status, step count and penalized score.

The saved case declares a per-candidate work limit (1,460 days for hydro,
1,024 logical steps for thermo), passed explicitly to search and CLI replay.
Classified `work_limit`, `wall_limit` and `cancelled` rows retain their status.
The evaluation budget is exact even for a partial final population; no partial
population is passed to `tell`. An optimizer's own stop can end earlier and is
recorded explicitly. The 120-second wall limit is a safety net, not the search
budget. A process killed at that limit is timed out, not a successful short run.
A search with no complete candidate is data; the scientific checker rejects it.

`best.json` retains actual evaluations, status counts, work, parameters, score,
seed, optimizer and model hash. `identity.json` additionally records the wheel
SHA-256 checked against `SHA256SUMS`, package files verified against that wheel,
package versions, case/script hashes, Python version and worker count. The
runner hashes the interpreter and staged binary. Installed dependencies and
host libraries are not snapshotted; keep the environment fixed during a run.

## 4. Replay and identity

Repeating the same normalized request and key returns the original attempt:

```sh
"$RUNNER" --root "$NODE" run --spec optimize-spec/job.json > repeated-handle.json
python - <<'PY'
import json
first = json.load(open('optimize-handle.json'))
again = json.load(open('repeated-handle.json'))
if first['id'] != again['id']:
    raise ValueError('idempotency failed')
print('same attempt:', first['id'])
PY
```

Use a **new key** for a new execution. Idempotency does not establish numerical
determinism. The complete driver runs a fresh 2-seed × 2-optimizer smoke and a
separate CLI replay for each winner; compare its fresh executions:

```sh
python optimization-jobs/tutorials/domain-evaluators/run.py --sources . \
  --evaluator hydro-craft --output hydro-direct --workers 1
python optimization-jobs/tutorials/domain-evaluators/run.py --sources . \
  --evaluator hydro-craft --output hydro-jobs --workers 2 --command-jobs
python optimization-jobs/tutorials/domain-evaluators/compare.py \
  hydro-direct/summary.json hydro-jobs/summary.json
```

All summary fields except elapsed seconds must match exactly; histories also
match in the worker-count regression. Both random streams of CMA-ES are seeded.
There is no asynchronous optimizer scheduling.

The same seed can still give another result on another machine. fcmaes CMA-ES
calls BLAS, and OpenBLAS selects its kernels by CPU model; a difference in the
last digit of one matrix product changes the rest of a search. The job
environment therefore sets `OPENBLAS_CORETYPE=Nehalem`, a kernel that every
x86-64 CPU supported by numpy can run, and `identity.json` records the kernels
that were loaded. The thermo adapter avoids numpy's vector power for the same
reason. With this, the recorded smoke summaries are reproduced exactly on
different x86-64 CPUs under the pinned requirements; continuous integration
compares them. Another Python package set, C library or architecture can
still change the last digits, so a mismatch names the fields that differ.

## 5. A campaign

From the workspace of section 1:

```sh
python experiment-craft/examples/campaigns/domain-evaluators/run.py --sources . \
  --evaluator hydro-craft --output hydro-campaign --preset smoke --workers 2
# Five-seed reproducibility exercise at the same small per-arm budget:
python experiment-craft/examples/campaigns/domain-evaluators/run.py --sources . \
  --evaluator hydro-craft --output hydro-five-seeds --preset smoke --workers 2 \
  --seeds 1,2,3,4,5
```

The generator expands seeds × optimizers into explicit named arms. This is
intentional: experiment-craft cannot bind an output from a multi-variant matrix.
The check step has `needs` for every arm and binds their sealed `best` outputs.
It replays each point through the staged CLI as a full record and checks three
things: the winner names this case's model hash, the CLI reports that same
hash for the model it evaluated, and the CLI score equals the search score.
It tabulates train, validation and test RMSE in `checks.json`.
A second observation of the completed campaign must submit no new native jobs.
To select using validation rather than training, inspect the numeric file:

```sh
python - <<'PYTHON'
import json
rows = json.load(open('hydro-five-seeds/checks.json'))['rows']
valid = [r for r in rows if r['passed']]
if len(valid) != len(rows):
    raise ValueError('failed arms remain in the report; do not silently discard them')
for row in sorted(valid, key=lambda r: r['metrics']['validation_rmse_mm']):
    print(row['id'], row['metrics'])
PYTHON
```

The execution report lists job states; fold metrics live in `checks.json`.

`report/report.json`, `report/results.csv` and `report/report.md` retain failed
or blocked execution rows. A failed producer blocks the dependent check. A
failed scientific check retains its own sealed `checks.json`; it is never
silently dropped from a successful report. Choose an arm using validation,
then treat test as the final descriptive assessment. Do not continue tuning
against test values. `study` uses five seeds and 20,000 evaluations per arm;
smoke is an execution check, not evidence of optimizer superiority.

## 6. Port it to another evaluator

The unchanged driver reads one descriptor and one domain adapter:

```sh
python optimization-jobs/tutorials/domain-evaluators/run.py --sources . \
  --evaluator thermo-craft --output thermo-direct --workers 1
python optimization-jobs/tutorials/domain-evaluators/run.py --sources . \
  --evaluator thermo-craft --output thermo-jobs --workers 2 --command-jobs
python optimization-jobs/tutorials/domain-evaluators/compare.py \
  thermo-direct/summary.json thermo-jobs/summary.json
```

Thermo searches log10 extraction pressures, minimizes negative efficiency and
checks a held-out condenser/turbine scenario. It has no intrinsic train/test
split; do not relabel efficiency as RMSE. Its descriptor and adapter select the
case, transform, penalty and metric names. Shared staging, budgets, identity,
execution, collection and replay code remain unchanged.

| Porting check | Hydro | Thermo | Rail | Robotics control | Population epidemiology |
|---|---|---|---|---|---|
| CLI | `describe`, `run FILE` | same | also `verify MODEL TRACE` | same | same |
| Request/result prefix | `hydro-craft` | `thermo-craft` | `rail-craft` | `robotics-control-craft` | `population-epidemiology` |
| Candidate outcomes | `complete` or classified failure | same convention | same convention | same convention | same convention |
| Objective | train RMSE or reservoir cost | negative cycle efficiency | weighted delay | tracking error and effort | probability/moment fit |
| Held-out check | fixed validation/test folds | changed operating scenario | separate corridor/closure | separate plant/disturbance | SIR `train_count` split or separate scenario |
| Randomness at fixed input | deterministic | deterministic | deterministic | deterministic | deterministic FSP/Markov propagation |

For each port verify the actual [model contract](model.md): parameter order,
units, bounds, invalid-row penalty, work limit and CLI schema, then declare its
wheel path and verify `SHA256SUMS`. All five currently return exit 0 for complete
rows, 1 for rejected requests and 2 for unsuccessful rows. Their wheels live in
`packages/`. Only hydro and thermo are implemented here. If a future population
port adds SSA sampling, fix training seeds and use disjoint validation seeds;
that is not the current probability solver's interface.

## 7. When to write a worker

| Need | Command job | Application worker |
|---|---|---|
| Use existing evaluator CLIs | Wrapper and descriptor | Registered worker implementation |
| Agent access over MCP | No; shell required | Yes |
| Claims, evidence and evaluation accounting in native result | No; tutorial checks are separate | Yes, per worker contract |
| Cooperative cancellation returning the best point | No; process group termination | Where the worker supports it |
| Scientific-comparison adapter | Descriptive report only here | Where its profile qualifies |
| Cost of adding a model | Adapter, descriptor, case, checks | Worker crate, registered model and CI |
