"""Replay selected parameters through the CLI, retaining unsuccessful rows."""
import argparse
import json
import hashlib
import math
from pathlib import Path
import optimize as adapter
from support import canonical, write
from wrap_cli import evaluate


def check(best, case, binary, folder):
    report = dict(passed=False, search=best, records={}, metrics={})
    if best.get("status") != "complete":
        report["error"] = "search has no complete candidate"
        return report
    expected_hash = hashlib.sha256(canonical(case["model"]).encode()).hexdigest()
    if best.get("model_sha256") != expected_hash:
        raise ValueError("winner model hash differs from check model")
    if not 0 < best["evaluations"] <= best["budget"] or sum(best["statuses"].values()) != best["evaluations"]:
        raise ValueError("invalid search accounting")
    models = {"nominal": case["model"], **case.get("held_out_models", {})}
    for name, model in models.items():
        request = dict(schema=case["request_schema"], model=model,
                       candidates=[dict(id="winner", parameters=best["parameters"], trace=False)],
                       budget=dict(workers=1, max_steps=case["max_steps"]))
        path = folder / (name + "-request.json")
        path.write_text(canonical(request) + "\n")
        data, status = evaluate(binary, path)
        write(folder / (name + "-replay.json"), data)
        report["records"][name] = data["records"][0]
        report.setdefault("cli_status", {})[name] = status
    nominal = report["records"]["nominal"]
    # CLI and wheel evaluate the same model; accommodate only roundoff in replay.
    matched = nominal["status"] == "complete" and math.isclose(
        nominal["score"], best["score"], rel_tol=1e-12, abs_tol=1e-12)
    all_complete = all(r["status"] == "complete" for r in report["records"].values())
    report["passed"] = matched and all_complete
    report["score_replayed"] = matched
    if nominal["status"] == "complete":
        report["metrics"] = adapter.report(report["records"])
    report["scope"] = case["scope"]
    return report


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--best", type=Path, default=Path("best.json"))
    p.add_argument("--case", type=Path, default=Path("case.json"))
    p.add_argument("--binary", default="binary")
    p.add_argument("--output", type=Path, default=Path("check-output"))
    args = p.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    result = check(json.loads(args.best.read_text()), json.loads(args.case.read_text()), args.binary, args.output)
    write(args.output / "checks.json", result)
    if not result["passed"]:
        raise ValueError("winner replay or held-out check failed; records retained")


if __name__ == "__main__":
    main()
