# 0.3.0 performance evidence

Host for every file here: 2 vCPUs, Intel(R) Xeon(R) Processor @ 2.10GHz
(KVM), Rust 1.97.0, CPython 3.12.3. Timings ran inside the shared CPU
lock. Do not combine these numbers with the 0.2.0 log's numbers, which
came from an AMD EPYC 9V74 host with Rust 1.88.

- `plan.md`: each round's target workload and expected effect, written
  before its timing, plus the fixed keep/reject rule.
- `roundNN-<workload>.json`: `validation/performance_pair.py --samples 11`
  for round NN against the previous kept round. `lean-` marks
  `--before-scores --after-scores`; round 1 is lean before, matrix after.
- `rejected/`: timings of reverted attempts.
- `cumulative/`: the 0.2.0 baseline binary against round 10 for every
  workload, 11 samples each. `matrix-*` compares 0.2.0 lean scores with
  0.3.0 matrix calls, because 0.2.0 has no matrix API.
- `breakdown.json`: `examples/breakdown.rs` (feature `profiling`), median
  decode, evaluation and encode microseconds of 201 calls per workload
  and API, plus whole matrix calls.
- `worker-scaling.json`: `validation/worker_scaling.py` with the round 10
  benchmark binary at 1-4 workers, 7 alternating samples each.
- `optimization/`: installed-wheel Python optimization runs.
