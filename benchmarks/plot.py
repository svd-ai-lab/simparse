"""Render the introduction graphic from recorded measurements, without rerunning them."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from xml.sax.saxutils import escape

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import FancyBboxPatch
from matplotlib.ticker import MaxNLocator

ROOT = Path(__file__).resolve().parents[1]
INK = "#172b45"
MUTED = "#60718a"
TEAL = "#008a86"
PURPLE = "#7860c5"
BACKGROUND = "#f5f7fb"


def size_range(low, high):
    unit, scale = ("MiB", 1024 ** 2) if high >= 1024 ** 2 else ("KiB", 1024)
    if low / scale < .01 and unit == "MiB":
        return f"{low / 1024:.1f} KiB–{high / scale:.2f} MiB"
    return f"{low / scale:.2f}–{high / scale:.2f} {unit}" if low != high else f"{low / scale:.2f} {unit}"


def render(data, svg_path, png_path):
    rows = data["results"]
    total = data["aggregate"]
    protocol = data["protocol"]
    env = data["environment"]
    plt.rcParams.update({"font.family": "DejaVu Sans", "svg.fonttype": "path",
                         "svg.hashsalt": "simparse-benchmark-v2", "axes.unicode_minus": False})
    fig = plt.figure(figsize=(14, 9.4), facecolor=BACKGROUND)
    fig.text(.05, .948, "SIMPARSE  /  PUBLIC CORPUS BENCHMARK", color=TEAL, size=11, weight="bold")
    fig.text(.05, .896, "Engineering files → compact AI context", color=INK, size=29, weight="bold")
    fig.text(.05, .863, "Single-file metadata inspection with the Rust CLI · AI Infra System IR", color=MUTED, size=13)

    cards = [
        (str(len(rows)), "software / format groups", "CAD, electromagnetics, thermal & structural"),
        (str(total["file_count"]), "public files inspected", f"{total['input_bytes_total'] / 1024 ** 2:.1f} MiB of source files · pinned & checksummed"),
        (f"{total['ir_bytes']['median'] / 1024:.2f} KiB", "median IR per file", "Large geometry, mesh & results stay external"),
    ]
    for index, (value, label, detail) in enumerate(cards):
        x = .05 + index * .31
        fig.add_artist(FancyBboxPatch((x, .735), .29, .1, boxstyle="round,pad=0.009,rounding_size=0.012",
                                     transform=fig.transFigure, facecolor="white", edgecolor="#e2e8f0", linewidth=.8))
        fig.text(x + .012, .793, value, color=INK, size=27, weight="bold")
        fig.text(x + .012, .766, label, color=INK, size=12)
        fig.text(x + .012, .745, detail, color=MUTED, size=8.6)

    fig.text(.05, .686, "SOFTWARE / FORMAT", color=MUTED, size=10, weight="bold")
    fig.text(.31, .686, "INSPECTION TIME", color=TEAL, size=11, weight="bold")
    fig.text(.68, .686, "IR SIZE", color=PURPLE, size=11, weight="bold")
    fig.text(.05, .660, "File count · source size range", color=MUTED, size=10)
    fig.text(.31, .660, "Fresh process + extraction + JSON output", color=MUTED, size=10)
    fig.text(.68, .660, "Compact JSON · IR field only", color=MUTED, size=10)

    label_ax = fig.add_axes([.05, .20, .24, .44])
    time_ax = fig.add_axes([.31, .20, .245, .44])
    size_ax = fig.add_axes([.68, .20, .19, .44])
    for ax in (label_ax, time_ax, size_ax):
        ax.set_facecolor(BACKGROUND)
        ax.set_ylim(len(rows) - .35, -.7)
        ax.set_yticks([])
        for spine in ax.spines.values():
            spine.set_visible(False)
    label_ax.set_xlim(0, 1)
    label_ax.axis("off")

    for index, row in enumerate(rows):
        stats = row["aggregate"]
        label_ax.text(0, index - .05, row["label"], color=INK, size=13, weight="bold", va="center")
        input_range = size_range(stats["input_bytes"]["min"], stats["input_bytes"]["max"])
        n = stats["file_count"]
        label_ax.text(0, index + .3, f"{n} {'file' if n == 1 else 'files'} · {input_range}", color=MUTED, size=9, va="center")
        for ax, key, scale, color, suffix in (
                (time_ax, "cli_ms", 1, TEAL, "ms"),
                (size_ax, "ir_bytes", 1024, PURPLE, "KiB")):
            med, low, high = (stats[key][k] / scale for k in ("median", "min", "max"))
            ax.barh(index, med, height=.26, color=color, alpha=.78, zorder=2)
            ax.plot([low, high], [index, index], color=INK, linewidth=1.1, zorder=3)
            ax.plot([low, high], [index, index], "|", color=INK, markersize=5, zorder=3)
            ax.plot(med, index, "o", color=color, markeredgecolor="white", markeredgewidth=1, markersize=7, zorder=4)
            digits = 1 if key == "cli_ms" else 2
            ax.text(1.045, index, f"{med:.{digits}f} {suffix}", transform=ax.get_yaxis_transform(),
                    va="center", color=INK, size=12, weight="bold")

    for ax, key, scale, unit in ((time_ax, "cli_ms", 1, "milliseconds"),
                                (size_ax, "ir_bytes", 1024, "KiB")):
        maximum = max(row["aggregate"][key]["max"] / scale for row in rows)
        ax.set_xlim(0, maximum * 1.12)
        ax.xaxis.set_major_locator(MaxNLocator(nbins=4, min_n_ticks=3))
        ax.tick_params(axis="x", colors=MUTED, labelsize=9, length=0, pad=9)
        ax.xaxis.grid(True, color="#dce3ed", linewidth=.7, zorder=0)
        ax.set_axisbelow(True)
        ax.set_xlabel(unit, color=MUTED, size=9, labelpad=8)

    fig.text(.05, .121,
             f"Bars: median across files. Whiskers: file-to-file range. Timing: {protocol['iterations_per_file']} runs/file, "
             f"{protocol['warmups_per_file']} excluded warmup, warm filesystem cache.", color=MUTED, size=9)
    fig.text(.05, .097, "Shallow metadata only; IR size is not lossless compression. Coverage and retained detail vary by format.", color=MUTED, size=9)
    negatives = data.get("negative_controls", [])
    if negatives:
        fig.text(.05, .073, f"{len(negatives)} additional malformed STEP control rejected as expected; excluded from the bars and file count.", color=MUTED, size=9)
    fig.text(.05, .040, f"{env['os']} {env['os_release']} · {env['cpu']} · release build · {data['generated_at_utc'][:10]}", color=MUTED, size=8)
    fig.text(.95, .040, "github.com/svd-ai-lab/simparse", color=TEAL, size=9, ha="right")

    svg_path, png_path = Path(svg_path), Path(png_path)
    svg_path.parent.mkdir(parents=True, exist_ok=True)
    png_path.parent.mkdir(parents=True, exist_ok=True)
    metadata = {"Date": data["generated_at_utc"], "Creator": "simparse benchmark plot"}
    fig.savefig(svg_path, format="svg", metadata=metadata)
    fig.savefig(png_path, format="png", dpi=160, metadata={"Software": "simparse benchmark plot"})
    plt.close(fig)
    # Keep the standalone SVG understandable to screen readers, not just visually.
    title = "simparse: public single-file inspection benchmark"
    description = "; ".join(
        f"{row['label']}: {row['aggregate']['file_count']} "
        f"{'file' if row['aggregate']['file_count'] == 1 else 'files'}, "
        f"median {row['aggregate']['cli_ms']['median']:.1f} ms, "
        f"IR {row['aggregate']['ir_bytes']['median'] / 1024:.2f} KiB"
        for row in rows)
    description += (". Bars show medians; whiskers show file-to-file ranges. "
                    "Shallow metadata inspection only; IR size is not lossless compression. "
                    f"{len(negatives)} malformed controls excluded from timing and file counts.")
    svg = svg_path.read_text(encoding="utf-8")
    start = svg.index("<svg")
    end = svg.index(">", start)
    svg = (svg[:end] + ' role="img" aria-labelledby="benchmark-title benchmark-description"' + svg[end:end + 1]
           + f'<title id="benchmark-title">{escape(title)}</title><desc id="benchmark-description">{escape(description)}</desc>'
           + svg[end + 1:])
    svg = "\n".join(line.rstrip() for line in svg.splitlines()) + "\n"
    svg_path.write_text(svg, encoding="utf-8", newline="\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, default=ROOT / "artifacts/public-benchmark.json")
    args = parser.parse_args()
    data = json.loads(args.input.read_text(encoding="utf-8"))
    render(data, args.input.with_suffix(".svg"), args.input.with_suffix(".png"))


if __name__ == "__main__":
    main()
