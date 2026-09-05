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

"""Render measured trial summaries as PNG and SVG."""
import json
from pathlib import Path
import statistics
import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

out = Path("benchmarks/results")
rows = json.loads((out / "raw.json").read_text())
# All figures are derived from the saved trials; no benchmark is rerun here.
plt.rcParams.update({"font.family": "DejaVu Sans", "svg.fonttype": "none"})
sizes = sorted({row["payload_bytes"] for row in rows})
labels = {64: "64 B payload", 1024: "1 KiB payload", 65536: "64 KiB payload"}


def measurements(kind, size, window, metric):
    values = [
        row[metric]
        for row in rows
        if row["server"] == kind and row["payload_bytes"] == size and row["window"] == window
    ]
    return statistics.mean(values), min(values), max(values)


def draw_throughput(dark=False):
    background = "#16151e" if dark else "#faf9fd"
    foreground = "#f0edf8" if dark else "#262133"
    muted = "#aaa3bc" if dark else "#756d85"
    grid = "#302b3e" if dark else "#eae5f1"
    colors = {"rust": "#bfa1ff" if dark else "#8250df", "python": "#696176" if dark else "#c5bdd1"}
    fig, axes = plt.subplots(3, 1, figsize=(10, 5.7))
    fig.patch.set_facecolor(background)
    fig.subplots_adjust(left=0.20, right=0.87, top=0.77, bottom=0.16, hspace=0.95)
    fig.text(0.065, 0.915, "WebSocket throughput", color=foreground, fontsize=23, weight="bold")
    fig.text(
        0.065, 0.858, "Messages delivered per second  ·  higher is better", color=muted, fontsize=11
    )
    for ax, size in zip(axes, sizes):
        ax.set_facecolor(background)
        python_rate = measurements("python", size, 32, "messages_s")[0]
        rust_rate = measurements("rust", size, 32, "messages_s")[0]
        ax.text(
            -0.20,
            1.20,
            labels[size],
            transform=ax.transAxes,
            color=foreground,
            fontsize=11,
            weight="bold",
        )
        ax.text(
            1.13,
            1.20,
            f"{rust_rate/python_rate:.1f}× throughput",
            transform=ax.transAxes,
            color=colors["rust"],
            fontsize=11,
            weight="bold",
            ha="right",
        )
        for position, kind in [(1, "rust"), (0, "python")]:
            mean, low, high = measurements(kind, size, 32, "messages_s")
            ax.barh(position, mean, height=0.55, color=colors[kind], zorder=3)
            ax.errorbar(
                mean,
                position,
                xerr=[[mean - low], [high - mean]],
                fmt="none",
                ecolor=foreground,
                elinewidth=1,
                capsize=2,
                zorder=4,
            )
            ax.text(
                high + 800,
                position,
                f"{mean:,.0f}",
                va="center",
                color=foreground,
                fontsize=11,
                weight="bold" if kind == "rust" else "normal",
            )
        ax.set_yticks([1, 0], ["Rust", "Python"], color=muted, fontsize=10)
        ax.set_xlim(0, 43000)
        ax.set_ylim(-0.6, 1.6)
        ax.set_xticks([0, 10000, 20000, 30000, 40000])
        ax.set_xticklabels(
            ["0", "10k", "20k", "30k", "40k"] if size == sizes[-1] else [], color=muted, fontsize=9
        )
        ax.tick_params(axis="both", length=0, pad=8)
        ax.grid(axis="x", color=grid, linewidth=0.7, zorder=0)
        for spine in ax.spines.values():
            spine.set_visible(False)
    fig.text(
        0.065,
        0.060,
        "ROS 2 Jazzy · Linux ARM64 · localhost JSON · 32 messages in flight",
        fontsize=9,
        color=muted,
    )
    fig.text(
        0.065,
        0.025,
        "Rust release vs Python rosbridge 2.7.0 · 3 × 2 s trials · whiskers show trial range",
        fontsize=8.5,
        color=muted,
    )
    suffix = "-dark" if dark else ""
    fig.savefig(out / f"comparison{suffix}.svg", facecolor=background)
    fig.savefig(out / f"comparison{suffix}.png", dpi=180, facecolor=background)
    plt.close(fig)


def draw_details():
    fig, axes = plt.subplots(1, 3, figsize=(12, 4.8))
    fig.patch.set_facecolor("#faf9fd")
    fig.subplots_adjust(left=0.09, right=0.97, top=0.75, bottom=0.24, wspace=0.48)
    fig.text(0.055, 0.91, "Latency & resource usage", fontsize=22, weight="bold", color="#262133")
    fig.text(
        0.055,
        0.84,
        "Same measured trials · Rust in purple, Python in gray",
        fontsize=11,
        color="#756d85",
    )
    metrics = [
        ("p95_ms", "p95 roundtrip · ms", 1),
        ("cpu_percent", "Server CPU · % of one core", 32),
        ("rss_mib", "Server memory · MiB", 32),
    ]
    for ax, (metric, title, window) in zip(axes, metrics):
        ax.set_facecolor("#faf9fd")
        for index, size in enumerate(sizes):
            for offset, kind, color in [(0.16, "rust", "#8250df"), (-0.16, "python", "#c5bdd1")]:
                mean, low, high = measurements(kind, size, window, metric)
                position = 2 - index + offset
                ax.barh(position, mean, 0.25, color=color, zorder=3)
                ax.errorbar(
                    mean,
                    position,
                    xerr=[[mean - low], [high - mean]],
                    fmt="none",
                    ecolor="#40354d",
                    capsize=2,
                    elinewidth=0.8,
                )
        ax.set_yticks([2, 1, 0], ["64 B", "1 KiB", "64 KiB"], color="#756d85", fontsize=10)
        ax.set_title(title, loc="left", fontsize=11, color="#262133", pad=15)
        ax.set_xlim(left=0)
        ax.tick_params(axis="both", length=0, labelcolor="#756d85", labelsize=9)
        ax.grid(axis="x", color="#eae5f1", linewidth=0.7)
        for spine in ax.spines.values():
            spine.set_visible(False)
    fig.text(
        0.055,
        0.105,
        "Latency: 1 message in flight. CPU and memory: 32 in flight; delivered throughput differs.",
        fontsize=9,
        color="#756d85",
    )
    fig.text(
        0.055,
        0.055,
        "Means of 3 trials; whiskers = range. CPU excludes client. Memory = larger before/after RSS sample.",
        fontsize=9,
        color="#756d85",
    )
    for extension in ("png", "svg"):
        fig.savefig(out / f"details.{extension}", dpi=180, facecolor=fig.get_facecolor())
    plt.close(fig)


draw_throughput()
draw_throughput(dark=True)
draw_details()

# Keep a compact table alongside the full per-message measurements.
import csv

summary = []
for size in sorted({r["payload_bytes"] for r in rows}):
    for window in (1, 32):
        for kind in ("python", "rust"):
            trials = [
                r
                for r in rows
                if r["server"] == kind and r["payload_bytes"] == size and r["window"] == window
            ]
            row = dict(server=kind, payload_bytes=size, window=window)
            for metric in ("messages_s", "median_ms", "p95_ms", "p99_ms", "cpu_percent", "rss_mib"):
                row[metric] = statistics.mean(r[metric] for r in trials)
            summary.append(row)
with (out / "summary.csv").open("w") as stream:
    writer = csv.DictWriter(stream, fieldnames=summary[0].keys())
    writer.writeheader()
    writer.writerows(summary)
report = [
    "# WebSocket benchmark results",
    "",
    "Linux ARM64 / ROS 2 Jazzy, Rust release build versus Python rosbridge 2.7.0. Three trials per configuration; values below are means of trial summaries. See [method](../README.md) and [environment](environment.json).",
    "",
    "![Throughput comparison](comparison.svg)",
    "",
    "![Latency and resource usage](details.svg)",
    "",
    "| Payload | Python p95 RTT, 1 in flight | Rust p95 RTT, 1 in flight | Python msg/s, 32 in flight | Rust msg/s, 32 in flight | Throughput ratio |",
    "| --- | ---: | ---: | ---: | ---: | ---: |",
]
for size in sorted({r["payload_bytes"] for r in rows}):

    def value(kind, window, metric):
        return next(
            r[metric]
            for r in summary
            if r["server"] == kind and r["window"] == window and r["payload_bytes"] == size
        )

    python_rate = value("python", 32, "messages_s")
    rust_rate = value("rust", 32, "messages_s")
    report.append(
        f'| {size:,} B | {value("python",1,"p95_ms"):.3f} ms | {value("rust",1,"p95_ms"):.3f} ms | {python_rate:,.0f} | {rust_rate:,.0f} | {rust_rate/python_rate:.2f}× |'
    )
report += [
    "",
    "These are end-to-end localhost measurements, including the benchmark client, ROS transport and Docker scheduling. They do not establish maximum capacity or production performance. Error bars show trial range, not confidence intervals. CPU excludes the client; RSS is sampled before and after each trial.",
    "",
    "See [compatibility coverage](../README.md#compatibility-results) and the [per-case upstream results](parity.json) for the current correctness checks. The measurements above predate subsequent protocol-compatibility fixes; the measured binary hash is retained in environment.json.",
]
(out / "README.md").write_text("\n".join(report) + "\n")
