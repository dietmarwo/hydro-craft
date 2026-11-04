"""Seeded synthetic inputs shared by the examples.

Everything here is synthetic: a snowy mid-latitude catchment with a
seasonal temperature cycle, and an irrigation reservoir with seasonal
inflow and demand. No observed data are imported.
"""

import math
import random

from hydro_craft import Prepared

# Order of the catchment parameter vector.
NAMES = ["capacity", "exponent", "lp", "kfast", "kbase", "percolation", "degree"]
UNITS = ["mm", "-", "-", "1/day", "1/day", "mm/day", "mm/(degC day)"]
# Model limits enforced by hydro-craft (anything outside is "invalid").
LIMITS = [(1.0, 2000.0), (0.1, 10.0), (0.05, 1.0), (0.0, 1.0), (0.0, 1.0),
          (0.0, 100.0), (0.0, 30.0)]
# A physically plausible search box inside the limits.
BOUNDS = [(50.0, 400.0), (0.5, 5.0), (0.3, 1.0), (0.05, 0.6), (0.005, 0.2),
          (0.5, 6.0), (1.0, 6.0)]
# Parameters used to generate the synthetic "observed" runoff.
TRUTH = [180.0, 2.2, 0.7, 0.25, 0.03, 2.5, 3.5]


def forcing(years=4, seed=11, warming=0.0, wetting=1.0):
    """Daily precipitation, PET and temperature with cold winters."""
    rng = random.Random(seed)
    days = []
    for i in range(365 * years):
        phase = 2 * math.pi * (i % 365) / 365
        temperature = 4.0 - 9.0 * math.cos(phase) + rng.gauss(0.0, 2.5) + warming
        wet = rng.random() < 0.45
        precipitation = min(rng.expovariate(1 / 6.5) * wetting, 150.0) if wet else 0.0
        pet = max(0.0, 1.8 - 1.6 * math.cos(phase) + rng.gauss(0.0, 0.2))
        days.append({"day": i, "precipitation_mm": precipitation, "pet_mm": pet,
                     "temperature_c": temperature, "observed_mm": None})
    return days


def catchment(days, area_km2=120.0):
    """One warm-up year, two training years, then validation and test halves."""
    n = len(days)
    return {"kind": "catchment", "area_km2": area_km2, "forcing": days,
            "warmup": 365, "train_end": n - 365, "validation_end": n - 182}


def observe(model, parameters=TRUTH, noise=0.08, gap_every=17, seed=5):
    """Fill observed_mm from a traced run with multiplicative noise and gaps."""
    rng = random.Random(seed)
    # A traced run needs observations in every fold; seed them with zeros.
    seeded = model | {"forcing": [d | {"observed_mm": 0.0} for d in model["forcing"]]}
    row = Prepared(seeded).evaluate(parameters, trace=True)
    assert row["status"] == "complete", row
    days = []
    for i, (day, step) in enumerate(zip(model["forcing"], row["metrics"]["trace"], strict=True)):
        value = None if i % gap_every == 0 else step["runoff_mm"] * (1 + rng.gauss(0.0, noise))
        days.append(day | {"observed_mm": None if value is None else max(value, 0.0)})
    return model | {"forcing": days}


def calibration_case(years=4, seed=11):
    """A catchment model with noisy synthetic observations."""
    return observe(catchment(forcing(years, seed)))


def reservoir(scenario="normal", years=3, seed=3):
    """An irrigation reservoir; scenario scales inflow (dry, normal, wet)."""
    scale = {"dry": 0.65, "normal": 1.0, "wet": 1.4}[scenario]
    rng = random.Random(seed)
    inputs = []
    for i in range(365 * years):
        phase = 2 * math.pi * (i % 365) / 365
        # Spring snowmelt inflow peak and a summer irrigation demand peak.
        inflow = scale * max(0.0, 58_000 + 45_000 * math.sin(phase - 1.0) + rng.gauss(0, 12_000))
        demand = 42_000 + 22_000 * max(0.0, -math.cos(phase))
        evaporation = 1_500 + 1_200 * max(0.0, -math.cos(phase))
        inputs.append({"inflow_m3": inflow, "evaporation_m3": evaporation, "demand_m3": demand})
    return {"kind": "reservoir", "capacity_m3": 4.0e6, "initial_m3": 2.0e6,
            "outlet_m3_per_day": 70_000, "reserve_m3": 0.6e6, "inputs": inputs}


def violation(x, limits=LIMITS):
    """Total distance of a parameter vector outside the model limits."""
    return sum(max(lo - v, 0.0, v - hi) for v, (lo, hi) in zip(x, limits, strict=True))
