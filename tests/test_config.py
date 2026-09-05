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

"""Exercise TOML precedence, WebSocket metadata and actual file output."""
import asyncio
import signal
import socket
import subprocess
import time

import websockets


def test_config_and_file_logs(tmp_path):
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    logs = tmp_path / "logs"
    config = tmp_path / "bridge.toml"
    config.write_text(
        f'port = 1\nurl_path = "/bridge"\nno_rosapi = true\n'
        f'[log]\ndirectory = "{logs}"\nrotation = "never"\nconsole = false\n'
    )
    process = subprocess.Popen(
        [
            "target/debug/rosbridge_server_rs",
            "--config",
            str(config),
            "--bind",
            f"127.0.0.1:{port}",
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    try:
        deadline = time.monotonic() + 10
        while True:
            assert process.poll() is None
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                    break
            except OSError:
                assert time.monotonic() < deadline
                time.sleep(0.05)

        async def exercise():
            try:
                async with websockets.connect(f"ws://127.0.0.1:{port}/wrong"):
                    raise AssertionError("wrong path accepted")
            except websockets.InvalidStatusCode as error:
                assert error.status_code == 404
            async with websockets.connect(
                f"ws://127.0.0.1:{port}/bridge?token=not-for-logs",
                origin="http://test-client",
                user_agent_header="rosbridge-config-test",
            ) as ws:
                await ws.send('{"op":"subscribe","topic":"/missing_config_test_topic"}')
                await asyncio.wait_for(ws.recv(), timeout=5)
                await ws.close(code=1000, reason="test complete")

        asyncio.run(exercise())
        time.sleep(0.1)
    finally:
        process.send_signal(signal.SIGINT)
        stdout, stderr = process.communicate(timeout=10)
    assert process.returncode == 0, stderr.decode()
    assert stdout == b"" and stderr == b""
    text = (logs / "rosbridge_server_rs.log").read_text()
    for expected in [
        "WebSocket client handshake",
        "127.0.0.1",
        "http://test-client",
        "rosbridge-config-test",
        "Client connected",
        "Client disconnected",
        "Operation failed",
        "/missing_config_test_topic",
        "duration_seconds",
        "test complete",
    ]:
        assert expected in text
    assert "not-for-logs" not in text
    assert "\x1b[" not in text
