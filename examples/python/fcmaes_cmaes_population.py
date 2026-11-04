"""Calibrate the catchment with fcmaes CMA-ES, one native call per generation.

Optimization with fcmaes ask/tell: ``fcmaes.cmaes.Cmaes.ask`` returns a
whole population as a 2-D NumPy array, which ``score_matrix`` evaluates
with two native workers and without JSON. Rows that are not complete
(here: ``lp`` above the model limit of 1.0) get a graded penalty before
``tell``. The sampler is seeded, so the run is reproducible.

Run: python examples/python/fcmaes_cmaes_population.py
"""

from collections import Counter
import time

from loguru import logger
import numpy as np
from scipy.optimize import Bounds

from fcmaes import cmaes
import synthetic as s
from hydro_craft import Prepared

logger.remove()
prepared = Prepared(s.calibration_case())
lower = [lo for lo, _ in s.BOUNDS]
upper = [hi for _, hi in s.BOUNDS]
upper[2] = 1.2  # lp above 1.0 is outside the model limits
rng = np.random.default_rng(3)
es = cmaes.Cmaes(bounds=Bounds(lower, upper), x0=[(a + b) / 2 for a, b in zip(lower, upper)],
                 input_sigma=0.3, popsize=32, max_evaluations=8000, rg=rng,
                 randn=lambda *shape: rng.standard_normal(shape))
statuses_seen = Counter()
best = (float("inf"), None)
generations = 0
start = time.perf_counter()
while True:
    xs = es.ask()
    scores, statuses, steps = prepared.score_matrix(xs, workers=2)
    statuses_seen.update(statuses)
    ys = np.array(scores)
    for i, status in enumerate(statuses):
        if status != "complete":
            ys[i] = 1e3 * (1.0 + s.violation(xs[i]))
    i = int(np.argmin(ys))
    if ys[i] < best[0]:
        best = (float(ys[i]), xs[i].copy())
    generations += 1
    if es.tell(ys) != 0 or generations * es.popsize >= 8000:
        break
elapsed = time.perf_counter() - start
print(f"{generations} generations of {es.popsize} in {elapsed:.2f} s; statuses {dict(statuses_seen)}")
print(f"best train RMSE {best[0]:.5f} mm/day")
row = prepared.evaluate(best[1].tolist())
print("validation RMSE {:.4f}, test RMSE {:.4f} mm/day".format(
    row["metrics"]["validation"]["rmse_mm"], row["metrics"]["test"]["rmse_mm"]))
print("parameters:", ", ".join(f"{n}={v:.4g}" for n, v in zip(s.NAMES, best[1], strict=True)))
