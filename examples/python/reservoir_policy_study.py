"""Which release rule works across dry, normal and wet inflow years?

Application: policy screening under scenario uncertainty. A 21 x 21 grid
of (target fraction, release gain) rule curves is scored through
``score_matrix`` for three inflow scenarios. The study reports the best
rule per scenario and the rule with the smallest worst-case regret, then
replays that rule with full records to read shortage days, the longest
shortage streak and spill.

Run: python examples/python/reservoir_policy_study.py
"""

import time

import synthetic as s
from hydro_craft import Prepared

scenarios = {name: Prepared(s.reservoir(name)) for name in ("dry", "normal", "wet")}
grid = [[t / 20, g / 20] for t in range(21) for g in range(21)]

start = time.perf_counter()
costs = {}
for name, prepared in scenarios.items():
    scores, statuses, steps = prepared.score_matrix(grid, workers=2)
    assert set(statuses) == {"complete"} and set(steps) == {365 * 3}
    costs[name] = scores
elapsed = time.perf_counter() - start
print(f"scored {len(grid)} policies x {len(scenarios)} scenarios in {elapsed * 1e3:.1f} ms\n")

print(f"{'scenario':<8} {'best target':>11} {'best gain':>10} {'cost':>12}")
best = {}
for name, scores in costs.items():
    i = min(range(len(grid)), key=scores.__getitem__)
    best[name] = scores[i]
    print(f"{name:<8} {grid[i][0]:11.2f} {grid[i][1]:10.2f} {scores[i]:12.0f}")

# Worst-case regret: the largest excess over each scenario's best rule.
regret = [max(costs[n][i] - best[n] for n in costs) for i in range(len(grid))]
robust = min(range(len(grid)), key=regret.__getitem__)
print(f"\nminimax-regret rule: target {grid[robust][0]:.2f}, gain {grid[robust][1]:.2f}, "
      f"worst regret {regret[robust]:.0f} (cost units: m3 shortage + 0.05 m3 spill)\n")

print(f"{'scenario':<8} {'cost':>12} {'shortage m3':>12} {'short days':>11} "
      f"{'longest':>8} {'spill m3':>12} {'balance':>8}")
for name, prepared in scenarios.items():
    row = prepared.evaluate(grid[robust])
    m = row["metrics"]
    print(f"{name:<8} {row['score']:12.0f} {m['shortage_m3']:12.0f} {m['shortage_days']:11d} "
          f"{m['longest_shortage_days']:8d} {m['spill_m3']:12.0f} {m['max_balance_m3']:8.1e}")
print("\nDecisions use the previous day's storage only, so the rule is non-anticipatory.")
