# hydro-craft 0.3.0 performance plan

Host: 2 vCPUs, Intel(R) Xeon(R) Processor @ 2.10GHz (KVM), Rust 1.97.0,
CPython 3.12.3. Every timing below runs inside the shared CPU lock.

Each round is written here before it is measured. The decision rule
is fixed for every round: keep the round if the declared workload's
median speedup from `validation/performance_pair.py --samples 11`
against the previous kept round's binary is at least 1.03x and the
paired output check passes; otherwise revert it and record it as
rejected.

## Profile of 0.2.0 on this host

A scratch copy of the 0.2.0 crate timed decode, evaluation and encode
separately (50 repeats, microseconds per call):

| Workload | Mode | Decode | Evaluate | Encode |
| --- | --- | ---: | ---: | ---: |
| one | full | 0.5 | 3.9 | 0.9 |
| serial | full | 64 | 420 | 108 |
| serial | lean | 44 | 239 | 19 |
| parallel | full | 38 | 454 | 125 |
| trace | full | 10 | 1,429 | 428 |
| invalid | full | 1,388 | 59 | 174 |
| domain | full | 19 | 2,830 | 36 |
| domain | lean | 15 | 2,396 | 11 |
| domain2 | full | 9 | 2,505 | 18 |
| domain2 | lean | 10 | 2,379 | 10 |

Creating and joining scoped threads costs 100-430 us per batch on this
host, so `parallel` (four workers, 96 candidates) is several times
slower than `serial`. Both domain kernels are latency-bound recurrences
of about 19 ns per simulated day. A micro-benchmark that interleaves
independent candidates over the same days ran the reservoir about 6x
and the catchment about 3x faster per day.

## Round 1: typed score-matrix API

- Change: `Prepared::score_matrix(rows, parameters, budget)` evaluates
  row-major untraced parameters and returns `MatrixScore` values with
  no candidate JSON decode and no result JSON encode. The Python
  binding takes `Vec<Vec<f64>>`, releases the GIL and returns scores,
  statuses and steps. Inline and scoped-worker dispatch is shared with
  the JSON path.
- Workload: `serial` (96 candidates, one worker), 0.2.0 lean
  `scores_json` before, matrix after.
- Expected: the profile shows about 44 us decode and 19 us encode around
  about 240 us of evaluation, so about 1.2x.
