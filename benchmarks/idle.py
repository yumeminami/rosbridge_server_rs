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


"""Same-host CPU comparison including rosapi, before and after unsubscribe."""
import argparse
import asyncio
import contextlib
import json
from pathlib import Path
import signal
import socket
import subprocess
import time

import psutil
import websockets


@contextlib.contextmanager
def launch(kind, directory):
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    commands = (
        [["target/release/rosbridge_server_rs", "--bind", f"127.0.0.1:{port}"]]
        if kind == "rust"
        else [
            ["/opt/ros/jazzy/lib/rosapi/rosapi_node"],
            [
                "/opt/ros/jazzy/lib/rosbridge_server/rosbridge_websocket",
                "--ros-args",
                "-p",
                f"port:={port}",
                "-p",
                "unregister_timeout:=0.5",
            ],
        ]
    )
    processes, logs = [], []
    try:
        for index, command in enumerate(commands):
            log = (directory / f"{kind}-{index}.log").open("w")
            logs.append(log)
            processes.append(subprocess.Popen(command, stdout=log, stderr=log))
        deadline = time.monotonic() + 15
        while True:
            if any(p.poll() is not None for p in processes):
                raise RuntimeError(f"{kind} startup failed; inspect {directory}")
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                    break
            except OSError:
                if time.monotonic() > deadline:
                    raise TimeoutError("WebSocket startup")
                time.sleep(0.05)
        yield processes, f"ws://127.0.0.1:{port}"
    finally:
        for process in reversed(processes):
            if process.poll() is None:
                process.send_signal(signal.SIGINT)
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
        for log in logs:
            log.close()


def usage(processes):
    # Count both Python processes, or the single Rust process. The ROS CLI daemon
    # and other container processes are deliberately outside this measurement.
    total_cpu, total_rss = 0.0, 0
    for process in processes:
        if process.poll() is not None:
            raise RuntimeError("server exited during CPU measurement")
        p = psutil.Process(process.pid)
        cpu = p.cpu_times()
        total_cpu += cpu.user + cpu.system
        total_rss += p.memory_info().rss
    return total_cpu, total_rss


async def measure(processes, seconds):
    start_cpu, _ = usage(processes)
    start = time.monotonic()
    await asyncio.sleep(seconds)
    end_cpu, rss = usage(processes)
    return dict(cpu_percent=100 * (end_cpu - start_cpu) / (time.monotonic() - start), rss_bytes=rss)


async def run(args):
    args.output.mkdir(parents=True, exist_ok=True)
    rows = []
    for trial in range(args.trials):
        for kind in ("python", "rust") if trial % 2 == 0 else ("rust", "python"):
            directory = args.output / f"{trial}-{kind}"
            directory.mkdir(exist_ok=True)
            with launch(kind, directory) as (processes, url):
                await asyncio.sleep(5)  # DDS discovery and initial rosapi setup.
                states = ["disconnected", "connected", "unsubscribed"]
                ws = None
                try:
                    for state in states:
                        if state == "connected":
                            ws = await websockets.connect(url)
                        elif state == "unsubscribed":
                            await ws.send(
                                json.dumps(
                                    dict(
                                        op="subscribe",
                                        topic="/rosbridge_idle_probe",
                                        type="std_msgs/msg/String",
                                    )
                                )
                            )
                            await asyncio.sleep(2)
                            await ws.send(
                                json.dumps(dict(op="unsubscribe", topic="/rosbridge_idle_probe"))
                            )
                        await asyncio.sleep(2)
                        row = dict(
                            server=kind,
                            state=state,
                            trial=trial,
                            **await measure(processes, args.seconds),
                        )
                        rows.append(row)
                        (args.output / "cpu.json").write_text(json.dumps(rows, indent=2))
                        print(json.dumps(row), flush=True)
                finally:
                    if ws:
                        await ws.close()
    metadata = dict(
        seconds=args.seconds,
        trials=args.trials,
        machine=__import__("platform").machine(),
        ros_distro=__import__("os").environ.get("ROS_DISTRO"),
        rmw=__import__("os").environ.get("RMW_IMPLEMENTATION"),
        scope="server plus rosapi; excludes ROS CLI daemon",
        subscription_topic="/rosbridge_idle_probe",
        publisher_count=0,
    )
    (args.output / "metadata.json").write_text(json.dumps(metadata, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seconds", type=float, default=10)
    parser.add_argument("--trials", type=int, default=3)
    parser.add_argument("--output", type=Path, default=Path("benchmarks/results/idle"))
    asyncio.run(run(parser.parse_args()))
