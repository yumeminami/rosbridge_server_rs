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


"""Plot every idle benchmark trial, with median markers."""
import json
from pathlib import Path
from statistics import median

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.ticker import FixedLocator, FixedFormatter

ROOT = Path(__file__).parent / "results" / "idle"
rows = json.loads((ROOT / "cpu.json").read_text())
states = ["disconnected", "connected", "unsubscribed"]
labels = ["Disconnected", "Connected · no subscriptions", "After unsubscribe"]
for theme in ("light", "dark"):
    bg, fg, grid = (
        ("#ffffff", "#253047", "#e7eaf0") if theme == "light" else ("#111827", "#e5e7eb", "#283244")
    )
    colors = {"python": "#8594a7", "rust": "#8b5cf6"}
    with plt.rc_context({"font.family": "DejaVu Sans", "font.size": 10, "svg.fonttype": "none"}):
        fig, axes = plt.subplots(1, 2, figsize=(11, 3.9), gridspec_kw={"width_ratios": [1.25, 1]})
        fig.patch.set_facecolor(bg)
        for ax, key, divisor in zip(axes, ["cpu_percent", "rss_bytes"], [1, 2**20]):
            ax.set_facecolor(bg)
            ax.set_axisbelow(True)
            ax.grid(axis="x", color=grid, linewidth=0.8)
            ax.set_ylim(-0.5, 2.5)
            ax.invert_yaxis()
            ax.set_yticks(range(3), labels if ax == axes[0] else [""] * 3, color=fg)
            ax.tick_params(axis="both", length=0, colors=fg, pad=8)
            for spine in ax.spines.values():
                spine.set_visible(False)
            for index, state in enumerate(states):
                for kind, offset in [("python", -0.12), ("rust", 0.12)]:
                    values = [
                        r[key] / divisor
                        for r in rows
                        if r["state"] == state and r["server"] == kind
                    ]
                    ax.scatter(
                        values,
                        [index + offset] * len(values),
                        s=27,
                        color=colors[kind],
                        alpha=0.55,
                        zorder=3,
                    )
                    ax.plot(
                        median(values),
                        index + offset,
                        marker="|",
                        markersize=15,
                        markeredgewidth=2.5,
                        color=colors[kind],
                        zorder=4,
                    )
        axes[0].set_xscale("log")
        axes[0].set_xlim(0.2, 8)
        axes[0].xaxis.set_major_locator(FixedLocator([0.2, 0.5, 1, 2, 5]))
        axes[0].xaxis.set_major_formatter(FixedFormatter(["0.2", "0.5", "1", "2", "5"]))
        axes[0].minorticks_off()
        axes[0].set_xlabel("CPU · % of one core · log scale", color=fg, labelpad=12)
        axes[1].set_xlim(0, 180)
        axes[1].set_xticks([0, 50, 100, 150])
        axes[1].set_xlabel("Resident memory · MiB", color=fg, labelpad=12)
        fig.text(0.04, 0.94, "Idle resources", color=fg, fontsize=18, weight="bold")
        fig.text(
            0.04,
            0.865,
            "Linux x86_64 · ROS 2 Jazzy · Cyclone DDS · lower is better",
            color=fg,
            fontsize=10,
        )
        fig.text(0.61, 0.94, "● Python + rosapi", color=colors["python"], fontsize=10)
        fig.text(0.82, 0.94, "● Rust + native rosapi", color=colors["rust"], fontsize=10)
        fig.text(
            0.04,
            0.025,
            "Dots: all three 10-second trials. Bars: medians. Includes first-trial spikes; excludes the ROS CLI daemon.",
            color=fg,
            fontsize=8.5,
        )
        fig.subplots_adjust(left=0.28, right=0.975, top=0.79, bottom=0.23, wspace=0.17)
        svg = ROOT / f"comparison-{theme}.svg"
        fig.savefig(svg, facecolor=bg)
        svg.write_text("\n".join(line.rstrip() for line in svg.read_text().splitlines()) + "\n")
        if theme == "light":
            fig.savefig(ROOT / "comparison.png", dpi=150, facecolor=bg)
        plt.close(fig)
