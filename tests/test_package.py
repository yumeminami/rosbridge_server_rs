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

"""Smoke-test an installed binary, native rosapi and real ROS topic delivery."""
import asyncio
import json
import signal
import subprocess
import sys

import rclpy
import websockets
from std_msgs.msg import String


async def check(binary):
    rclpy.init()
    node = rclpy.create_node("package_smoke_test")
    publisher = node.create_publisher(String, "/package_probe", 10)
    process = subprocess.Popen([binary])
    try:
        for _ in range(100):
            assert process.poll() is None, "server exited during startup"
            try:
                ws = await websockets.connect("ws://127.0.0.1:9090")
                break
            except OSError:
                await asyncio.sleep(0.1)
        else:
            raise AssertionError("server did not start on the default port")
        try:
            await ws.send(
                json.dumps(
                    {
                        "op": "call_service",
                        "service": "/rosapi/get_ros_version",
                        "type": "rosapi/GetROSVersion",
                        "id": "version",
                        "args": {},
                    }
                )
            )
            reply = json.loads(await asyncio.wait_for(ws.recv(), 10))
            assert reply["id"] == "version" and reply["result"], reply
            assert reply["values"]["version"] == 2, reply
            await ws.send(
                json.dumps(
                    {
                        "op": "subscribe",
                        "topic": "/package_probe",
                        "type": "std_msgs/msg/String",
                    }
                )
            )
            for _ in range(100):
                if publisher.get_subscription_count():
                    break
                await asyncio.sleep(0.1)
            else:
                raise AssertionError("DDS subscription was not discovered")
            publisher.publish(String(data="installed package works"))
            reply = json.loads(await asyncio.wait_for(ws.recv(), 10))
            assert reply["op"] == "publish" and reply["topic"] == "/package_probe", reply
            assert reply["msg"]["data"] == "installed package works", reply
        finally:
            await ws.close()
    finally:
        process.send_signal(signal.SIGINT)
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
            raise AssertionError("server did not shut down")
        node.destroy_node()
        rclpy.shutdown()
    assert process.returncode == 0
    print(f"Package smoke test passed: {binary}")


if __name__ == "__main__":
    asyncio.run(check(sys.argv[1]))
