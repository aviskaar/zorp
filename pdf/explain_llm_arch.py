#!/usr/bin/env python3
"""One-page PDF explaining Large Language Model (LLM) architecture.

Sections:
  1. Forward pass (inference) pipeline  -- left-to-right schematic
  2. Training objective                 -- bullets + key-concepts legend
  3. Footer note

Uses the system "San Francisco" Unicode font (SFNS.ttf) so the arrows,
math symbols and accents render correctly. Visual hierarchy is built from
font size + accent colour rather than a bold family.
"""

from fpdf import FPDF

SF = "/System/Library/Fonts/SFNS.ttf"

# accent palette (0-255)
C_TITLE   = (20, 40, 70)
C_PIPE    = (20, 100, 70)
C_TITLE_BAR = (20, 50, 90)
C_ATT     = (40, 160, 120)
C_MLP     = (100, 60, 180)
C_HEAD    = (80, 170, 65)
C_IN      = (180, 40, 80)
C_OUT     = (20, 160, 70)
C_PANEL   = (245, 249, 255)
C_INK     = (26, 34, 52)


def build():
    pdf = FPDF()
    pdf.add_page()
    sf_italic = "/System/Library/Fonts/SFNSItalic.ttf"
    pdf.add_font("SF", fname=SF)
    pdf.add_font("SF", "I", fname=sf_italic)
    W = pdf.w

    def rule(x, y, color=(180, 190, 210)):
        pdf.set_draw_color(*color)
        pdf.set_xy(x, y)
        pdf.rect(x, y, 0.16, 2.4, "D")

    # ================= HEADER BAND =================
    pdf.set_fill_color(*C_TITLE)
    pdf.rect(0, 16, pdf.w, pdf.h - 32, "F")
    pdf.ln((pdf.h - 32) / 2)  # place title under band
    pdf.set_text_color(255, 255, 255)
    pdf.set_font("SF", "", 20)
    pdf.cell(W - 32, 8, "Large Language Model Architecture", new_x="LMGTABL")
    pdf.set_x(16)
    pdf.set_font("SF", "", 9)
    pdf.set_text_color(205, 220, 242)
    pdf.multi_cell(W - 32, 4.4,
                   "A generative deep-learning system: text is split into tokens, projected to vectors, "
                   "processed through layers of self-attention, then decoded back into text.")
    pdf.set_x(16)

    # ================= SECTION 1: PIPELINE =================
    y0 = pdf.get_y() + 2.6
    pdf.rule(14, y0, C_PIPE)
    pdf.set_text_color(*C_TITLE)
    pdf.set_font("SF", "B", 12)
    pdf.cell(W - 28, 6.2, "Forward pass (inference)", new_x="LMGTABL")
    y0 = pdf.get_y() + 0.7

    steps = [
        ("Text input",   C_IN),
        ("Embed\n+ Positional", C_PIPE),
        ("LayerNorm",    C_TITLE_BAR),
        ("Attention",    C_ATT),
        ("MLP",          C_MLP),
        ("Head",         C_HEAD),
        ("Text output",  C_OUT),
    ]
    n = len(steps)
    bwidth = 26
    gap = (W - 28 - n * bwidth) / (n - 1)
    x0 = 14
    for i, (label, border) in enumerate(steps):
        x = round(x0 + i * gap)
        # panel
        pdf.set_draw_color(*border)
        pdf.set_fill_color(*C_PANEL)
        pdf.set_text_color(*C_TITLE)
        pdf.set_font("SF", "B", 7.6)
        pdf.set_xy(x, y0)
        pdf.rect(x, y0, bwidth, 12, "D")
        pdf.rect(x + 0.35, y0 + 0.35, bwidth - 0.7, 11.3, "F")
        pdf.set_xy(x + 0.4, y0 + 5.2)
        pdf.multi_cell(bwidth - 0.8, 0.7, label, align="C")
        # arrow to next
        xn = round(x0 + (i + 1) * gap)
        pdf.set_draw_color(C_TITLE)
        pdf.set_text_color(96, 112, 150)
        pdf.set_font("SF", "", 6.5)
        pdf.set_text_color(96, 112, 150)
        pdf.set_xy(round((x + xn) / 2), y0 - 0.1)
        pdf.cell(xn - x, 9, "\u2192", align="C")
    ypar = pdf.get_y() + 2.4

    # ================= decoder explanation =================
    pdf.rule(14, ypar, C_PIPE)
    pdf.set_xy(14, ypar)
    pdf.set_font("SF", "B", 10)
    pdf.set_text_color(*C_TITLE)
    pdf.cell(W - 28, 0, "A causal transformer decoder", new_x="LMGTABL")
    ypar = pdf.get_y() + 0.95
    pdf.set_x(14)
    pdf.set_font("SF", "", 7.8)
    pdf.set_text_color(42, 52, 72)
    para = (
        "A decoder reads a sequence left-to-right while masking future tokens, so the first token "
        "only ever sees itself. At each position self-attention fuses every other position with learned "
        "query, key and value projections:  Attention(Q, K, V) = softmax(QK\u00b7 / \u221ad)\u00b7V.")
    pdf.multi_cell(W - 28, 4.4, para, align="J")
    pdf.set_x(14)
    para2 = (
        "L stacked blocks (12\u2013132) build a residual stream x\u2192 x+\u2211(Attention, MLP): the output of each "
        "attention layer is added to its input, then passed through a small feed-forward network. The "
        "final linear head maps the last hidden state to a probability distribution over the vocabulary.")
    pdf.multi_cell(W - 28, 4.4, para2, align="J")
    yb = pdf.get_y() + 3

    # ================= SECTION 2: TRAINING =================
    pdf.rule(14, yb, C_ATT)
    pdf.set_xy(14, yb)
    pdf.set_font("SF", "B", 12)
    pdf.set_text_color(*C_TITLE)
    pdf.cell(W - 28, 6.2, "Training objective", new_x="LMGTABL")
    yb = pdf.get_y() + 1.2

    labels = ["Goal: predict the next token",
              "Causal masking skips each token",
              "Loss = cross-entropy per token",
              "Trained on ~trillions of tokens",
              "Scale up parameters and data"]
    notes = ["Next-token prediction is the ground-truth signal.",
             "Future tokens are hidden from the model.",
             "Each token is softmaxed over the whole vocabulary.",
             "Big data, billions of parameters, many GPU-hours.",
             "Scaling is the inductive bias behind modern ability."]
    y = yb + 1
    for lab, note in zip(labels, notes):
        pdf.set_font("SF", "B", 8.2)
        pdf.set_text_color(*C_TITLE)
        pdf.set_xy(20, y)
        pdf.cell(pdf.get_string_width(lab) + 2, 4.1, lab)
        y += 4.9
        pdf.set_font("SF", "", 6.7)
        pdf.set_text_color(84, 94, 116)
        pdf.set_xy(20, y)
        pdf.cell(W - 40, 3.4, "\u2022 " + note)
        y += 3.4

    # right: key-concepts legend box
    lx = 190
    pdf.set_draw_color(*C_TITLE)
    pdf.set_fill_color(*C_PANEL)
    pdf.set_text_color(*C_TITLE)
    pdf.rect(lx, yb + 1, W - 28 - lx, 48, "DF")
    pdf.set_font("SF", "B", 7.6)
    pdf.set_xy(lx + 2, yb + 2.6)
    pdf.cell(W - 28 - lx - 4, 0, "Key concepts")
    cy = yb + 20
    for c in ["Embedding dim d \u2248 4096",
              "d_model \u00d7 H = Q/K/V dim",
              "softmax(QK\u00b7 / d)V",
              "Residual stream x\u2211 sublayers",
              "GPT-style causal decoder",
              "Vocabulary (token IDs)"]:
        pdf.set_font("SF", "", 6.6)
        pdf.set_xy(lx + 2, cy)
        pdf.multi_cell(W - 28 - lx - 3, 3.2, c)
        cy += 3.5

    # ================= FOOTER =================
    rule(14, 265)
    pdf.set_text_color(58, 66, 84)
    pdf.set_font("SF", "I", 7)
    pdf.set_x(14); pdf.set_y(266)
    pdf.cell(W - 28, 4,
             "Tokens \u2192 embeddings \u2192 (self-attention + MLP) repeated L times \u2192 next-token head. "
             "Scaling parameters and data transfers directly into capabilities, while causal masking keeps "
             "pretraining an honest next-word predictor. Illustrated per the Transformer and GPT families of papers.")

    pdf.output("llm_architecture_one_page.pdf")
    print("written llm_architecture_one_page.pdf (%.0f mm wide x %.0f mm tall)"
          % (pdf.w, pdf.page_height))


if __name__ == "__main__":
    build()
