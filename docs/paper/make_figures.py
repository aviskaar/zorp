"""Generate the figures used in zorp-paper.md.

Re-run with `python3 make_figures.py` from this directory whenever the
architecture or test counts change. Every figure is built from real repo
state (crate layout, spec text, `cargo test` output), not illustrative
mockups.
"""

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import matplotlib.patches as mpatches
from matplotlib.patches import FancyBboxPatch, FancyArrowPatch, Circle
import os

OUT = os.path.join(os.path.dirname(__file__), "figures")
os.makedirs(OUT, exist_ok=True)

BG = "#0d1117"
PURPLE = "#a371f7"
PURPLE_DIM = "#6e40a3"
FG = "#e6edf3"
GRAY = "#8b949e"
GREEN = "#3fb950"


def box(ax, x, y, w, h, text, fc="#161b22", ec=PURPLE, fontsize=10, textcolor=FG, lw=1.6):
    p = FancyBboxPatch(
        (x, y), w, h,
        boxstyle="round,pad=0.02,rounding_size=0.08",
        fc=fc, ec=ec, lw=lw, mutation_aspect=1,
    )
    ax.add_patch(p)
    ax.text(x + w / 2, y + h / 2, text, ha="center", va="center",
             color=textcolor, fontsize=fontsize, fontweight="normal")
    return p


# ---------------------------------------------------------------------------
# Figure 0: logo (redrawn from zorp-landing/public/favicon.svg: a rounded
# dark square with a purple "Z" glyph, viewBox 0 0 64 64, path
# "M18 20h28l-22 24h22").
# ---------------------------------------------------------------------------
fig, ax = plt.subplots(figsize=(1.4, 1.4), dpi=300)
fig.patch.set_facecolor("none")
ax.set_xlim(0, 64)
ax.set_ylim(0, 64)
ax.invert_yaxis()
ax.axis("off")
ax.add_patch(FancyBboxPatch((0, 0), 64, 64, boxstyle="round,pad=0,rounding_size=14",
                             fc=BG, ec="none"))
ax.plot([18, 46, 24, 46], [20, 20, 44, 44], color=PURPLE, linewidth=6,
        solid_capstyle="round", solid_joinstyle="round")
fig.savefig(os.path.join(OUT, "logo.png"), transparent=True, bbox_inches="tight", pad_inches=0)
plt.close(fig)


# ---------------------------------------------------------------------------
# Figure 1: layered architecture (from docs/superpowers/specs/
# 2026-08-09-zorp-architecture-design.md, "Structure" section).
# ---------------------------------------------------------------------------
fig, ax = plt.subplots(figsize=(6.4, 4.6), dpi=300)
fig.patch.set_facecolor("white")
ax.set_xlim(0, 10)
ax.set_ylim(0, 10)
ax.axis("off")

box(ax, 0.5, 8.0, 9.0, 1.4, "zorp-agent (one binary, one CLI)",
    fc="#1c2333", ec=PURPLE, fontsize=12)

caps = ["validate", "investigate", "co-write", "deliver"]
cw = 2.1
for i, c in enumerate(caps):
    box(ax, 0.5 + i * (cw + 0.1), 6.1, cw, 1.3, c, fc="#161b22", ec=PURPLE, fontsize=10.5)

box(ax, 0.5, 4.1, 9.0, 1.5,
    "zorp-track (foundation, feature-gated)\ntracks · DuckDB run record · LanceDB · checkpoints",
    fc="#161b22", ec=PURPLE_DIM, fontsize=10)

box(ax, 0.5, 2.1, 9.0, 1.5,
    "existing zorp-agent harness\ntools · sandbox · trust · verify · sessions · MCP",
    fc="#161b22", ec=GRAY, fontsize=10)

box(ax, 0.5, 0.3, 9.0, 1.3,
    "zorp core crate (src/): model transport, raw primitives\n(binary: zorp)",
    fc="#161b22", ec=GRAY, fontsize=10)

for y0, y1 in [(8.0, 7.4), (6.1, 5.6), (4.1, 3.6), (2.1, 1.6)]:
    ax.annotate("", xy=(5, y1), xytext=(5, y0),
                arrowprops=dict(arrowstyle="-|>", color=GRAY, lw=1.2))

fig.savefig(os.path.join(OUT, "architecture.png"), bbox_inches="tight", pad_inches=0.15)
plt.close(fig)


# ---------------------------------------------------------------------------
# Figure 2: the four capabilities as a checkpointed pipeline (from
# docs/ARCHITECTURE.md and the per-capability specs). Each capability is
# also usable standalone -- shown as the dashed entry arrows.
# ---------------------------------------------------------------------------
fig, ax = plt.subplots(figsize=(7.4, 4.0), dpi=300)
fig.patch.set_facecolor("white")
ax.set_xlim(0, 12.4)
ax.set_ylim(0, 4.3)
ax.axis("off")

stages = [
    ("validate", "novelty +\nfeasibility read\n(needs a\nsearch MCP tool)"),
    ("investigate", "staged, pre-\nregistered attempts;\nevery one recorded"),
    ("co-write", "drafts from the\nrecorded evidence;\nhuman is author\nof record"),
    ("deliver", "matches draft to\nreal venues\n(needs huiban\nMCP tool)"),
]
n = len(stages)
w, h = 2.5, 1.8
gap = 0.55
x0 = 0.4
y0 = 1.1

for i, (name, desc) in enumerate(stages):
    x = x0 + i * (w + gap)
    box(ax, x, y0, w, h, f"{name}\n\n{desc}", fc="#161b22", ec=PURPLE, fontsize=8.3)
    cx = x + w / 2
    ax.add_patch(Circle((cx, y0 + h + 0.35), 0.16, fc=GREEN, ec="none"))
    ax.text(cx, y0 + h + 0.35, "✓", ha="center", va="center", fontsize=8, color="white")
    ax.text(cx, y0 - 0.32, "runs\nstandalone", ha="center", va="top", fontsize=6.6, color=GRAY)
    ax.annotate("", xy=(cx, y0 - 0.02), xytext=(cx, y0 - 0.42),
                arrowprops=dict(arrowstyle="-|>", color=GRAY, lw=0.9, linestyle="dashed"))
    if i < n - 1:
        xn = x0 + (i + 1) * (w + gap)
        ax.annotate("", xy=(xn, y0 + h / 2), xytext=(x + w, y0 + h / 2),
                    arrowprops=dict(arrowstyle="-|>", color=PURPLE, lw=1.4))

ax.text(6.2, 3.85, "human checkpoint after each stage\n(interactive by default, --yes for unattended)",
        ha="center", va="bottom", fontsize=7.6, color=GRAY, style="italic")

ax.set_ylim(0, 4.3)
fig.savefig(os.path.join(OUT, "pipeline.png"), bbox_inches="tight", pad_inches=0.15)
plt.close(fig)


# ---------------------------------------------------------------------------
# Figure 3: test counts per crate, from actual `cargo test --workspace
# --exclude zorp-track` and `cargo test -p zorp-agent --features research`
# runs on 2026-08-13.
# ---------------------------------------------------------------------------
fig, ax = plt.subplots(figsize=(6.4, 3.4), dpi=300)
fig.patch.set_facecolor("white")

crates = ["src\n(zorp core)", "zorp-mcp", "zorp-eval", "zorp-agent\n(default)", "zorp-agent\n(+research)"]
counts = [11, 25, 40, 469, 445]
colors = [PURPLE_DIM, PURPLE_DIM, PURPLE_DIM, PURPLE, PURPLE]

bars = ax.bar(crates, counts, color=colors, edgecolor="#3a2359", linewidth=1.2)
for b, c in zip(bars, counts):
    ax.text(b.get_x() + b.get_width() / 2, b.get_height() + 6, str(c),
            ha="center", va="bottom", fontsize=9, color="#161b22")

ax.set_ylabel("passing tests")
ax.set_title("Test count by crate/feature set, cargo test, 2026-08-13", fontsize=10)
ax.spines[["top", "right"]].set_visible(False)
ax.set_ylim(0, max(counts) * 1.18)

fig.savefig(os.path.join(OUT, "testing.png"), bbox_inches="tight", pad_inches=0.15)
plt.close(fig)

print("figures written to", OUT)
