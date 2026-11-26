"""Exercise actual model inference on the patched engine; no mock scores."""
import json
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EXE = ROOT / ("engines/Gom2024/build-eigen/Release/katago.exe" if os.name == "nt" else "engines/Gom2024/build-eigen/katago")
MODEL = ROOT / "models/zhizi_renju28b_s1600.bin.gz"
CONFIG = ROOT / "config/analysis.cfg"
for n in (15, 19):
    command = [str(EXE), "analysis", "-model", str(MODEL), "-config", str(CONFIG)]
    request = dict(id=f"test-{n}", boardXSize=n, boardYSize=n, rules={"basicrule": "RENJU"},
                   moves=[], maxVisits=2, includePolicy=True)
    proc = subprocess.run(command, input=json.dumps(request)+"\n", capture_output=True,
                          cwd=str(ROOT), text=True, timeout=240, check=True)
    responses = []
    for line in proc.stdout.splitlines():
        try:
            responses.append(json.loads(line))
        except ValueError:
            pass
    responses = [x for x in responses if x.get("id") == request["id"]]
    if any("error" in x or ("warning" in x and x.get("field") == "rules") for x in responses):
        raise RuntimeError(responses)
    result = next((x for x in responses if "rootInfo" in x and not x.get("isDuringSearch")), None)
    if not result or len(result.get("policy", [])) != n*n+1 or "forbidden" not in result:
        raise RuntimeError(f"No valid real {n}x{n} output: {responses}\n{proc.stderr[-2000:]}")
    print(f"{n}x{n}: actual model inference, visits={result['rootInfo']['visits']}")
