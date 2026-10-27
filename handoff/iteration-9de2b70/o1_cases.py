#!/usr/bin/env python3
"""The three O-1 cases, run against one version of the gate.

O-1 says: three of the twenty-six generated-card mutations make
`check_candidate` raise out of its own `parse` instead of returning a verdict.
The three inputs are fixed and hashed (see `o1-inputs.sha256`), so the same
three files can be replayed against the gate of any SHA.

The output is one JSON record per case, and a summary line. A case is
`managed rejection` only if the gate *returns* a result that does not accept the
candidate; a raised exception is recorded as `traceback` even though the job
would still fail closed.

Usage: o1_cases.py <gate.py> <out.json>
"""
import importlib.util
import json
import os
import sys
import traceback

FIXTURES = "/home/user/fits-verify-9de2b70/01-fixtures"
ORIGINAL = os.path.join(FIXTURES, "orig_int16.fits")
CASES = [
    ("mutant-15-PCOUNT", os.path.join(FIXTURES, "derived", "mutant-15.fits.fz")),
    ("mutant-17-BITPIX", os.path.join(FIXTURES, "derived", "mutant-17.fits.fz")),
    ("mutant-19-GCOUNT", os.path.join(FIXTURES, "derived", "mutant-19.fits.fz")),
]


def load_gate(path):
    spec = importlib.util.spec_from_file_location("gate_under_test", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    if len(sys.argv) != 3:
        print(__doc__)
        return 2
    gate_path, out_path = sys.argv[1], sys.argv[2]
    os.environ["PATH"] = "/tmp/tools/src/.libs:" + os.environ["PATH"]
    os.environ["LD_LIBRARY_PATH"] = "/tmp/tools/src/.libs"

    gate = load_gate(gate_path)
    scratch = "/tmp/o1-scratch"
    evidence_root = "/tmp/o1-evidence"
    os.makedirs(scratch, exist_ok=True)
    os.makedirs(evidence_root, exist_ok=True)
    compiled = getattr(gate, "compiled_funpack", None)
    evidence = gate.Evidence(evidence_root)

    records = []
    for label, candidate in CASES:
        case = dict(name=os.path.basename(candidate), expect="rewritten", images=1)
        record = dict(label=label, candidate=candidate,
                      candidate_sha256=__import__("hashlib").sha256(
                          open(candidate, "rb").read()).hexdigest())
        try:
            result = gate.check_candidate(ORIGINAL, candidate, case, scratch,
                                          evidence, label)
        except Exception as error:                      # the O-1 symptom
            record.update(outcome="traceback",
                          exception="%s: %s" % (type(error).__name__, error),
                          traceback=traceback.format_exc().splitlines()[-4:],
                          rejected=None)
        else:
            accepted = result.get("accepted")
            record.update(outcome="verdict", accepted=accepted,
                          rejected=accepted is False,
                          problems=result.get("problems", [])[:3],
                          funpack_exit=result.get("funpack_exit"),
                          fitsverify_exit=result.get("fitsverify_exit"))
        records.append(record)
        print("%-18s %-10s %s" % (label, record["outcome"],
                                  record.get("exception") or
                                  "accepted=%s problems=%s" % (record["accepted"],
                                                               record["problems"][:1])))

    summary = dict(
        gate=gate_path,
        gate_sha256=__import__("hashlib").sha256(open(gate_path, "rb").read()).hexdigest(),
        cases=records,
        tracebacks=sum(1 for r in records if r["outcome"] == "traceback"),
        managed_rejections=sum(1 for r in records if r.get("rejected") is True),
        accepted=sum(1 for r in records if r.get("accepted") is True),
    )
    with open(out_path, "w") as handle:
        json.dump(summary, handle, indent=2)
    print("\ntracebacks=%d managed_rejections=%d accepted=%d -> %s"
          % (summary["tracebacks"], summary["managed_rejections"],
             summary["accepted"], out_path))
    return 0


if __name__ == "__main__":
    sys.exit(main())
