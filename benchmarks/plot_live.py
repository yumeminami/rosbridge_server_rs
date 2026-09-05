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


"""Plot the recorded live browser container CPU samples."""
import json
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

ROOT = Path(__file__).parent / "results" / "live"
rows = json.loads((ROOT / "cpu.json").read_text())
elapsed = [row["time"] - rows[0]["time"] for row in rows]
for theme in ("light", "dark"):
    bg, fg, grid = (
        ("#ffffff", "#253047", "#e7eaf0") if theme == "light" else ("#111827", "#e5e7eb", "#283244")
    )
    with plt.rc_context({"font.family": "DejaVu Sans", "font.size": 10, "svg.fonttype": "none"}):
        fig, ax = plt.subplots(figsize=(11, 4.3))
        fig.patch.set_facecolor(bg)
        ax.set_facecolor(bg)
        ax.set_axisbelow(True)
        ax.grid(axis="y", color=grid, linewidth=0.8)
        for spine in ax.spines.values():
            spine.set_visible(False)
        for name, label, color in [
            ("rd001-rosbridge-1", "Python + rosapi", "#8594a7"),
            ("rosbridge_server_rs", "Rust + native rosapi", "#8b5cf6"),
        ]:
            values = [
                float(next(c for c in row["containers"] if c["Name"] == name)["CPUPerc"][:-1])
                for row in rows
            ]
            ax.plot(elapsed, values, color=color, linewidth=2, label=label)
        ax.set_ylim(0, 55)
        ax.set_xlim(0, 60)
        ax.tick_params(axis="both", length=0, colors=fg, pad=8)
        ax.set_xlabel("Elapsed time · seconds", color=fg, labelpad=10)
        ax.set_ylabel("CPU · % of one core", color=fg, labelpad=10)
        ax.legend(frameon=False, labelcolor=fg, loc="upper left")
        fig.text(0.07, 0.94, "Live browser workload", color=fg, fontsize=18, weight="bold")
        fig.text(
            0.07,
            0.87,
            "Shared ROS host · /diagnostics · one WebSocket connection per server",
            color=fg,
            fontsize=10,
        )
        fig.text(
            0.07,
            0.025,
            "30 Docker snapshots · all samples shown · observed CPU, not throughput or latency",
            color=fg,
            fontsize=9,
        )
        fig.subplots_adjust(left=0.09, right=0.97, top=0.79, bottom=0.19)
        svg = ROOT / f"comparison-{theme}.svg"
        fig.savefig(svg, facecolor=bg)
        svg.write_text("\n".join(line.rstrip() for line in svg.read_text().splitlines()) + "\n")
        plt.close(fig)
