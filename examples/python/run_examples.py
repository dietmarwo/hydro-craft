"""Run every Python example and save its output under outputs/.

Run from anywhere with the installed package:
python examples/python/run_examples.py
"""

import json
from pathlib import Path
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
EXAMPLES = [
    "catchment_sensitivity",
    "warming_scenarios",
    "reservoir_policy_study",
    "scipy_calibration",
    "fcmaes_cmaes_population",
    "fcmaes_robust_policy",
]


def main():
    (HERE / "outputs").mkdir(exist_ok=True)
    summary = {}
    for name in EXAMPLES:
        start = time.perf_counter()
        result = subprocess.run([sys.executable, str(HERE / f"{name}.py")], cwd=HERE,
                                capture_output=True, text=True, timeout=300, check=False)
        seconds = time.perf_counter() - start
        (HERE / "outputs" / f"{name}.txt").write_text(result.stdout + result.stderr)
        if result.returncode != 0:
            raise SystemExit(f"{name} failed:\n{result.stderr}")
        summary[name] = round(seconds, 3)
    print(json.dumps({"status": "passed", "wall_seconds": summary}, indent=2))


if __name__ == "__main__":
    main()
