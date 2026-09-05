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

"""The compatibility runner must report failures to CI, not just write a log."""
import contextlib
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import types
import unittest
from unittest.mock import patch, Mock

RUNNER = Path(__file__).resolve().parents[1] / "benchmarks/parity.py"


class ParityRunnerTests(unittest.TestCase):
    def run_case(self, outcome, *, missing=False, case="smoke.test.py", output="test output"):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cases = root / "rosbridge_server/test/websocket"
            cases.mkdir(parents=True)
            if not missing:
                (cases / case).touch()
            support = types.ModuleType("server")
            support.UPSTREAM = root
            support.python_environment = lambda: {}
            support.server = lambda *args: contextlib.nullcontext((None, 9090))
            result_dir = root / "results"
            with (
                patch.dict(sys.modules, {"server": support}),
                patch.dict(os.environ, {"ROSBRIDGE_TEST_RESULTS": str(result_dir)}),
                patch.object(sys, "argv", [str(RUNNER)]),
                patch("subprocess.Popen", return_value=Mock()),
                patch(
                    "subprocess.run",
                    side_effect=outcome if isinstance(outcome, Exception) else None,
                    return_value=subprocess.CompletedProcess(
                        [], outcome if isinstance(outcome, int) else 0, stdout=output
                    ),
                ),
                self.assertRaises(SystemExit) as exit_status,
            ):
                runpy.run_path(str(RUNNER), run_name="__main__")
            results = result_dir / "parity.json"
            return (
                exit_status.exception.code,
                json.loads(results.read_text()) if results.exists() else [],
            )

    def test_success(self):
        code, results = self.run_case(0)
        self.assertEqual(code, 0)
        self.assertEqual([row["status"] for row in results], ["passed", "passed"])

    def test_failure(self):
        code, results = self.run_case(1)
        self.assertEqual(code, 1)
        self.assertEqual([row["status"] for row in results], ["failed", "failed"])

    def test_timeout(self):
        code, results = self.run_case(subprocess.TimeoutExpired("test", 100))
        self.assertEqual(code, 1)
        self.assertEqual([row["status"] for row in results], ["timeout", "timeout"])

    def test_humble_baseline_failure_does_not_hide_rust_failure(self):
        with patch.dict(os.environ, {"ROS_DISTRO": "humble"}):
            code, results = self.run_case(
                1,
                case="event_loop_starvation.test.py",
                output="Event-loop starvation detected",
            )
        self.assertEqual(code, 1)
        self.assertEqual([row["status"] for row in results], ["known baseline failure", "failed"])

    def test_missing_cases(self):
        code, results = self.run_case(0, missing=True)
        self.assertNotEqual(code, 0)
        self.assertEqual(results, [])


if __name__ == "__main__":
    unittest.main()
