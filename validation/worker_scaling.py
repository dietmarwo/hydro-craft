"""Time one benchmark binary at several worker counts (CRAFT_WORKERS)."""

import argparse
import json
import os
import statistics
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", help="compiled examples/performance.rs")
    parser.add_argument("fixture")
    parser.add_argument("--workloads", default="parallel,domain,domain2")
    parser.add_argument("--workers", default="1,2,4")
    parser.add_argument("--apis", default="full,matrix")
    parser.add_argument("--samples", type=int, default=7)
    args = parser.parse_args()
    workers = [int(w) for w in args.workers.split(",")]
    rows = []
    for workload in args.workloads.split(","):
        for api in args.apis.split(","):
            samples = {w: [] for w in workers}
            for i in range(args.samples):
                # Alternate the order so drift affects every worker count alike.
                for w in workers if i % 2 == 0 else workers[::-1]:
                    env = {k: v for k, v in os.environ.items()
                           if k not in ("CRAFT_SCORES", "CRAFT_MATRIX")}
                    env["CRAFT_WORKERS"] = str(w)
                    if api == "lean":
                        env["CRAFT_SCORES"] = "1"
                    elif api == "matrix":
                        env["CRAFT_MATRIX"] = "1"
                    out = subprocess.check_output([args.binary, args.fixture, workload], env=env)
                    samples[w].append(float(out))
            medians = {w: statistics.median(s) for w, s in samples.items()}
            rows.append({"workload": workload, "api": api, "samples_seconds": samples,
                         "median_seconds": medians,
                         "speedup_vs_1": {w: medians[workers[0]] / m for w, m in medians.items()}})
    print(json.dumps({"cpus": os.cpu_count(), "rows": rows}, indent=2))


if __name__ == "__main__":
    main()
