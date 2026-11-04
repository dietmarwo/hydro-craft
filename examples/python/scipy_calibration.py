"""Calibrate the catchment with SciPy differential evolution.

Optimization with SciPy: ``differential_evolution(vectorized=True)``
hands the objective a whole generation at once, which goes to
``score_matrix`` in one native call. The search box for ``lp`` is
deliberately wider than the model allows (up to 1.2 instead of 1.0), so
some candidates come back ``invalid``. They get a graded penalty: a
large constant plus the distance outside the model limits, which keeps
the penalty landscape pointing back to the valid region.

The winner is replayed with a full record to compare train, validation
and test folds with the generating parameters.

Run: python examples/python/scipy_calibration.py
"""

from collections import Counter
import time

import numpy as np
from scipy.optimize import differential_evolution

import synthetic as s
from hydro_craft import Prepared

prepared = Prepared(s.calibration_case())
bounds = list(s.BOUNDS)
bounds[2] = (0.3, 1.2)  # lp above 1.0 is outside the model limits
statuses_seen = Counter()
evaluations = 0


def objective(population):
    """Vectorized objective: population has shape (dimension, size)."""
    global evaluations
    rows = population.T
    scores, statuses, _ = prepared.score_matrix(rows)
    statuses_seen.update(statuses)
    evaluations += len(rows)
    values = np.array(scores)
    for i, status in enumerate(statuses):
        if status != "complete":
            values[i] = 1e3 * (1.0 + s.violation(rows[i]))
    return values


start = time.perf_counter()
result = differential_evolution(objective, bounds, vectorized=True, updating="deferred",
                                popsize=12, maxiter=120, tol=1e-8, seed=7, polish=False)
elapsed = time.perf_counter() - start
print(f"evaluations {evaluations} in {elapsed:.2f} s; statuses {dict(statuses_seen)}")
print(f"best train RMSE {result.fun:.5f} mm/day\n")

final = prepared.evaluate(result.x.tolist())
truth = prepared.evaluate(s.TRUTH)
print(f"{'parameter':<12} {'found':>9} {'truth':>9}  unit")
for name, unit, found, true in zip(s.NAMES, s.UNITS, result.x, s.TRUTH, strict=True):
    print(f"{name:<12} {found:9.4f} {true:9.4f}  {unit}")
print(f"\n{'fold':<11} {'RMSE found':>11} {'RMSE truth':>11} {'bias found':>11} {'bias truth':>11}")
for fold in ("train", "validation", "test"):
    a, b = final["metrics"][fold], truth["metrics"][fold]
    print(f"{fold:<11} {a['rmse_mm']:11.4f} {b['rmse_mm']:11.4f} "
          f"{a['volume_bias_mm']:11.2f} {b['volume_bias_mm']:11.2f}")
print("\nSelect on train, check validation, report test once. The noise in the")
print("synthetic observations limits how closely any parameter set can fit.")
