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

"""Plot the recorded SoC process CPU and RSS samples."""
import csv
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

out = Path(__file__).parent / "results/soc"
with (out / "samples.csv").open() as source:
    rows = [{k: float(v) for k, v in row.items()} for row in csv.DictReader(source)]
plt.rcParams.update({"font.family": "DejaVu Sans", "svg.fonttype": "none"})
for theme, background, foreground, grid in [
    ("light", "#ffffff", "#243047", "#e4e8ef"),
    ("dark", "#0d1117", "#dce4ef", "#293342"),
]:
    fig, axes = plt.subplots(1, 2, figsize=(11, 3.8), layout="constrained")
    fig.patch.set_facecolor(background)
    for ax, metric, title in zip(
        axes, ["cpu", "rss_mib"], ["CPU · % of one core", "Process RSS · MiB"]
    ):
        ax.set_facecolor(background)
        for name, label, color in [
            ("rust", "Rust + native rosapi", "#28a98c"),
            ("python", "Python + rosapi", "#8b7bea"),
        ]:
            ax.plot(
                [r["seconds"] for r in rows],
                [r[name + "_" + metric] for r in rows],
                label=label,
                color=color,
                linewidth=2,
            )
        ax.set_title(title, loc="left", color=foreground, pad=14, weight="bold")
        ax.set_xlabel("Elapsed seconds", color=foreground)
        ax.set_ylim(bottom=0)
        ax.set_xlim(0, rows[-1]["seconds"])
        ax.tick_params(colors=foreground, length=0)
        ax.grid(axis="y", color=grid)
        ax.set_axisbelow(True)
        for spine in ax.spines.values():
            spine.set_visible(False)
    axes[0].legend(frameon=False, labelcolor=foreground, loc="upper left", fontsize=9)
    fig.savefig(out / f"comparison-{theme}.svg", facecolor=background)
    plt.close(fig)
