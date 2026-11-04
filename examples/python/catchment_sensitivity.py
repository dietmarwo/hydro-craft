"""Which catchment parameters control the fit and the water balance?

Application: local and global sensitivity of a calibrated catchment.

1. One-at-a-time: perturb each parameter by -25 %, -10 %, +10 % and +25 %
   of the calibrated value and read train RMSE and test-fold volume bias
   from full records (``Prepared.batch``).
2. Morris elementary effects: 20 random trajectories over the plausible
   search box, scored through ``Prepared.score_matrix`` without JSON.

Run: python examples/python/catchment_sensitivity.py
"""

import random
import statistics
import time

import synthetic as s
from hydro_craft import Prepared

model = s.calibration_case()
prepared = Prepared(model)
base = s.TRUTH
reference = prepared.evaluate(base)
assert reference["status"] == "complete", reference
ref_test = reference["metrics"]["test"]["volume_bias_mm"]
print(f"forcing days: {len(model['forcing'])}, model sha256: {prepared.batch([{'id': 'x', 'parameters': base}])['model_sha256'][:16]}...")
print(f"reference train RMSE {reference['score']:.4f} mm/day, test volume bias {ref_test:.2f} mm\n")

# 1. One-at-a-time perturbations, one full-record batch per parameter.
print("One-at-a-time (train RMSE in mm/day; change of test volume bias in mm)")
print(f"{'parameter':<12} {'-25%':>9} {'-10%':>9} {'+10%':>9} {'+25%':>9}   d(test bias) at +10%")
for k, name in enumerate(s.NAMES):
    candidates = []
    for factor in (0.75, 0.9, 1.1, 1.25):
        x = list(base)
        lo, hi = s.LIMITS[k]
        x[k] = min(max(x[k] * factor, lo), hi)
        candidates.append({"id": f"{name}{factor}", "parameters": x})
    rows = prepared.batch(candidates)["records"]
    assert all(r["status"] == "complete" for r in rows), rows
    rmse = " ".join(f"{r['score']:9.4f}" for r in rows)
    shift = rows[2]["metrics"]["test"]["volume_bias_mm"] - ref_test
    print(f"{name:<12} {rmse}   {shift:+8.2f}")

# 2. Morris screening on the training RMSE.
rng = random.Random(42)
levels, trajectories, delta = 8, 20, 4 / 7
dim = len(s.NAMES)
effects = [[] for _ in range(dim)]
start = time.perf_counter()
calls = 0
for _ in range(trajectories):
    unit = [rng.randrange(levels - 4) / (levels - 1) for _ in range(dim)]
    order = list(range(dim))
    rng.shuffle(order)
    points = [list(unit)]
    for k in order:
        unit[k] += delta
        points.append(list(unit))
    rows = [[lo + u * (hi - lo) for u, (lo, hi) in zip(p, s.BOUNDS, strict=True)] for p in points]
    scores, statuses, _ = prepared.score_matrix(rows)
    calls += len(rows)
    assert all(status == "complete" for status in statuses), statuses
    for step, k in enumerate(order):
        effects[k].append((scores[step + 1] - scores[step]) / delta)
elapsed = time.perf_counter() - start
print(f"\nMorris elementary effects on train RMSE ({trajectories} trajectories, {calls} runs, {elapsed * 1e3:.1f} ms)")
print(f"{'parameter':<12} {'mu*':>9} {'sigma':>9}")
ranking = sorted(range(dim), key=lambda k: -statistics.mean(abs(e) for e in effects[k]))
for k in ranking:
    mu_star = statistics.mean(abs(e) for e in effects[k])
    sigma = statistics.stdev(effects[k])
    print(f"{s.NAMES[k]:<12} {mu_star:9.4f} {sigma:9.4f}")
print("\nLarge mu* means influential; sigma comparable to mu* means nonlinear or interacting.")
