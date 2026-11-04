"""Find a reservoir rule that is good on average over three inflow scenarios.

Optimization with standard fcmaes minimizers and failure penalties. The
objective is the mean cost over dry, normal and wet scenarios. The search
box is [0, 1.25] for both controls, wider than the model's [0, 1], so
some candidates are ``invalid``; they get a penalty of 1e8 plus 1e8 times
the distance outside [0, 1]. Each candidate is scored through
``score_matrix([x])`` once per scenario. The same objective runs with
BiteOpt (``fcmaes.bitecpp.minimize``) and the C++ CMA-ES
(``fcmaes.cmaescpp.minimize``).

Run: python examples/python/fcmaes_robust_policy.py
"""

from collections import Counter
import time

from loguru import logger
import numpy as np
from scipy.optimize import Bounds

from fcmaes import bitecpp, cmaescpp
import synthetic as s
from hydro_craft import Prepared

logger.remove()
scenarios = [Prepared(s.reservoir(name)) for name in ("dry", "normal", "wet")]
statuses_seen = Counter()


def cost(x):
    total = 0.0
    for prepared in scenarios:
        scores, statuses, _ = prepared.score_matrix([list(x)])
        statuses_seen[statuses[0]] += 1
        if statuses[0] != "complete":
            # Same penalty for every scenario: the controls are invalid.
            return 1e8 * (1.0 + s.violation(x, [(0.0, 1.0), (0.0, 1.0)]))
        total += scores[0]
    return total / len(scenarios)


bounds = Bounds([0.0, 0.0], [1.25, 1.25])
for name, minimize in (("bitecpp", bitecpp.minimize), ("cmaescpp", cmaescpp.minimize)):
    statuses_seen.clear()
    start = time.perf_counter()
    if name == "bitecpp":
        result = minimize(cost, bounds, x0=[1.1, 1.1], max_evaluations=600,
                          rg=np.random.default_rng(11))
    else:
        result = minimize(cost, bounds, x0=[1.1, 1.1], input_sigma=0.3, popsize=16,
                          max_evaluations=600, rg=np.random.default_rng(11))
    elapsed = time.perf_counter() - start
    print(f"{name}: target {result.x[0]:.4f}, gain {result.x[1]:.4f}, mean cost {result.fun:.0f}, "
          f"{result.nfev} evaluations in {elapsed:.2f} s, scenario statuses {dict(statuses_seen)}")

x = [float(v) for v in result.x]
print(f"\n{'scenario':<8} {'cost':>11} {'shortage days':>14} {'spill m3':>12}")
for label, prepared in zip(("dry", "normal", "wet"), scenarios, strict=True):
    m = prepared.evaluate(x)["metrics"]
    print(f"{label:<8} {m['shortage_m3'] + 0.05 * m['spill_m3']:11.0f} {m['shortage_days']:14d} "
          f"{m['spill_m3']:12.0f}")
