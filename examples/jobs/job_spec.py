#!/usr/bin/env python3
"""Stage a hashed evaluation or optimization spec without running it."""
import argparse
import importlib.util
from pathlib import Path
import sys


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--sources", type=Path, default=Path(__file__).resolve().parents[3])
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--kind", choices=["evaluation", "optimization"], default="evaluation")
    p.add_argument("--seed", type=int, default=1)
    p.add_argument("--evals", type=int, default=400)
    p.add_argument("--optimizer", choices=["cmaes", "de"], default="cmaes")
    p.add_argument("--workers", type=int, default=1)
    p.add_argument("--request-key", default="hydro-example")
    args = p.parse_args()
    if args.evals < 1 or args.seed < 0 or not 1 <= args.workers <= 8:
        raise ValueError("positive evals, nonnegative seed and 1..8 workers required")
    shared = args.sources.resolve() / "optimization-jobs/tutorials/domain-evaluators"
    sys.path.insert(0, str(shared))
    spec = importlib.util.spec_from_file_location("domain_driver", shared / "run.py")
    driver = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(driver)
    folder = args.output.resolve()
    driver.stage(args.sources.resolve(), "hydro-craft", folder)
    if args.kind == "evaluation":
        job = driver.command_spec(folder, ["wrap_cli.py"], {"result": "result.json", "status": "status.json"}, args.workers)
    else:
        job = driver.search_spec(folder, args.seed, args.evals, args.optimizer, args.workers)
    job["request_key"] = args.request_key
    driver.write(folder / "job.json", job)
    print(folder / "job.json")


if __name__ == "__main__":
    main()
