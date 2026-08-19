# Transformer Architecture: How Large Language Models Work

## 1. Origin

The Transformer was introduced in the 2017 paper *"Attention Is All You Need"* by
Vaswani et al. It replaced recurrent neural networks (RNNs) and convolutional
neural networks (CNNs) in sequence tasks and became the sole architecture behind
every major LLM released since 2017.

The core idea: compute relationships between every pair of positions in a
sequence in parallel, rather than processing elements one after another.

---

## 2. Core Components (per layer)

A single Transformer layer is a stack of three sub-modules. Modern LLMs
(GPT-3: 96 layers; LLaMA-3: 80 layers; etc.) simply repeat this block many times.

### 2.1 Multi-Head Self-Attention

Input: a matrix **X** of shape *(seq_len × d_model)*, where each row is a token
embedding (e.g. *d_model* = 12,800 for GPT-3).

Self-attention computes three projections per token per head:

```
Q = X · W_Q      (queries:  "what am I looking for?")
K = X · W_K      (keys:     "what can I match to?")
V = X · W_V      (values:   "what do I carry when matched?")
attn = softmax( Q Kᵀ / √d_k ) · V
```

- **d_k** is the per-head dimension; the model splits *d_model* into *H* heads
  (e.g. *H* = 96, *d_k* = 128 for GPT-3), so each head sees a smaller
  subspace and attends to a different pattern.
- The softmax produces a probability distribution over all positions; it is
  *not* the same as a matrix product—each row is normalised independently.
- Scaling by **1/√d_k** prevents the dot products from growing too large, which
  would push softmax into a saturated, near-zero-gradient region.

Output: a weighted blend of all value vectors, weighted by relevance.

### 2.2 Feed-Forward Network (FFN / MLP block)

After attention, each position's vector is passed through two linear layers with
a nonlinearity in between:

```
y = 1(1(X · W_1 + b_1)) · W_2 + b_2
```

where **1(·)** is GELU (GPT-3) or GELU/ReLU (other models). The hidden
dimension is usually **4 × d_model** (or 8/3 × d_model in GPT-4, which
compresses it back to a "swiglu" style expansion). FFN layers store
factoid and syntactic knowledge; most of a model's parameters live here.

### 2.3 Layer Normalisation & Residual Connections

Each sub-module wraps its computation in:

```
out = x + SubLayer(Norm(x))
```

(Pre-LN, used in GPT-3 and later, applies the normalisation *before* the
sub-module. Post-LN, used in the original 2017 paper, applies it *after*.)
Residual connections let the model to depth without signal degradation and
mean that gradients can flow through many layers.

---

## 3. Embedding Layer (before the first attention layer)

| Component | Purpose |
|---|---|
| **Token embedding** | Maps each token ID to a *d_model*-dim vector. Learned. |
| **Positional encoding** | Injects sequence order information. Original paper used fixed sin/cos; GPT-3 uses learned absolute positions; modern models (RoPE in LLaMA/Mistral) bake relative-position math into the attention score itself. |
| **Layer norm + dropout** | Initial stabilisation before the first transformer block. |

Tokens are discrete IDs (from a vocabulary of ~50k tokens for most modern
LLMs). Sentence "Hello world" → two IDs → two embedding rows, then position
encodings are added.

---

## 4. The Output Layer

After the final layer norm, the output of the last hidden layer is projected
to the vocabulary:

```
logits = h_final · W_vocab        shape: (seq_len, 50000)
probs  = softmax(logits)
```

The next token is sampled (temperature-adjusted) or arg-maxed from *probs*.
Because every position is computed in parallel, a single forward pass produces
probabilities for *all* subsequent positions simultaneously.

---

## 5. Pre-training Objective (Autoregressive, for LLMs)

GPT-style models are trained on next-token prediction:

Given token sequence **[t_1, t_2, …, t_n]**, the loss is:

```
L = - Σ  log P(t_{i+1} | t_1, …, t_i ;  θ)
```
over every window in every training sample (total: trillions of tokens).
θ is the set of all weight matrices *W_Q, W_K, W_V, W_ff, W_vocab*, etc.

---

## 6. Scaling and Emergent Capabilities

- **Parameters**: GPT-3 = 175 B; this grew to 340 B (Llama-2) and ~500 B
  (Llama-3). Each parameter is a floating-point value in one of the matrices
  above.
- **Depth**: more layers → more "rounds" of attention → longer-range context
  integration, but also more parameters and more training cost.
- **Emergence**: certain behaviours (few-shot reasoning, simple arithmetic,
  code generation) appear only above specific size/context thresholds and are
  not explicitly taught—this remains partly unexplained.

---

## 7. Key Limitations (honest summary)

| Limitation | Example |
|---|---|
| No memory of training-time data | A model cannot "remember" reading the source text; it only learned a compressed representation. |
| No real-time knowledge | Knowledge is frozen at end-of-training (e.g. Jan 2024 cutoff). |
| Hallucination | High-confidence fluency ≠ factual accuracy; the model optimises the training objective, not truth. |
| Context window limits | 8k–128k tokens; attention is O(n²), so it is expensive to extend. |

---

*Sources: Vaswani et al., "Attention Is All You Need," NeurIPS 2017;
Brown et al., "Language Models are Few-Shot Learners" (GPT-3), 2020;
Touvron et al., "Llama 2," 2023; Gu et al., "RoPE," 2021.*
