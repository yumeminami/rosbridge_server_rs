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


"""Compare every native rosapi endpoint with the installed Python rosapi."""
import asyncio
import json
import os
import signal
import subprocess
import time

import pytest
import rclpy
from rclpy.action import ActionServer
from rclpy.parameter import Parameter
from rosidl_runtime_py.convert import message_to_ordereddict
from rosidl_runtime_py.set_message import set_message_fields
from rosapi_msgs import srv
from std_msgs.msg import String
from example_interfaces.srv import AddTwoInts
from test_msgs.action import Fibonacci

from test_websocket import ros, server, send, receive, wait_until
import websockets

CASES = [
    ("topics", "Topics", {}),
    ("interfaces", "Interfaces", {}),
    ("topics_for_type", "TopicsForType", {"type": "std_msgs/msg/String"}),
    ("topics_and_raw_types", "TopicsAndRawTypes", {}),
    ("services", "Services", {}),
    ("services_for_type", "ServicesForType", {"type": "example_interfaces/srv/AddTwoInts"}),
    ("nodes", "Nodes", {}),
    ("node_details", "NodeDetails", {}),
    ("action_servers", "GetActionServers", {}),
    ("action_type", "ActionType", {"action": "/rosapi_test_action"}),
    ("topic_type", "TopicType", {"topic": "/rosapi_test_topic"}),
    ("service_type", "ServiceType", {"service": "/rosapi_test_service"}),
    ("publishers", "Publishers", {"topic": "/rosapi_test_topic"}),
    ("subscribers", "Subscribers", {"topic": "/rosapi_test_topic"}),
    ("service_providers", "ServiceProviders", {"service": "example_interfaces/srv/AddTwoInts"}),
    ("service_node", "ServiceNode", {"service": "/rosapi_test_service"}),
    ("message_details", "MessageDetails", {"type": "geometry_msgs/msg/Pose"}),
    (
        "service_request_details",
        "ServiceRequestDetails",
        {"type": "example_interfaces/srv/AddTwoInts"},
    ),
    (
        "service_response_details",
        "ServiceResponseDetails",
        {"type": "example_interfaces/srv/AddTwoInts"},
    ),
    ("action_goal_details", "ActionGoalDetails", {"type": "test_msgs/action/Fibonacci"}),
    ("action_result_details", "ActionResultDetails", {"type": "test_msgs/action/Fibonacci"}),
    ("action_feedback_details", "ActionFeedbackDetails", {"type": "test_msgs/action/Fibonacci"}),
    ("get_param", "GetParam", {}),
    ("set_param", "SetParam", {}),
    ("has_param", "HasParam", {}),
    ("delete_param", "DeleteParam", {}),
    ("get_param_names", "GetParamNames", {}),
    ("get_time", "GetTime", {}),
    ("get_ros_version", "GetROSVersion", {}),
]


@pytest.fixture(scope="module")
def api(server, ros, tmp_path_factory):
    pub = ros.create_publisher(String, "/rosapi_test_topic", 10)
    sub = ros.create_subscription(String, "/rosapi_test_topic", lambda _: None, 10)
    service = ros.create_service(AddTwoInts, "/rosapi_test_service", lambda req, res: res)
    action = ActionServer(ros, Fibonacci, "/rosapi_test_action", lambda _: Fibonacci.Result())
    ros.declare_parameter("rosapi_test", 42)
    log = tmp_path_factory.mktemp("rosapi") / "python.log"
    with log.open("w") as output:
        baseline = subprocess.Popen(
            ["/opt/ros/jazzy/lib/rosapi/rosapi_node", "--ros-args", "-r", "__ns:=/reference"],
            stdout=output,
            stderr=output,
        )
        clients = {}
        try:
            for name, typ, _ in CASES:
                cls = getattr(srv, typ)
                native = ros.create_client(cls, "/rosapi/" + name)
                python = ros.create_client(cls, "/reference/rosapi/" + name)
                assert native.wait_for_service(timeout_sec=10), name
                assert python.wait_for_service(timeout_sec=10), log.read_text()
                clients[name] = (cls, native, python)
            time.sleep(0.5)
            yield server, ros, clients
        finally:
            for _, native, python in clients.values():
                ros.destroy_client(native)
                ros.destroy_client(python)
            print(log.read_text())
            baseline.send_signal(signal.SIGINT)
            baseline.wait(timeout=10)
            action.destroy()
            ros.destroy_service(service)
            ros.destroy_subscription(sub)
            ros.destroy_publisher(pub)


def call(client, cls, args):
    request = cls.Request()
    set_message_fields(request, args)
    future = client.call_async(request)
    wait_until(future.done, timeout=10)
    return dict(message_to_ordereddict(future.result()))


def normalize_details(response):
    # Python rosapi mistakenly includes runtime properties and object addresses as
    # constants. Compare the stable schema; real constants have a separate test.
    for typedef in response["typedefs"]:
        typedef.pop("constnames", None)
        typedef.pop("constvalues", None)
    return response


@pytest.mark.parametrize("name,typ,args", CASES, ids=[c[0] for c in CASES])
def test_rosapi_matches_python(api, name, typ, args):
    _, ros, clients = api
    cls, native, python = clients[name]
    args = args.copy()
    if name == "node_details":
        args["node"] = ros.get_fully_qualified_name()
    if name in ("get_param", "has_param", "set_param", "delete_param"):
        args["name"] = ros.get_fully_qualified_name() + ":rosapi_test"
        if name == "set_param":
            args["value"] = "42"
        if name == "delete_param":
            # Delete an undeclared parameter, avoiding a fixture state dependency.
            args["name"] += "_absent"
    # Jazzy rosapi 2.7.0 crashes here by calling a removed rclpy Node method.
    # Verify the action fixture directly instead of treating that crash as a contract.
    if name == "action_type":
        assert call(native, cls, args) == {"type": "test_msgs/action/Fibonacci"}
        return
    expected = call(python, cls, args)
    actual = call(native, cls, args)
    if name.endswith("_details") and name != "node_details":
        assert normalize_details(actual) == normalize_details(expected)
    elif name == "get_time":
        a, b = actual["time"], expected["time"]
        assert abs((a["sec"] - b["sec"]) + (a["nanosec"] - b["nanosec"]) / 1e9) < 2
    elif name == "get_param_names":
        # Each implementation excludes itself. The reference process is a remote
        # node to Rust, so remove only its own configuration from the comparison.
        actual["names"] = [n for n in actual["names"] if not n.startswith("/reference/rosapi:")]
        assert sorted(actual["names"]) == sorted(expected["names"])
    elif name in (
        "nodes",
        "interfaces",
        "services",
        "services_for_type",
        "topics_for_type",
        "publishers",
        "subscribers",
        "service_providers",
        "action_servers",
        "get_param_names",
    ):
        key = next(iter(actual))
        assert sorted(actual[key]) == sorted(expected[key])
    elif name == "node_details":
        assert {k: sorted(v) for k, v in actual.items()} == {
            k: sorted(v) for k, v in expected.items()
        }
    else:
        assert actual == expected


@pytest.mark.parametrize(
    "value", [True, 123, 1.25, "hello", [True, False], [1, 2], [1.0, 2.5], ["a", "b"]]
)
def test_parameter_roundtrip(api, value):
    _, ros, clients = api
    name = "roundtrip_" + str(abs(hash(json.dumps(value))))
    ros.declare_parameter(name, value)
    full_name = ros.get_fully_qualified_name() + ":" + name
    try:
        cls, native, _ = clients["set_param"]
        assert call(native, cls, {"name": full_name, "value": json.dumps(value)})["successful"]
        cls, native, _ = clients["get_param"]
        response = call(native, cls, {"name": full_name})
        assert response["successful"]
        assert json.loads(response["value"]) == value
    finally:
        ros.undeclare_parameter(name)


def test_missing_parameter_default(api):
    _, ros, clients = api
    cls, native, python = clients["get_param"]
    args = {"name": "/missing_rosapi_test_node:value", "default_value": '"fallback"'}
    actual = call(native, cls, args)
    assert actual["successful"] is False
    assert json.loads(actual["value"]) == "fallback"


def test_native_rosapi_websocket(api):
    url, _, _ = api

    async def run():
        async with websockets.connect(url) as ws:
            await send(
                ws,
                op="call_service",
                service="/rosapi/topics_and_raw_types",
                type="rosapi/TopicsAndRawTypes",
                args={},
            )
            response = await receive(ws, "service_response")
            assert response["result"]
            assert "/rosapi_test_topic" in response["values"]["topics"]

    asyncio.run(run())


@pytest.mark.parametrize(
    "typ", ["std_msgs/msg/Header", "test_msgs/msg/Arrays", "test_msgs/msg/Defaults"]
)
def test_message_details_schema(api, typ):
    _, _, clients = api
    cls, native, python = clients["message_details"]
    actual = normalize_details(call(native, cls, {"type": typ}))
    expected = normalize_details(call(python, cls, {"type": typ}))
    assert actual == expected


def test_real_interface_constants(api):
    _, _, clients = api
    cls, native, _ = clients["message_details"]
    detail = call(native, cls, {"type": "sensor_msgs/msg/NavSatStatus"})["typedefs"][0]
    constants = dict(zip(detail["constnames"], detail["constvalues"]))
    assert constants["STATUS_NO_FIX"] == "-1"
    assert constants["STATUS_FIX"] == "0"
    assert "SLOT_TYPES" not in constants
    assert "status" not in constants


def test_invalid_parameter_request_does_not_stop_server(api):
    _, ros, clients = api
    cls, native, _ = clients["set_param"]
    assert not call(native, cls, {"name": "invalid", "value": "1"})["successful"]
    assert not call(
        native, cls, {"name": ros.get_fully_qualified_name() + ":rosapi_test", "value": "{"}
    )["successful"]
    cls, native, _ = clients["get_ros_version"]
    assert call(native, cls, {})["version"] == 2


def test_delete_existing_dynamic_parameter(api):
    from rcl_interfaces.msg import ParameterDescriptor

    _, ros, clients = api
    name = "delete_dynamic"
    ros.declare_parameter(name, 42, ParameterDescriptor(dynamic_typing=True))
    full_name = ros.get_fully_qualified_name() + ":" + name
    cls, native, _ = clients["delete_param"]
    assert call(native, cls, {"name": full_name})["successful"]
    cls, native, _ = clients["has_param"]
    assert call(native, cls, {"name": full_name})["exists"] is False


def test_missing_interface_returns_empty_details(api):
    _, _, clients = api
    for name in (
        "message_details",
        "service_request_details",
        "service_response_details",
        "action_goal_details",
        "action_result_details",
        "action_feedback_details",
    ):
        cls, native, _ = clients[name]
        assert call(native, cls, {"type": "missing_package/NoSuchType"}) == {"typedefs": []}


def test_rosapi_filters_and_parameter_timeout(ros, tmp_path):
    import socket

    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    allowed = ros.create_publisher(String, "/allowed/one", 10)
    denied = ros.create_publisher(String, "/denied/two", 10)
    remaps = []
    for name, _, _ in CASES:
        remaps += ["-r", f"/rosapi/{name}:=/filtered/rosapi/{name}"]
    log = tmp_path / "filtered.log"
    with log.open("w") as output:
        process = subprocess.Popen(
            [
                "target/debug/rosbridge_server_rs",
                "--bind",
                f"127.0.0.1:{port}",
                "--node-name",
                "filtered_bridge",
                "--",
                "--ros-args",
                "-p",
                "topics_glob:=[/allowed/*]",
                "-p",
                "services_glob:=[/rosapi_test_service,/slow_rosapi/*]",
                "-p",
                "params_glob:=[rosapi_*]",
                "-p",
                "params_timeout:=0.1",
                *remaps,
            ],
            stdout=output,
            stderr=output,
        )
        clients = []
        try:

            def client(name, cls):
                result = ros.create_client(cls, "/filtered/rosapi/" + name)
                clients.append(result)
                assert result.wait_for_service(timeout_sec=10), log.read_text()
                return result

            topics = call(client("topics", srv.Topics), srv.Topics, {})
            assert topics == {"topics": ["/allowed/one"], "types": ["std_msgs/msg/String"]}
            get_param_client = client("get_param", srv.GetParam)
            denied_param = call(
                get_param_client,
                srv.GetParam,
                {"name": ros.get_fully_qualified_name() + ":blocked", "default_value": "42"},
            )
            assert not denied_param["successful"]
            assert json.loads(denied_param["value"]) == 42

            from rcl_interfaces.srv import GetParameters

            def slow_response(request, response):
                time.sleep(0.5)
                return response

            service = ros.create_service(
                GetParameters, "/slow_rosapi/get_parameters", slow_response
            )
            try:
                service_type = client("service_type", srv.ServiceType)
                wait_until(
                    lambda: call(
                        service_type, srv.ServiceType, {"service": "/slow_rosapi/get_parameters"}
                    )["type"]
                    == "rcl_interfaces/srv/GetParameters"
                )
                result = call(
                    get_param_client,
                    srv.GetParam,
                    {"name": "/slow_rosapi:rosapi_slow", "default_value": "null"},
                )
                assert result["successful"] is False
                assert result["reason"] == "Timeout occurred"
            finally:
                ros.destroy_service(service)
        finally:
            for c in clients:
                ros.destroy_client(c)
            process.send_signal(signal.SIGINT)
            process.wait(timeout=10)
            ros.destroy_publisher(allowed)
            ros.destroy_publisher(denied)
