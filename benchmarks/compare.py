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

"""Matched JSON topic roundtrip benchmark; one server runs at a time."""
import asyncio
import hashlib
import os
import json
import platform
from pathlib import Path
import statistics
import subprocess
import time

import psutil
import websockets
from server import UPSTREAM, server

OUT = Path("benchmarks/results")
PAYLOADS = [64, 1024, 65536]
WINDOWS = [1, 32]
REPEATS = 3
SECONDS = 2.0


def percentile(values, fraction):
    return sorted(values)[min(len(values) - 1, int((len(values) - 1) * fraction))]


async def trial(port, process, size, window):
    async with websockets.connect(
        f"ws://127.0.0.1:{port}", compression=None, max_size=2**24, max_queue=256
    ) as ws:
        topic = "/benchmark_echo"
        qos = dict(history="keep_last", depth=256, reliability="reliable", durability="volatile")
        await ws.send(
            json.dumps(dict(op="advertise", topic=topic, type="std_msgs/msg/String", qos=qos))
        )
        await ws.send(
            json.dumps(dict(op="subscribe", topic=topic, type="std_msgs/msg/String", qos=qos))
        )
        await asyncio.sleep(0.5)

        def command(sequence):
            return json.dumps(
                dict(
                    op="publish", topic=topic, msg=dict(data=f"{sequence:016d}" + "x" * (size - 16))
                ),
                separators=(",", ":"),
            )

        async def receive():
            value = json.loads(await asyncio.wait_for(ws.recv(), 10))
            assert value["op"] == "publish", value
            data = value["msg"]["data"]
            assert len(data) == size and data[16:] == "x" * (size - 16)
            return int(data[:16])

        for sequence in range(20):
            await ws.send(command(sequence))
            assert await receive() == sequence
        native = psutil.Process(process.pid)
        cpu_before = sum(native.cpu_times()[:2])
        rss = native.memory_info().rss
        started = time.perf_counter()
        deadline = started + SECONDS
        pending = {}
        sequence = 20
        latencies = []

        async def publish():
            nonlocal sequence
            value = command(sequence)
            pending[sequence] = time.perf_counter()
            await ws.send(value)
            sequence += 1

        for _ in range(window):
            await publish()
        while pending:
            received = await receive()
            sent_at = pending.pop(received)
            latencies.append((time.perf_counter() - sent_at) * 1000)
            if time.perf_counter() < deadline:
                await publish()
        elapsed = time.perf_counter() - started
        cpu_seconds = sum(native.cpu_times()[:2]) - cpu_before
        rss = max(rss, native.memory_info().rss)
        return dict(
            messages=len(latencies),
            elapsed_s=elapsed,
            messages_s=len(latencies) / elapsed,
            median_ms=statistics.median(latencies),
            p95_ms=percentile(latencies, 0.95),
            p99_ms=percentile(latencies, 0.99),
            cpu_percent=100 * cpu_seconds / elapsed,
            rss_mib=rss / 2**20,
            latency_ms=latencies,
        )


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    rows = []
    for repeat in range(REPEATS):
        for size in PAYLOADS:
            for window in WINDOWS:
                # Alternate order to reduce systematic first-server bias.
                for kind in ("python", "rust") if repeat % 2 == 0 else ("rust", "python"):
                    label = f"{kind}-{size}-{window}-{repeat}"
                    with server(kind, OUT / (label + ".server.log")) as (process, port):
                        row = asyncio.run(trial(port, process, size, window))
                    row.update(server=kind, payload_bytes=size, window=window, repeat=repeat)
                    rows.append(row)
                    (OUT / "raw.json").write_text(json.dumps(rows))
                    print(
                        label,
                        f'{row["messages_s"]:.1f} msg/s, p95 {row["p95_ms"]:.3f} ms',
                        flush=True,
                    )
    metadata = dict(
        platform=platform.platform(),
        cpu_count=psutil.cpu_count(),
        cpu_affinity=psutil.Process().cpu_affinity(),
        rust_binary_sha256=hashlib.sha256(
            Path("target/release/rosbridge_server_rs").read_bytes()
        ).hexdigest(),
        python_package=subprocess.check_output(
            ["dpkg-query", "-W", "ros-jazzy-rosbridge-server"], text=True
        ).strip(),
        machine=platform.machine(),
        python=platform.python_version(),
        rust=subprocess.check_output(["rustc", "--version"], text=True).strip(),
        upstream_commit=subprocess.check_output(
            [
                "git",
                "-c",
                "safe.directory=" + str(UPSTREAM),
                "-C",
                str(UPSTREAM),
                "rev-parse",
                "HEAD",
            ],
            text=True,
        ).strip(),
        repeats=REPEATS,
        seconds=SECONDS,
        payloads=PAYLOADS,
        windows=WINDOWS,
        encoding="JSON, WebSocket compression disabled",
        qos="reliable volatile keep_last depth256",
        clock="time.perf_counter",
        path="WebSocket publish -> RCL/RMW topic -> WebSocket subscription",
        python_server="ros-jazzy-rosbridge-server 2.7.0 (installed package)",
        ros_distro="jazzy",
        rmw=os.environ.get("RMW_IMPLEMENTATION", "default (rmw_fastrtps_cpp in this image)"),
    )
    (OUT / "environment.json").write_text(json.dumps(metadata, indent=2))


if __name__ == "__main__":
    main()
