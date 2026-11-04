"""Median per-call cost of the installed package's Python APIs.

Times `evaluate` (full record), `batch`, `scores` and `score_matrix` for
one candidate and for 96 candidates of the packaged 90-day example, all
with one worker. Each value is the median of 15 rounds of 200 calls.
"""

import importlib
import json
import statistics
import sys
import time


def median_call(fn, calls=200, rounds=15):
    fn()
    times = []
    for _ in range(rounds):
        start = time.perf_counter()
        for _ in range(calls):
            fn()
        times.append((time.perf_counter() - start) / calls)
    return statistics.median(times)


def main():
    package = importlib.import_module(sys.argv[1])
    example = package.load_example()
    prepared = package.Prepared(example["model"])
    x = example["parameters"]
    rows = [[x[0] * (1 + (i % 13) * 0.003)] + x[1:] for i in range(96)]
    one = [{"id": "one", "parameters": x}]
    many = [{"id": f"c{i}", "parameters": r} for i, r in enumerate(rows)]
    cases = {
        "evaluate_one": lambda: prepared.evaluate(x),
        "batch_one": lambda: prepared.batch(one),
        "scores_one": lambda: prepared.scores(one),
        "batch_96": lambda: prepared.batch(many),
        "scores_96": lambda: prepared.scores(many),
    }
    if hasattr(prepared, "score_matrix"):
        cases["score_matrix_one"] = lambda: prepared.score_matrix([x])
        cases["score_matrix_96"] = lambda: prepared.score_matrix(rows)
    result = {name: median_call(fn) for name, fn in cases.items()}
    print(json.dumps({"module": sys.argv[1], "seconds_per_call": result}, indent=2))


if __name__ == "__main__":
    main()
