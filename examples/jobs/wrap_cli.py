"""Capture a bounded evaluator's stdout; exit 2 is a classified data outcome."""
import argparse
import json
from pathlib import Path
import subprocess
from support import write


def evaluate(binary, request, timeout=60):
    p = subprocess.run([str(Path(binary).resolve()), "run", str(request)],
                       capture_output=True, text=True, timeout=timeout)
    status = dict(cli_exit=p.returncode, stderr=p.stderr)
    if p.returncode not in (0, 2):
        raise RuntimeError("CLI rejected request: " + p.stderr)
    data = json.loads(p.stdout)
    records = data.get("records")
    if not isinstance(records, list) or not records:
        raise ValueError("CLI did not return records")
    complete = all(r.get("status") == "complete" for r in records)
    if (p.returncode == 0) != complete:
        raise ValueError("CLI exit and record statuses disagree")
    status.update(all_complete=complete, records=len(records))
    return data, status


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--binary", default="binary")
    p.add_argument("--request", default="request.json")
    p.add_argument("--output", type=Path, default=Path("."))
    args = p.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    try:
        data, status = evaluate(args.binary, args.request)
    except Exception as error:
        write(args.output / "status.json", dict(status="wrapper_failed", error=str(error)))
        raise
    write(args.output / "result.json", data)
    write(args.output / "status.json", status)


if __name__ == "__main__":
    main()
