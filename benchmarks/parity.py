#
# Copyright (c) 2026 Wing Mun Fung
#
# This program and the accompanying materials are made available under the
# terms of the Eclipse Public License 2.0, available at
# https://www.eclipse.org/legal/epl-2.0/, or the Apache License, Version 2.0,
# available at https://www.apache.org/licenses/LICENSE-2.0.
#
# SPDX-License-Identifier: EPL-2.0 OR Apache-2.0
#

"""Execute the same upstream WebSocket assertions against both implementations."""
import json
import os
from pathlib import Path
import subprocess
import sys
from server import UPSTREAM, python_environment, server

out = Path(os.environ.get("ROSBRIDGE_TEST_RESULTS", "benchmarks/results"))
out.mkdir(parents=True, exist_ok=True)
rows = []
executed = []
selected = set(sys.argv[1:])
if selected and (out / "parity.json").exists():
    rows = [
        row for row in json.loads((out / "parity.json").read_text()) if row["case"] not in selected
    ]
cases = sorted((UPSTREAM / "rosbridge_server/test/websocket").glob("*.test.py"))
if not cases or selected - {case.name for case in cases}:
    sys.exit("No upstream cases found, or a requested case does not exist")
for case in cases:
    if selected and case.name not in selected:
        continue
    for kind in ("python", "rust"):
        label = kind + "-" + case.name
        load = None
        with server(kind, out / (label + ".server.log")) as (_, port):
            if case.name == "event_loop_starvation.test.py":
                load = subprocess.Popen(
                    [sys.executable, str(case.parent / "starvation_load_publisher.py")],
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )
            try:
                result = subprocess.run(
                    [sys.executable, "benchmarks/upstream_case.py", str(case), str(port)],
                    env=python_environment(),
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT,
                    text=True,
                    timeout=100,
                )
                status, output = ("passed" if result.returncode == 0 else "failed"), result.stdout
            except subprocess.TimeoutExpired as error:
                status, output = "timeout", str(error.stdout)
            finally:
                if load:
                    load.terminate()
                    load.wait(timeout=5)
            (out / (label + ".log")).write_text(output)
            row = dict(server=kind, case=case.name, status=status)
            rows.append(row)
            executed.append(row)
            (out / "parity.json").write_text(json.dumps(rows, indent=2))
            print(label, status, flush=True)

sys.exit(0 if all(row["status"] == "passed" for row in executed) else 1)
