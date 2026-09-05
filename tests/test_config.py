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


def test_default_config_and_forwarding_allowlists(tmp_path):
    import json
    import os

    config = tmp_path / ".rosbridge_server_rs" / "rosbridge.toml"
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    for first_start in (True, False):
        command = ["target/debug/rosbridge_server_rs", "--no-rosapi"]
        if first_start:
            command += ["--bind", f"127.0.0.1:{port}"]
        else:
            config.write_text(
                f"port = {port}\n"
                "topics_glob = []\n"
                'topics_pub_glob = ["/config_allowed"]\n'
                'topics_sub_glob = ["/config_allowed"]\n'
                "services_glob = []\n"
                "params_glob = []\n"
            )
            configured = config.read_text()
        process = subprocess.Popen(
            command,
            env={**os.environ, "HOME": str(tmp_path)},
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
            assert config.is_file()
            if first_start:
                assert "topics_sub_glob" in config.read_text()
                continue
            assert config.read_text() == configured

            async def exercise():
                async with websockets.connect(f"ws://127.0.0.1:{port}") as ws:
                    for op, resource, typ in [
                        ("subscribe", "topic", "std_msgs/String"),
                        ("advertise", "topic", "std_msgs/String"),
                        ("publish", "topic", "std_msgs/String"),
                        ("call_service", "service", "std_srvs/Trigger"),
                        ("advertise_service", "service", "std_srvs/Trigger"),
                        ("send_action_goal", "action", "example_interfaces/Fibonacci"),
                    ]:
                        await ws.send(
                            json.dumps(
                                {
                                    "op": op,
                                    resource: "/config_denied",
                                    "type": typ,
                                    "msg": {"data": "denied"},
                                    "args": {},
                                }
                            )
                        )
                        reply = json.loads(await asyncio.wait_for(ws.recv(), 5))
                        assert "denies" in json.dumps(reply)
                    await ws.send(
                        json.dumps(
                            {
                                "op": "subscribe",
                                "topic": "/config_allowed",
                                "type": "std_msgs/String",
                            }
                        )
                    )
                    for _ in range(20):
                        await ws.send(
                            json.dumps(
                                {
                                    "op": "publish",
                                    "topic": "/config_allowed",
                                    "type": "std_msgs/String",
                                    "msg": {"data": "allowed"},
                                }
                            )
                        )
                        try:
                            reply = json.loads(await asyncio.wait_for(ws.recv(), 0.25))
                        except asyncio.TimeoutError:
                            continue
                        assert reply["msg"] == {"data": "allowed"}
                        break
                    else:
                        raise AssertionError("allowed topic did not deliver")

            asyncio.run(exercise())
        finally:
            process.send_signal(signal.SIGINT)
            _, stderr = process.communicate(timeout=10)
            assert process.returncode == 0, stderr.decode()
