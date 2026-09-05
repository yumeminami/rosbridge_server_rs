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

"""Shared server lifecycle for parity tests and benchmarks."""
import contextlib
import os
from pathlib import Path
import signal
import socket
import subprocess
import time

UPSTREAM = Path(os.environ.get("ROSBRIDGE_SUITE_PATH", "/upstream"))


def python_environment():
    env = os.environ.copy()
    return env


@contextlib.contextmanager
def server(kind, log_path):
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    if kind == "rust":
        command = [
            "target/release/rosbridge_server_rs",
            "--no-rosapi",
            "--bind",
            f"127.0.0.1:{port}",
        ]
    else:
        command = [
            "python3",
            f"/opt/ros/{os.environ['ROS_DISTRO']}/lib/rosbridge_server/rosbridge_websocket",
            "--ros-args",
            "-p",
            f"port:={port}",
            "-p",
            "unregister_timeout:=0.5",
        ]
    with open(log_path, "w") as log:
        process = subprocess.Popen(command, env=python_environment(), stdout=log, stderr=log)
        try:
            for _ in range(200):
                if process.poll() is not None:
                    raise RuntimeError(Path(log_path).read_text())
                try:
                    with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                        break
                except OSError:
                    time.sleep(0.05)
            else:
                raise TimeoutError("server startup")
            yield process, port
        finally:
            if process.poll() is None:
                process.send_signal(signal.SIGINT)
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
