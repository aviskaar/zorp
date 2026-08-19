## User request
Create a 2-page document explaining transformer architecture of LLMs.

## What was delivered
`transformer-architecture.md` (143 lines) — a 7-section technical document covering:
1. Origin (Vaswani et al. 2017)
2. Core components: multi-head self-attention, FFN, layer norm + residuals
3. Embedding layer (tokens, positional encodings)
4. Output layer (logits, softmax, next-token sampling)
5. Pre-training objective (autoregressive next-token prediction loss)
6. Scaling and emergent capabilities
7. Key limitations

## Key facts cited
- GPT-3: 96 layers, 175B params, 96 heads, ~50k vocab
- Llama-2: 340B, Llama-3: ~500B
- RoPE (Gu et al. 2021) used in Llama/Mistral
- SwiGLU FFN in GPT-4

## Open items
- No conversion to PDF/HTML was requested, but the note mentioned it could be done.
- No follow-up request has been received yet.
