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

"""Real ROS 2 + WebSocket contract tests. No Python is used by the Rust server."""
import asyncio
import base64
import io
import json
import os
import signal
import socket
import subprocess
import threading
import time
import uuid

import cbor2
import pytest
import rclpy
import websockets
from PIL import Image
from rclpy.action import ActionClient, ActionServer, CancelResponse, GoalResponse
from rclpy.executors import MultiThreadedExecutor
from rclpy.qos import QoSProfile, ReliabilityPolicy, DurabilityPolicy
from rclpy.serialization import deserialize_message
from std_msgs.msg import String, Int32, UInt8MultiArray, Float64MultiArray, Header
from geometry_msgs.msg import PoseStamped
from example_interfaces.srv import AddTwoInts
from test_msgs.action import Fibonacci
from test_msgs.msg import BasicTypes, Arrays, BoundedSequences, UnboundedSequences


def wait_until(predicate, timeout=8):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.02)
    raise AssertionError("condition did not become true")


@pytest.fixture(scope="module")
def ros():
    rclpy.init()
    node = rclpy.create_node("rust_bridge_test_" + uuid.uuid4().hex[:8])
    executor = MultiThreadedExecutor(num_threads=4)
    executor.add_node(node)
    thread = threading.Thread(target=executor.spin, daemon=True)
    thread.start()
    yield node
    executor.shutdown()
    thread.join(timeout=5)
    node.destroy_node()
    rclpy.shutdown()


@pytest.fixture(scope="module")
def server(tmp_path_factory):
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    log = tmp_path_factory.mktemp("server") / "server.log"
    with log.open("w") as output:
        process = subprocess.Popen(
            [
                "target/debug/rosbridge_server_rs",
                "--bind",
                f"127.0.0.1:{port}",
                "--service-timeout",
                "2",
            ],
            stdout=output,
            stderr=output,
        )

        def ready():
            if process.poll() is not None:
                raise AssertionError(log.read_text())
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                    return True
            except OSError:
                return False

        wait_until(ready)
        yield f"ws://127.0.0.1:{port}"
        process.send_signal(signal.SIGINT)
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
            pytest.fail("server did not shut down")
        assert process.returncode == 0, log.read_text()


async def send(ws, **message):
    await ws.send(json.dumps(message))


async def receive(ws, op=None):
    data = await asyncio.wait_for(ws.recv(), 8)
    value = cbor2.loads(data) if isinstance(data, bytes) else json.loads(data)
    if op:
        assert value["op"] == op, value
    return value


def unique(prefix):
    return "/" + prefix + "_" + uuid.uuid4().hex[:8]


def test_ros_to_websocket_and_back(server, ros):
    topic = unique("text")
    received = []
    sub = ros.create_subscription(String, topic, lambda msg: received.append(msg.data), 10)
    pub = ros.create_publisher(String, topic, 10)

    async def run():
        async with websockets.connect(server) as ws:
            await send(ws, op="subscribe", topic=topic, type="std_msgs/String")
            await asyncio.to_thread(wait_until, lambda: pub.get_subscription_count() >= 2)
            pub.publish(String(data="robot → browser 中文"))
            assert (await receive(ws, "publish"))["msg"]["data"] == "robot → browser 中文"
            await send(
                ws,
                op="publish",
                topic=topic,
                type="std_msgs/msg/String",
                msg={"data": "browser → robot"},
            )
            await asyncio.to_thread(wait_until, lambda: "browser → robot" in received)

    asyncio.run(run())
    ros.destroy_subscription(sub)
    ros.destroy_publisher(pub)


@pytest.mark.parametrize("message_type", [BasicTypes, Arrays, BoundedSequences, UnboundedSequences])
def test_runtime_type_conversion(server, ros, message_type):
    topic = unique("dynamic")
    received = []
    sub = ros.create_subscription(message_type, topic, received.append, 10)

    async def run():
        async with websockets.connect(server) as ws:
            typ = f"test_msgs/msg/{message_type.__name__}"
            await send(ws, op="advertise", topic=topic, type=typ)
            await asyncio.to_thread(wait_until, lambda: sub.get_publisher_count() > 0)
            fields = (
                {"int32_value": -123}
                if message_type is BasicTypes
                else {"int32_values": [1, -2, 3]}
            )
            await send(ws, op="publish", topic=topic, msg=fields)
            await asyncio.to_thread(wait_until, lambda: bool(received))
            if message_type is BasicTypes:
                assert received[0].int32_value == -123
            else:
                assert list(received[0].int32_values) == [1, -2, 3]

    try:
        asyncio.run(run())
    finally:
        ros.destroy_subscription(sub)


def test_defaults_header_and_validation(server, ros):
    topic = unique("pose")
    received = []
    sub = ros.create_subscription(PoseStamped, topic, received.append, 10)

    async def run():
        async with websockets.connect(server) as ws:
            await send(ws, op="advertise", topic=topic, type="geometry_msgs/PoseStamped")
            await asyncio.to_thread(wait_until, lambda: sub.get_publisher_count() > 0)
            await send(ws, op="publish", topic=topic, msg={"pose": {"position": {"x": 1.5}}})
            await asyncio.to_thread(wait_until, lambda: bool(received))
            assert received[0].header.stamp.sec > 0
            assert received[0].pose.position.x == 1.5
            assert received[0].pose.position.y == 0
            await send(ws, op="publish", id="bad", topic=topic, msg={"not_a_field": 42})
            error = await receive(ws, "status")
            assert error["id"] == "bad" and error["level"] == "error"

    asyncio.run(run())
    ros.destroy_subscription(sub)


@pytest.mark.parametrize("compression", ["none", "cbor", "cbor-raw", "png"])
def test_encodings(server, ros, compression):
    topic = unique("bytes")
    pub = ros.create_publisher(UInt8MultiArray, topic, 10)

    async def run():
        async with websockets.connect(server) as ws:
            await send(
                ws,
                op="subscribe",
                topic=topic,
                type="std_msgs/UInt8MultiArray",
                compression=compression,
            )
            await asyncio.to_thread(wait_until, lambda: pub.get_subscription_count() > 0)
            pub.publish(UInt8MultiArray(data=[0, 1, 127, 255]))
            value = await receive(ws)
            if compression == "png":
                assert value["op"] == "png"
                rgb = Image.open(io.BytesIO(base64.b64decode(value["data"]))).tobytes()
                value = json.loads(rgb.decode().rstrip("\n"))
            assert value["op"] == "publish"
            if compression == "cbor-raw":
                msg = deserialize_message(value["msg"]["bytes"], UInt8MultiArray)
                assert list(msg.data) == [0, 1, 127, 255]
            elif compression == "cbor":
                assert value["msg"]["data"] == b"\x00\x01\x7f\xff"
            else:
                assert base64.b64decode(value["msg"]["data"]) == b"\x00\x01\x7f\xff"

    asyncio.run(run())
    ros.destroy_publisher(pub)


def test_service_client(server, ros):
    name = unique("add")

    def callback(request, response):
        response.sum = request.a + request.b
        return response

    service = ros.create_service(AddTwoInts, name, callback)

    async def run():
        async with websockets.connect(server) as ws:
            await asyncio.sleep(0.4)
            await send(ws, op="call_service", id="sum", service=name, args=[20, 22])
            result = await receive(ws, "service_response")
            assert result["id"] == "sum" and result["result"] is True, result
            assert result["values"] == {"sum": 42}

    asyncio.run(run())
    ros.destroy_service(service)


def test_websocket_service_server(server, ros):
    name = unique("external")
    client = ros.create_client(AddTwoInts, name)

    async def run():
        async with websockets.connect(server) as ws:
            await send(
                ws, op="advertise_service", service=name, type="example_interfaces/AddTwoInts"
            )
            assert await asyncio.to_thread(client.wait_for_service, timeout_sec=8)
            future = client.call_async(AddTwoInts.Request(a=5, b=7))
            request = await receive(ws, "call_service")
            await send(
                ws,
                op="service_response",
                id=request["id"],
                service=name,
                result=True,
                values={"sum": 12},
            )
            await asyncio.to_thread(wait_until, future.done)
            assert future.result().sum == 12

    asyncio.run(run())
    ros.destroy_client(client)


def test_action_client(server, ros):
    name = unique("fib")

    def execute(goal):
        feedback = Fibonacci.Feedback(sequence=[0, 1])
        goal.publish_feedback(feedback)
        time.sleep(0.1)
        goal.succeed()
        return Fibonacci.Result(sequence=[0, 1, 1, 2, 3])

    action = ActionServer(ros, Fibonacci, name, execute)

    async def run():
        async with websockets.connect(server) as ws:
            await asyncio.sleep(0.5)
            await send(
                ws,
                op="send_action_goal",
                id="goal",
                action=name,
                action_type="test_msgs/Fibonacci",
                args={"order": 5},
                feedback=True,
            )
            messages = []
            while not messages or messages[-1]["op"] != "action_result":
                messages.append(await receive(ws))
            result = messages[-1]
            assert result["id"] == "goal" and result["status"] == 4 and result["result"], result
            assert result["values"]["sequence"] == [0, 1, 1, 2, 3]
            assert any(m["op"] == "action_feedback" for m in messages)

    asyncio.run(run())
    action.destroy()


def test_websocket_action_server_and_cancel(server, ros):
    name = unique("external_action")
    client = ActionClient(ros, Fibonacci, name)

    async def run():
        async with websockets.connect(server) as ws:
            await send(ws, op="advertise_action", action=name, type="test_msgs/Fibonacci")
            assert await asyncio.to_thread(client.wait_for_server, timeout_sec=8)
            feedback = []
            future = client.send_goal_async(
                Fibonacci.Goal(order=10),
                feedback_callback=lambda m: feedback.append(m.feedback.sequence),
            )
            request = await receive(ws, "send_action_goal")
            await asyncio.to_thread(wait_until, future.done)
            handle = future.result()
            assert handle.accepted
            result = handle.get_result_async()
            await send(
                ws, op="action_feedback", id=request["id"], action=name, values={"sequence": [0, 1]}
            )
            await asyncio.to_thread(wait_until, lambda: bool(feedback))
            cancel = handle.cancel_goal_async()
            cancellation = await receive(ws, "cancel_action_goal")
            assert cancellation["id"] == request["id"]
            await asyncio.to_thread(wait_until, cancel.done)
            assert cancel.result().return_code == 0
            await send(
                ws,
                op="action_result",
                id=request["id"],
                action=name,
                values={"sequence": [0, 1]},
                status=5,
                result=True,
            )
            await asyncio.to_thread(wait_until, result.done)
            assert result.result().status == 5
            assert list(result.result().result.sequence) == [0, 1]

    asyncio.run(run())
    client.destroy()


def test_two_clients_share_publisher_and_disconnect(server, ros):
    topic = unique("shared")

    async def run():
        async with websockets.connect(server) as first, websockets.connect(server) as second:
            for ws in (first, second):
                await send(ws, op="advertise", topic=topic, type="std_msgs/String")
            await asyncio.to_thread(wait_until, lambda: ros.count_publishers(topic) == 1)
            await first.close()
            await asyncio.sleep(0.1)
            assert ros.count_publishers(topic) == 1
        await asyncio.to_thread(wait_until, lambda: ros.count_publishers(topic) == 0)

    asyncio.run(run())


def test_action_result_is_validated_before_get_result(server, ros):
    name = unique("early_result")
    client = ActionClient(ros, Fibonacci, name)

    async def run():
        async with websockets.connect(server) as ws:
            await send(ws, op="advertise_action", action=name, type="test_msgs/Fibonacci")
            assert await asyncio.to_thread(client.wait_for_server, timeout_sec=8)
            future = client.send_goal_async(Fibonacci.Goal(order=3))
            request = await receive(ws, "send_action_goal")
            await asyncio.to_thread(wait_until, future.done)
            await send(
                ws,
                op="action_result",
                id=request["id"],
                action=name,
                values={"sequence": "invalid"},
                status=4,
                result=True,
            )
            error = await receive(ws, "status")
            assert error["level"] == "error"
            await send(
                ws,
                op="action_result",
                id=request["id"],
                action=name,
                values={"sequence": [0, 1, 1]},
                status=4,
                result=True,
            )
            result = future.result().get_result_async()
            await asyncio.to_thread(wait_until, result.done)
            assert result.result().status == 4
            assert list(result.result().result.sequence) == [0, 1, 1]

    try:
        asyncio.run(run())
    finally:
        client.destroy()
