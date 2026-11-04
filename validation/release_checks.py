"""Check the installed lean and matrix APIs and run the Python snippets in the docs.

Usage: release_checks.py MODULE TUTORIAL [MANUAL]. MANUAL defaults to
user-manual.md next to TUTORIAL. Each document's Python blocks run in
order in one namespace per document.
"""

import importlib
import json
from pathlib import Path
import re
import sys
from tempfile import TemporaryDirectory

module, tutorial, *rest = sys.argv[1:]
manual = rest[0] if rest else str(Path(tutorial).with_name("user-manual.md"))
package = importlib.import_module(module)
example = package.load_example()
prepared = package.Prepared(example["model"])
candidates = [{"id": "ok", "parameters": example["parameters"]},
              {"id": "bad", "parameters": [1] * 63}]
with TemporaryDirectory() as tmp:
    flag = Path(tmp) / "cancel"
    flag.touch()
    for budget in ({}, {"workers": 4}, {"max_steps": 1}, {"cancel_file": str(flag)}):
        full = prepared.batch(candidates, **budget)
        lean = prepared.scores(candidates, **budget)
        assert lean["model_sha256"] == full["model_sha256"]
        assert lean["records"] == [r | {"metrics": None} for r in full["records"]]
        if "cancel_file" in budget:
            assert lean["records"][0]["status"] == "cancelled"
        # Matrix rows share one dimension, so compare each row shape separately.
        for subset in ([candidates[0]], [candidates[1]], [candidates[0]] * 3):
            expected = prepared.scores([c | {"id": str(i)} for i, c in enumerate(subset)],
                                       **budget)["records"]
            scores, statuses, steps = prepared.score_matrix(
                [c["parameters"] for c in subset], **budget)
            assert statuses == [r["status"] for r in expected]
            assert steps == [r["steps"] for r in expected]
            for score, r in zip(scores, expected, strict=True):
                assert (score == r["score"]) if r["status"] == "complete" else score != score
    if module == "population_epidemiology":
        calendar = package.Prepared({"kind": "calendar", "initial_on_probability": 0.5,
                                     "segments": [{"duration": 1, "on_rate": 0, "off_rate": 0}] * 3})
        query = [{"id": "calendar", "parameters": []}]
        limited = calendar.scores(query, max_steps=2)["records"][0]
        assert (limited["status"], limited["steps"]) == ("work_limit", 2)
        canceled = calendar.scores(query, cancel_file=str(flag))["records"][0]
        assert (canceled["status"], canceled["steps"]) == ("cancelled", 0)
        complete = calendar.scores(query, max_steps=3)["records"][0]
        assert (complete["status"], complete["steps"], complete["score"]) == ("complete", 3, 0.5)
    if module == "rail_craft":
        closed = package.Prepared({
            "resources": [{"name": "one", "clearance": 0}], "horizon": 20,
            "trains": [{"name": "train", "release": 0, "planned": 0, "priority": 0,
                        "route": [{"resource": 0, "duration": 1}]}],
            "closures": [{"resource": 0, "start": a, "end": b} for a, b in ((0, 4), (2, 7), (7, 8))],
        })
        row = closed.evaluate([0, 0], trace=True)
        assert row["status"] == "complete"
        assert row["metrics"]["intervals"][0]["entry"] == 8
    if module == "robotics_control_craft":
        long = package.Prepared(example["model"] | {"dt_s": 1e-4})
        assert long.evaluate(example["parameters"])["status"] == "complete"
        traced = long.evaluate(example["parameters"], trace=True)
        assert traced["status"] == "invalid" and traced["steps"] == 10_001
import numpy

array = numpy.array([example["parameters"]] * 2, dtype=float)
assert prepared.score_matrix(array) == prepared.score_matrix(array.tolist())
for bad in ([[1.0, 2.0], [1.0]], numpy.zeros(3), [], numpy.zeros((4097, 2))):
    try:
        prepared.score_matrix(bad)
    except ValueError:
        pass
    else:
        raise AssertionError(f"score_matrix accepted {bad!r}")
try:
    prepared.scores([candidates[0] | {"trace": True}])
except ValueError:
    pass
else:
    raise AssertionError("scores accepted a traced candidate")

def run_blocks(path, label, minimum):
    snippets = re.findall(r"^```python\n(.*?)^```", Path(path).read_text(), re.M | re.S)
    assert len(snippets) >= minimum, (path, len(snippets))
    namespace = {}
    for i, snippet in enumerate(snippets, 1):
        exec(compile(snippet, f"{label}-{i}", "exec"), namespace)
    return len(snippets)


tutorial_blocks = run_blocks(tutorial, "tutorial", 4)
manual_blocks = run_blocks(manual, "manual", 15) if Path(manual).exists() else 0
print(json.dumps({"status": "passed", "tutorial_python_blocks": tutorial_blocks,
                  "manual_python_blocks": manual_blocks,
                  "lean_api_and_budget_parity": True, "matrix_api_parity": True}))
