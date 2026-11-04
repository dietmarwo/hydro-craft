"""How would a warmer climate shift snow storage and runoff timing?

Application: scenario comparison with traced runs. The calibrated
parameters stay fixed while the daily temperature series is shifted by
0, +1, +2 and +4 degC. Each scenario is its own prepared model (its own
provenance hash); one traced record per scenario gives daily stores and
runoff for the last simulated year.

Run: python examples/python/warming_scenarios.py
"""

import statistics

import synthetic as s
from hydro_craft import Prepared

base = s.calibration_case()
print(f"{'warming':>8} {'peak SWE':>9} {'peak day':>9} {'snow days':>10} "
      f"{'spring %':>9} {'runoff':>8} {'ET':>8} {'max Q':>8}  model sha256")
print(f"{'degC':>8} {'mm':>9} {'of year':>9} {'>1 mm':>10} {'Mar-Jun':>9} "
      f"{'mm/yr':>8} {'mm/yr':>8} {'m3/s':>8}")
for warming in (0.0, 1.0, 2.0, 4.0):
    days = [d | {"temperature_c": d["temperature_c"] + warming} for d in base["forcing"]]
    model = base | {"forcing": days}
    prepared = Prepared(model)
    batch = prepared.batch([{"id": f"+{warming}", "parameters": s.TRUTH, "trace": True}])
    row = batch["records"][0]
    assert row["status"] == "complete", row
    last_year = row["metrics"]["trace"][-365:]
    snow = [d["stores_mm"][0] for d in last_year]
    runoff = [d["runoff_mm"] for d in last_year]
    peak_day = max(range(365), key=snow.__getitem__)
    spring = sum(runoff[59:181]) / sum(runoff)
    print(f"{warming:>+8.1f} {max(snow):9.1f} {peak_day:9d} {sum(v > 1 for v in snow):10d} "
          f"{100 * spring:9.1f} {sum(runoff):8.1f} {sum(d['et_mm'] for d in last_year):8.1f} "
          f"{max(d['discharge_m3_s'] for d in last_year):8.2f}  {batch['model_sha256'][:12]}")
    assert row["metrics"]["max_daily_balance_mm"] < 1e-9

print("\nThe daily water balance closes to rounding in every scenario.")
print(f"Mean last-year temperature without warming: "
      f"{statistics.mean(d['temperature_c'] for d in base['forcing'][-365:]):.2f} degC")
