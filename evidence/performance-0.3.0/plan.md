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

## Round 2: persistent worker pool

- Change: replace per-batch `std::thread::scope` workers with helper
  threads that persist for the process and wait on a condition
  variable. The calling thread evaluates its own batch together with up
  to `workers - 1` helpers through the same atomic index and per-index
  result slots, so record order and per-candidate results are unchanged.
  A forked child gets a fresh pool.
- Workload: `parallel` (96 full records, four workers on the 2-vCPU
  host), round 1 before, round 2 after.
- Expected: thread creation and join cost 100-430 us per batch here,
  against roughly 0.4 ms of serial work, so at least 2x.

## Round 3: typed full-record metrics

- Change: the domain returns typed catchment and reservoir metrics
  structs that serialize directly, instead of building nested
  `serde_json::Value` maps with string keys for every record. Fields are
  declared in sorted key order so the JSON text is unchanged; the CLI
  still converts the typed batch to a `Value`.
- Workload: `serial` (96 full records, one worker), round 2 before.
- Expected: the profile attributes about 180 us of evaluation and 90 us
  of encoding to full metrics, so about 1.5x.

## Round 4: typed trace rows

- Change: traced days become typed catchment and reservoir day structs,
  preallocated per traced candidate, instead of one `json!` map with
  six string keys per simulated day. Fields stay in sorted key order.
- Workload: `trace` (12 traced full records of 90 days), round 3 before.
- Expected: the profile spent about 1.4 ms building and 0.4 ms encoding
  the per-day maps, so at least 2x.

## Round 5: interleaved reservoir candidates

- Change: when no deadline or cancel file is monitored and `max_steps`
  covers every simulated day, the engine hands groups of up to eight
  untraced reservoir candidates to a lane kernel that advances them
  together over the shared daily inputs (widths 8, 4 and 1). Scalar
  and lane evaluation share one `reservoir_day` function, so each
  candidate performs the same floating-point operations in the same
  order; results are bit-identical. Valid candidates are charged one
  logical step per day, as before; invalid ones are rejected before
  their first step. Workers claim whole groups.
- Workload: `domain2` (48 full reservoir records of 2,400 days), round 4
  before.
- Expected: the micro-benchmark ran eight interleaved reservoir lanes
  about 6x faster per day than one, so at least 3x on this workload.

## Round 6: interleaved catchment candidates

- Change: the catchment gets the same treatment as the reservoir. Groups
  of up to four untraced candidates advance together over the shared
  forcing (widths 4, 2 and 1) through one shared `catchment_day` and
  `score_day` step, so results stay bit-identical; parameter
  validation and fold scoring are unchanged.
- Workload: `domain` (48 full catchment records of 2,400 days), round 5
  before.
- Expected: the micro-benchmark ran four interleaved catchment lanes
  about 3x faster per day than one at exponent 1, so at least 2x.
