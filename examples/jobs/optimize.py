"""Synthetic catchment adapter; optimize only training RMSE."""
import sys
from pathlib import Path
import numpy as np

# In a job support.py is staged beside this file; from a checkout use siblings.
if not (Path(__file__).parent / "support.py").exists():
    sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "optimization-jobs/tutorials/domain-evaluators"))
from support import search


def configuration(case):
    bounds = case["bounds"]
    return [b[0] for b in bounds], [b[1] for b in bounds], case["initial"]


def decode(xs):
    return np.asarray(xs)


def penalty(x):
    limits = [(1, 2000), (.1, 10), (.05, 1), (0, 1), (0, 1), (0, 100), (0, 30)]
    return 1000 * (1 + sum(max(lo-v, 0, v-hi) for v, (lo, hi) in zip(x, limits, strict=True)))


def report(records):
    metrics = records["nominal"]["metrics"]
    return {fold + "_rmse_mm": metrics[fold]["rmse_mm"] for fold in ["train", "validation", "test"]}


if __name__ == "__main__":
    search(sys.modules[__name__])
