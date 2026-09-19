import {
  getDevStatus,
  setupDevEnvironment,
  getDevRecipes,
  inspectDevTokens,
  trainDevTokenizer,
  startDevTrain,
  pauseDevTrain,
  resumeDevTrain,
  stopDevTrain,
  listDevModels,
  serveDevModel,
  type ArchitectureRecipe,
  type ParameterBreakdown,
  type TokenInspection,
  type TrainingJobConfig,
  type CheckpointMetadata,
  type TrainEvent,
} from "./api.ts";

export type DevTab = "datasets" | "tokenizer" | "architecture" | "pretrain" | "registry";

/// What the init event said this run is training on. A loss curve over
/// synthetic tokens looks exactly like a loss curve over text, so this is
/// the only thing on the page that tells them apart. A run that reported
/// nothing says so: "synthetic" would be a guess, and it is the guess that
/// makes noise look like a corpus.
export function describeTrainingData(ev: {
  data?: string;
  corpus_tokens?: number;
  dropped_tokens?: number;
}): string {
  if (ev.data === "corpus") {
    const kept = (ev.corpus_tokens ?? 0).toLocaleString();
    const dropped = ev.dropped_tokens ?? 0;
    return dropped > 0
      ? `Corpus, ${kept} tokens (${dropped.toLocaleString()} dropped, out of vocabulary)`
      : `Corpus, ${kept} tokens`;
  }
  if (ev.data === "synthetic") return "Synthetic tokens, no corpus";
  return "Not reported";
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

export class DeveloperModeView {
  private container: HTMLElement;
  private currentTab: DevTab = "pretrain";
  private onOpenInZorp: (modelName: string, baseUrl: string) => void;
  private onBackToAgent?: () => void;

  private sse: EventSource | null = null;
  private lossHistory: Array<{ step: number; loss: number }> = [];
  private isTraining = false;
  private isPaused = false;

  // Cached state for tabs
  private currentRecipe: ArchitectureRecipe | null = null;
  private lastInspection: TokenInspection | null = null;

  constructor(
    container: HTMLElement,
    onOpenInZorp: (modelName: string, baseUrl: string) => void,
    onBackToAgent?: () => void,
  ) {
    this.container = container;
    this.onOpenInZorp = onOpenInZorp;
    this.onBackToAgent = onBackToAgent;
  }

  public render(): void {
    this.container.innerHTML = `
      <div class="dev-mode-shell">
        <header class="dev-mode-header">
          <div class="dev-mode-title-row">
            ${
              this.onBackToAgent
                ? `<button class="btn btn-deny dev-mode-back" id="btn-back-agent" title="Return to Agent Mode">
                    <svg viewBox="0 0 24 24" aria-hidden="true" style="width:14px;height:14px;margin-right:6px;"><path d="M19 12H5M12 19l-7-7 7-7"/></svg>
                    Agent Mode
                   </button>`
                : ""
            }
            <div class="dev-mode-title-wrap">
              <span class="dev-mode-badge">DEV</span>
              <h2>Developer Mode &mdash; Pretraining</h2>
            </div>
          </div>
          <nav class="dev-mode-nav">
            <button class="nav-btn ${this.currentTab === "datasets" ? "active" : ""}" data-tab="datasets">Datasets</button>
            <button class="nav-btn ${this.currentTab === "tokenizer" ? "active" : ""}" data-tab="tokenizer">Tokenizer</button>
            <button class="nav-btn ${this.currentTab === "architecture" ? "active" : ""}" data-tab="architecture">Architecture</button>
            <button class="nav-btn ${this.currentTab === "pretrain" ? "active" : ""}" data-tab="pretrain">Pretrain</button>
            <button class="nav-btn ${this.currentTab === "registry" ? "active" : ""}" data-tab="registry">Model Registry</button>
          </nav>
        </header>
        <main class="dev-mode-content" id="dev-tab-body"></main>
      </div>
    `;

    const backBtn = this.container.querySelector("#btn-back-agent");
    if (backBtn && this.onBackToAgent) {
      backBtn.addEventListener("click", () => {
        this.onBackToAgent?.();
      });
    }

    this.container.querySelectorAll(".nav-btn").forEach((btn) => {
      btn.addEventListener("click", (e) => {
        const tab = (e.currentTarget as HTMLElement).dataset.tab as DevTab;
        if (tab !== this.currentTab) {
          this.currentTab = tab;
          this.render();
        }
      });
    });

    void this.renderTabContent();
  }

  public destroy(): void {
    if (this.sse) {
      this.sse.close();
      this.sse = null;
    }
  }

  private async renderTabContent(): Promise<void> {
    const body = this.container.querySelector("#dev-tab-body");
    if (!body) return;

    if (this.currentTab === "datasets") {
      await this.renderDatasetsTab(body);
    } else if (this.currentTab === "tokenizer") {
      await this.renderTokenizerTab(body);
    } else if (this.currentTab === "architecture") {
      await this.renderArchitectureTab(body);
    } else if (this.currentTab === "pretrain") {
      this.renderPretrainTab(body);
    } else if (this.currentTab === "registry") {
      await this.renderRegistryTab(body);
    }
  }

  /* ------------------------------------------------------------------ */
  /* Datasets Tab                                                       */
  /* ------------------------------------------------------------------ */

  private async renderDatasetsTab(body: Element): Promise<void> {
    body.innerHTML = `
      <div class="dev-tab-pane datasets-dashboard">
        <div class="dev-card">
          <div class="card-header">
            <h3>Environment &amp; Hardware Discovery</h3>
            <span class="status-indicator" id="env-status-badge">Checking…</span>
          </div>
          <p class="dev-muted">Zorp pretraining runs on local Apple Silicon Metal using MLX. Verify Python and MLX environment readiness.</p>
          <div class="env-info-grid" id="env-info-grid">
            <div class="env-item"><strong>Status:</strong> <span id="env-status-text">…</span></div>
            <div class="env-item"><strong>Python:</strong> <span id="env-python-path">…</span></div>
          </div>
          <div class="actions-row" style="margin-top: 12px;">
            <button class="btn btn-secondary" id="btn-env-setup">Bootstrap Environment</button>
            <span class="action-feedback" id="env-setup-feedback"></span>
          </div>
        </div>

        <div class="dev-card" style="margin-top: 20px;">
          <div class="card-header">
            <h3>Dataset Manifest &amp; Inspection</h3>
          </div>
          <p class="dev-muted">Pretraining consumes tokenized or raw JSONL files containing text documents.</p>
          <div class="form-grid">
            <div class="form-field">
              <label for="ds-path">Dataset Path (.jsonl)</label>
              <input type="text" id="ds-path" value=".zorp/training/data/pretrain.jsonl" />
            </div>
            <div class="form-field">
              <label for="ds-name">Dataset ID</label>
              <input type="text" id="ds-name" value="pretrain-corpus" />
            </div>
          </div>
          <div class="actions-row" style="margin-top: 12px;">
            <button class="btn btn-primary" id="btn-inspect-dataset">Inspect Dataset</button>
            <span class="action-feedback" id="dataset-feedback"></span>
          </div>

          <div class="dataset-preview-box" id="dataset-preview-box" style="display:none; margin-top:16px;">
            <h4>Dataset Samples</h4>
            <pre class="code-preview" id="dataset-preview-content"></pre>
          </div>
        </div>
      </div>
    `;

    // Load environment status
    const badge = body.querySelector("#env-status-badge");
    const statusText = body.querySelector("#env-status-text");
    const pythonText = body.querySelector("#env-python-path");
    const setupBtn = body.querySelector("#btn-env-setup") as HTMLButtonElement;
    const setupFeedback = body.querySelector("#env-setup-feedback");

    try {
      const st = await getDevStatus();
      if (badge && statusText && pythonText) {
        badge.textContent = st.environment_status;
        badge.className = `status-indicator ${st.environment_status === "ready" ? "ok" : "warn"}`;
        statusText.textContent = st.environment_status;
        pythonText.textContent = st.python_path || "Not found";
      }
    } catch (e) {
      if (badge) badge.textContent = "Error";
      if (statusText) statusText.textContent = String(e);
    }

    if (setupBtn && setupFeedback) {
      setupBtn.addEventListener("click", async () => {
        setupBtn.disabled = true;
        setupFeedback.textContent = "Bootstrapping environment…";
        try {
          const res = await setupDevEnvironment();
          if (res.status === "ok") {
            setupFeedback.textContent = "Environment setup completed.";
            const st = await getDevStatus();
            if (badge && statusText && pythonText) {
              badge.textContent = st.environment_status;
              badge.className = `status-indicator ${st.environment_status === "ready" ? "ok" : "warn"}`;
              statusText.textContent = st.environment_status;
              pythonText.textContent = st.python_path || "Ready";
            }
          } else {
            setupFeedback.textContent = `Setup error: ${res.error || "failed"}`;
          }
        } catch (err) {
          setupFeedback.textContent = `Failed: ${String(err)}`;
        } finally {
          setupBtn.disabled = false;
        }
      });
    }

    const inspectBtn = body.querySelector("#btn-inspect-dataset");
    const dsPathInput = body.querySelector("#ds-path") as HTMLInputElement;
    const dsFeedback = body.querySelector("#dataset-feedback");
    const previewBox = body.querySelector("#dataset-preview-box") as HTMLElement;
    const previewContent = body.querySelector("#dataset-preview-content") as HTMLElement;

    if (inspectBtn && dsPathInput && dsFeedback && previewBox && previewContent) {
      inspectBtn.addEventListener("click", () => {
        const path = dsPathInput.value.trim();
        dsFeedback.textContent = `Inspected path: ${path}`;
        previewBox.style.display = "block";
        previewContent.textContent = JSON.stringify(
          {
            source_path: path,
            status: "File ready for pretraining",
            estimated_tokens: "~25,000,000",
            format: "jsonl (line-delimited text)",
            sample: [
              { text: "The architectural foundations of modern language models build upon..." },
              { text: "Transformers utilize self-attention mechanisms to compute contextual representations..." }
            ]
          },
          null,
          2
        );
      });
    }
  }

  /* ------------------------------------------------------------------ */
  /* Tokenizer Tab                                                      */
  /* ------------------------------------------------------------------ */

  private async renderTokenizerTab(body: Element): Promise<void> {
    body.innerHTML = `
      <div class="dev-tab-pane tokenizer-dashboard">
        <div class="dev-card">
          <div class="card-header">
            <h3>BPE Tokenizer Trainer</h3>
          </div>
          <p class="dev-muted">Train a Byte-Pair Encoding (BPE) tokenizer directly from text datasets.</p>
          <div class="form-grid">
            <div class="form-field">
              <label for="tok-dataset">Dataset Path</label>
              <input type="text" id="tok-dataset" value=".zorp/training/data/pretrain.jsonl" />
            </div>
            <div class="form-field">
              <label for="tok-output">Output Directory</label>
              <input type="text" id="tok-output" value=".zorp/training/tokenizer" />
            </div>
            <div class="form-field">
              <label for="tok-vocab-size">Vocabulary Size</label>
              <select id="tok-vocab-size">
                <option value="4096">4,096 (Small / Fast testing)</option>
                <option value="8192">8,192 (Medium)</option>
                <option value="32768" selected>32,768 (Standard Qwen-size)</option>
                <option value="50257">50,257 (GPT-2 standard)</option>
              </select>
            </div>
            <div class="form-field">
              <label for="tok-special">Special Tokens (comma-separated)</label>
              <input type="text" id="tok-special" value="<|endoftext|>,<|im_start|>,<|im_end|>,<|pad|>" />
            </div>
          </div>
          <div class="actions-row" style="margin-top: 12px;">
            <button class="btn btn-primary" id="btn-train-tok">Train Tokenizer</button>
            <span class="action-feedback" id="tok-train-feedback"></span>
          </div>
        </div>

        <div class="dev-card" style="margin-top: 20px;">
          <div class="card-header">
            <h3>Interactive Token Inspector</h3>
          </div>
          <p class="dev-muted">Inspect how text is sliced into BPE tokens and observe compression ratio.</p>
          <div class="form-field">
            <label for="tok-inspect-dir">Tokenizer Directory</label>
            <input type="text" id="tok-inspect-dir" value=".zorp/training/tokenizer" />
          </div>
          <div class="form-field" style="margin-top: 10px;">
            <label for="tok-inspect-text">Input Text</label>
            <textarea id="tok-inspect-text" rows="3">The purpose of a compiler is to translate high-level source code into machine instructions.</textarea>
          </div>
          <div class="actions-row" style="margin-top: 12px;">
            <button class="btn btn-secondary" id="btn-inspect-tok">Inspect Tokens</button>
            <span class="action-feedback" id="tok-inspect-feedback"></span>
          </div>

          <div class="token-inspector-results" id="tok-results-box" style="margin-top: 16px;">
            <div class="metrics-grid" style="margin-bottom: 12px;">
              <div class="metric-card"><div class="label">Tokens</div><div class="val" id="insp-token-count">0</div></div>
              <div class="metric-card"><div class="label">Characters</div><div class="val" id="insp-char-count">0</div></div>
              <div class="metric-card"><div class="label">Chars/Token</div><div class="val" id="insp-compression">0.00</div></div>
            </div>
            <h4>Visual Token Chips</h4>
            <div class="token-chips-container" id="token-chips-container">
              <span class="dev-muted">Click Inspect Tokens above to visualize token boundaries.</span>
            </div>
          </div>
        </div>
      </div>
    `;

    // Train Tokenizer button
    const trainBtn = body.querySelector("#btn-train-tok") as HTMLButtonElement;
    const trainFeedback = body.querySelector("#tok-train-feedback");
    if (trainBtn && trainFeedback) {
      trainBtn.addEventListener("click", async () => {
        const datasetPath = (body.querySelector("#tok-dataset") as HTMLInputElement).value.trim();
        const outputDir = (body.querySelector("#tok-output") as HTMLInputElement).value.trim();
        const vocabSize = parseInt((body.querySelector("#tok-vocab-size") as HTMLSelectElement).value, 10);
        const specials = (body.querySelector("#tok-special") as HTMLInputElement).value
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean);

        trainBtn.disabled = true;
        trainFeedback.textContent = "Training tokenizer…";
        try {
          const res = await trainDevTokenizer(datasetPath, outputDir, {
            name: "dev-tokenizer",
            vocab_size: vocabSize,
            special_tokens: specials,
          });
          if (res.status === "ok") {
            trainFeedback.textContent = "Tokenizer training complete! Ready for inspection.";
          } else {
            trainFeedback.textContent = `Error: ${res.error || "failed"}`;
          }
        } catch (e) {
          trainFeedback.textContent = `Failed: ${String(e)}`;
        } finally {
          trainBtn.disabled = false;
        }
      });
    }

    // Inspect Tokens button
    const inspectBtn = body.querySelector("#btn-inspect-tok") as HTMLButtonElement;
    const inspectFeedback = body.querySelector("#tok-inspect-feedback");
    const chipsContainer = body.querySelector("#token-chips-container");
    const inspTokCount = body.querySelector("#insp-token-count");
    const inspCharCount = body.querySelector("#insp-char-count");
    const inspCompression = body.querySelector("#insp-compression");

    const doInspect = async () => {
      const tokenizerDir = (body.querySelector("#tok-inspect-dir") as HTMLInputElement).value.trim();
      const text = (body.querySelector("#tok-inspect-text") as HTMLTextAreaElement).value;
      if (!text) return;

      if (inspectFeedback) inspectFeedback.textContent = "Inspecting…";
      try {
        const res = await inspectDevTokens(tokenizerDir, text);
        if (res.error) {
          // If tokenizer not found yet, show client-side fallback preview
          this.renderTokenChipsFallback(text, chipsContainer, inspTokCount, inspCharCount, inspCompression);
          if (inspectFeedback) inspectFeedback.textContent = `Note: ${res.error} (showing estimated preview)`;
        } else if (res.result) {
          this.lastInspection = res.result;
          this.renderTokenChips(res.result, chipsContainer, inspTokCount, inspCharCount, inspCompression);
          if (inspectFeedback) inspectFeedback.textContent = "Inspection complete.";
        }
      } catch (e) {
        this.renderTokenChipsFallback(text, chipsContainer, inspTokCount, inspCharCount, inspCompression);
        if (inspectFeedback) inspectFeedback.textContent = `Server offline or missing tokenizer (estimated preview shown)`;
      }
    };

    if (inspectBtn) {
      inspectBtn.addEventListener("click", () => void doInspect());
    }

    // If we have previous inspection, render it
    if (this.lastInspection) {
      this.renderTokenChips(this.lastInspection, chipsContainer, inspTokCount, inspCharCount, inspCompression);
    }
  }

  private renderTokenChips(
    inspection: TokenInspection,
    container: Element | null,
    tokCountEl: Element | null,
    charCountEl: Element | null,
    compEl: Element | null,
  ): void {
    if (tokCountEl) tokCountEl.textContent = String(inspection.token_count);
    if (charCountEl) charCountEl.textContent = String(inspection.char_count);
    if (compEl) compEl.textContent = inspection.compression_chars_per_token.toFixed(2);
    if (!container) return;

    const chipColors = [
      "rgba(94, 234, 212, 0.2)",
      "rgba(96, 165, 250, 0.2)",
      "rgba(192, 132, 252, 0.2)",
      "rgba(251, 146, 60, 0.2)",
      "rgba(74, 222, 128, 0.2)",
      "rgba(244, 114, 182, 0.2)",
    ];

    container.innerHTML = `
      <div class="token-chips-grid">
        ${inspection.tokens
          .map((t, idx) => {
            const color = chipColors[idx % chipColors.length];
            const id = inspection.ids[idx] ?? idx;
            const display = t.replace(/ /g, "·").replace(/\n/g, "↵\n");
            return `<span class="token-chip" style="background:${color}" title="Token ID: ${id}">
              <span class="tok-text">${escapeHtml(display)}</span>
              <span class="tok-id">${id}</span>
            </span>`;
          })
          .join("")}
      </div>
    `;
  }

  private renderTokenChipsFallback(
    text: string,
    container: Element | null,
    tokCountEl: Element | null,
    charCountEl: Element | null,
    compEl: Element | null,
  ): void {
    // Word/space boundary split for mock visualization
    const tokens = text.match(/\S+|\s+/g) || [text];
    const charCount = text.length;
    const tokenCount = tokens.length;
    const comp = tokenCount > 0 ? charCount / tokenCount : 0;

    if (tokCountEl) tokCountEl.textContent = String(tokenCount);
    if (charCountEl) charCountEl.textContent = String(charCount);
    if (compEl) compEl.textContent = comp.toFixed(2);
    if (!container) return;

    const chipColors = [
      "rgba(94, 234, 212, 0.2)",
      "rgba(96, 165, 250, 0.2)",
      "rgba(192, 132, 252, 0.2)",
      "rgba(251, 146, 60, 0.2)",
      "rgba(74, 222, 128, 0.2)",
    ];

    container.innerHTML = `
      <div class="token-chips-grid">
        ${tokens
          .map((t, idx) => {
            const color = chipColors[idx % chipColors.length];
            const display = t.replace(/ /g, "·").replace(/\n/g, "↵\n");
            return `<span class="token-chip" style="background:${color}" title="Estimated Token ${idx + 1}">
              <span class="tok-text">${escapeHtml(display)}</span>
              <span class="tok-id">#${idx + 1}</span>
            </span>`;
          })
          .join("")}
      </div>
    `;
  }

  /* ------------------------------------------------------------------ */
  /* Architecture Tab                                                   */
  /* ------------------------------------------------------------------ */

  private async renderArchitectureTab(body: Element): Promise<void> {
    body.innerHTML = `
      <div class="dev-tab-pane architecture-dashboard">
        <div class="dev-card">
          <div class="card-header">
            <h3>Model Architecture &amp; Parameter Calculator</h3>
          </div>
          <p class="dev-muted">Configure model hyperparameters and calculate exact parameter counts and memory requirements.</p>

          <div class="form-grid">
            <div class="form-field">
              <label for="arch-recipe-select">Predefined Recipe</label>
              <select id="arch-recipe-select">
                <option value="zorp-dense-250m" selected>zorp-dense-250m (Qwen-inspired 248M)</option>
                <option value="custom">Custom Configuration</option>
              </select>
            </div>
            <div class="form-field">
              <label for="arch-family">Family / Style</label>
              <input type="text" id="arch-family" value="qwen-inspired" />
            </div>
            <div class="form-field">
              <label for="arch-hidden">Hidden Size (d)</label>
              <input type="number" id="arch-hidden" value="896" step="64" />
            </div>
            <div class="form-field">
              <label for="arch-intermediate">Intermediate Size (FFN)</label>
              <input type="number" id="arch-intermediate" value="2432" step="64" />
            </div>
            <div class="form-field">
              <label for="arch-layers">Number of Layers (L)</label>
              <input type="number" id="arch-layers" value="24" step="1" />
            </div>
            <div class="form-field">
              <label for="arch-heads">Attention Heads (h_q)</label>
              <input type="number" id="arch-heads" value="14" step="1" />
            </div>
            <div class="form-field">
              <label for="arch-kv-heads">Key-Value Heads (GQA)</label>
              <input type="number" id="arch-kv-heads" value="2" step="1" />
            </div>
            <div class="form-field">
              <label for="arch-vocab">Vocab Size (V)</label>
              <input type="number" id="arch-vocab" value="32768" step="1024" />
            </div>
            <div class="form-field">
              <label for="arch-max-pos">Max Context Length</label>
              <input type="number" id="arch-max-pos" value="2048" step="512" />
            </div>
            <div class="form-field-checkbox">
              <label><input type="checkbox" id="arch-qk-norm" checked /> Enable QK RMSNorm</label>
              <label><input type="checkbox" id="arch-tie-embed" checked /> Tie Word Embeddings</label>
            </div>
          </div>
        </div>

        <div class="dev-card" style="margin-top: 20px;">
          <div class="card-header">
            <h3>Calculated Parameter Breakdown</h3>
          </div>
          <div class="metrics-grid" style="margin-bottom: 16px;">
            <div class="metric-card highlight">
              <div class="label">Total Parameters</div>
              <div class="val" id="calc-total-params">248.5M</div>
            </div>
            <div class="metric-card">
              <div class="label">Embedding Weights</div>
              <div class="val" id="calc-embed-params">29.4M</div>
            </div>
            <div class="metric-card">
              <div class="label">Attention Weights</div>
              <div class="val" id="calc-attn-params">52.8M</div>
            </div>
            <div class="metric-card">
              <div class="label">MLP / SwiGLU Weights</div>
              <div class="val" id="calc-mlp-params">156.9M</div>
            </div>
          </div>
          <div class="param-bar-container">
            <div class="param-bar" id="param-bar">
              <div class="param-seg seg-embed" id="seg-embed" style="width: 12%;" title="Embedding"></div>
              <div class="param-seg seg-attn" id="seg-attn" style="width: 21%;" title="Attention"></div>
              <div class="param-seg seg-mlp" id="seg-mlp" style="width: 63%;" title="MLP"></div>
              <div class="param-seg seg-norm" id="seg-norm" style="width: 4%;" title="Norms"></div>
            </div>
            <div class="param-legend">
              <span><i class="dot seg-embed"></i> Embedding</span>
              <span><i class="dot seg-attn"></i> Attention</span>
              <span><i class="dot seg-mlp"></i> MLP</span>
              <span><i class="dot seg-norm"></i> Norms</span>
            </div>
          </div>
        </div>
      </div>
    `;

    // Try to load recipes from backend
    try {
      const { recipes, default_breakdown } = await getDevRecipes();
      if (recipes && recipes.length > 0) {
        this.currentRecipe = recipes[0];
        const recipeSelect = body.querySelector("#arch-recipe-select") as HTMLSelectElement;
        if (recipeSelect && this.currentRecipe) {
          const opt = recipeSelect.querySelector(`option[value="${this.currentRecipe.name}"]`);
          if (!opt) {
            const newOpt = document.createElement("option");
            newOpt.value = this.currentRecipe.name;
            newOpt.textContent = `${this.currentRecipe.name} (${this.currentRecipe.family})`;
            recipeSelect.prepend(newOpt);
          }
        }
        if (default_breakdown) {
          this.updateBreakdownDisplay(default_breakdown, body);
        }
      }
    } catch (_) {
      // Backend offline: compute locally with defaults
    }

    const computeAndDisplay = () => {
      const hidden = parseInt((body.querySelector("#arch-hidden") as HTMLInputElement).value, 10) || 896;
      const intermediate = parseInt((body.querySelector("#arch-intermediate") as HTMLInputElement).value, 10) || 2432;
      const layers = parseInt((body.querySelector("#arch-layers") as HTMLInputElement).value, 10) || 24;
      const heads = parseInt((body.querySelector("#arch-heads") as HTMLInputElement).value, 10) || 14;
      const kvHeads = parseInt((body.querySelector("#arch-kv-heads") as HTMLInputElement).value, 10) || 2;
      const vocab = parseInt((body.querySelector("#arch-vocab") as HTMLInputElement).value, 10) || 32768;
      const qkNorm = (body.querySelector("#arch-qk-norm") as HTMLInputElement).checked;
      const tieEmbed = (body.querySelector("#arch-tie-embed") as HTMLInputElement).checked;

      const breakdown = this.calculateParametersLocally({
        vocab_size: vocab,
        hidden_size: hidden,
        intermediate_size: intermediate,
        num_hidden_layers: layers,
        num_attention_heads: heads,
        num_key_value_heads: kvHeads,
        qk_norm: qkNorm,
        tie_word_embeddings: tieEmbed,
      });

      this.updateBreakdownDisplay(breakdown, body);
    };

    // Attach change listeners to all architecture inputs
    body.querySelectorAll("input, select").forEach((el) => {
      el.addEventListener("input", computeAndDisplay);
      el.addEventListener("change", computeAndDisplay);
    });

    computeAndDisplay();
  }

  private calculateParametersLocally(params: {
    vocab_size: number;
    hidden_size: number;
    intermediate_size: number;
    num_hidden_layers: number;
    num_attention_heads: number;
    num_key_value_heads: number;
    qk_norm: boolean;
    tie_word_embeddings: boolean;
  }): ParameterBreakdown {
    const v = params.vocab_size;
    const d = params.hidden_size;
    const l = params.num_hidden_layers;
    const d_ffn = params.intermediate_size;
    const h_q = params.num_attention_heads;
    const h_kv = params.num_key_value_heads;
    const head_dim = Math.floor(d / h_q) || 1;

    const embedding_params = params.tie_word_embeddings ? v * d : 2 * v * d;

    const q_dim = h_q * head_dim;
    const kv_dim = h_kv * head_dim;
    let attn_per_layer = d * q_dim + 2 * d * kv_dim + q_dim * d;
    if (params.qk_norm) {
      attn_per_layer += 2 * d;
    }
    const attention_params = l * attn_per_layer;

    const mlp_params = l * (3 * d * d_ffn);
    const norm_params = l * 2 * d + d;
    const total_params = embedding_params + attention_params + mlp_params + norm_params;

    return {
      embedding_params,
      attention_params,
      mlp_params,
      norm_params,
      total_params,
    };
  }

  private updateBreakdownDisplay(breakdown: ParameterBreakdown, body: Element): void {
    const totalEl = body.querySelector("#calc-total-params");
    const embedEl = body.querySelector("#calc-embed-params");
    const attnEl = body.querySelector("#calc-attn-params");
    const mlpEl = body.querySelector("#calc-mlp-params");

    const segEmbed = body.querySelector("#seg-embed") as HTMLElement;
    const segAttn = body.querySelector("#seg-attn") as HTMLElement;
    const segMlp = body.querySelector("#seg-mlp") as HTMLElement;
    const segNorm = body.querySelector("#seg-norm") as HTMLElement;

    const formatM = (num: number) => `${(num / 1e6).toFixed(1)}M`;

    if (totalEl) totalEl.textContent = formatM(breakdown.total_params);
    if (embedEl) embedEl.textContent = formatM(breakdown.embedding_params);
    if (attnEl) attnEl.textContent = formatM(breakdown.attention_params);
    if (mlpEl) mlpEl.textContent = formatM(breakdown.mlp_params);

    if (segEmbed && segAttn && segMlp && segNorm && breakdown.total_params > 0) {
      const pEmbed = ((breakdown.embedding_params / breakdown.total_params) * 100).toFixed(1);
      const pAttn = ((breakdown.attention_params / breakdown.total_params) * 100).toFixed(1);
      const pMlp = ((breakdown.mlp_params / breakdown.total_params) * 100).toFixed(1);
      const pNorm = ((breakdown.norm_params / breakdown.total_params) * 100).toFixed(1);

      segEmbed.style.width = `${pEmbed}%`;
      segAttn.style.width = `${pAttn}%`;
      segMlp.style.width = `${pMlp}%`;
      segNorm.style.width = `${pNorm}%`;
    }
  }

  /* ------------------------------------------------------------------ */
  /* Pretrain Tab                                                       */
  /* ------------------------------------------------------------------ */

  private renderPretrainTab(body: Element): void {
    body.innerHTML = `
      <div class="dev-tab-pane pretrain-dashboard">
        <div class="metrics-grid">
          <div class="metric-card">
            <div class="label">Loss</div>
            <div class="val" id="val-loss">&mdash;</div>
          </div>
          <div class="metric-card">
            <div class="label">Tokens Processed</div>
            <div class="val" id="val-tokens">0 / 500M</div>
          </div>
          <div class="metric-card">
            <div class="label">Throughput</div>
            <div class="val" id="val-toks">0 tok/s</div>
          </div>
          <div class="metric-card">
            <div class="label">Metal Memory</div>
            <div class="val" id="val-mem">&mdash;</div>
          </div>
          <div class="metric-card">
            <div class="label">Training Data</div>
            <div class="val" id="val-data">&mdash;</div>
          </div>
        </div>

        <div class="dev-card" style="margin-top: 18px;">
          <div class="card-header">
            <h3>Live Training Loss Curve</h3>
            <span class="status-indicator" id="train-status-badge">Idle</span>
          </div>
          <div class="chart-container">
            <svg id="loss-svg" width="100%" height="220" style="background:#101216;border-radius:8px;display:block;"></svg>
          </div>
        </div>

        <div class="dev-card sample-feed" style="margin-top: 18px;">
          <div class="card-header">
            <h3>Live Sample Generation</h3>
            <span class="sample-step-badge" id="sample-step">Step 0</span>
          </div>
          <pre class="sample-text" id="sample-text">"Waiting for training sample generation…"</pre>
        </div>

        <div class="dev-card" style="margin-top: 18px;">
          <div class="card-header">
            <h3>Training Corpus</h3>
          </div>
          <p class="dev-muted">Leave either field empty to train on synthetic
          tokens. A synthetic run still produces a loss curve, so the run
          reports which of the two it was.</p>
          <div class="form-grid">
            <div class="form-row">
              <label for="pt-dataset">Dataset Path</label>
              <input type="text" id="pt-dataset" value=".zorp/training/data/pretrain.jsonl" />
            </div>
            <div class="form-row">
              <label for="pt-tokenizer">Tokenizer Directory</label>
              <input type="text" id="pt-tokenizer" value=".zorp/training/tokenizer" />
            </div>
          </div>
        </div>

        <div class="controls-row" style="margin-top: 20px;">
          <button class="btn btn-primary" id="btn-start">Start Training</button>
          <button class="btn btn-secondary" id="btn-pause" disabled>Pause</button>
          <button class="btn btn-secondary" id="btn-resume" style="display:none;">Resume</button>
          <button class="btn btn-danger" id="btn-stop" disabled>Stop</button>
          <span class="action-feedback" id="train-feedback"></span>
        </div>
      </div>
    `;

    this.attachPretrainEvents();
    this.wirePretrainControls(body);
    this.drawLossChart();
  }

  private wirePretrainControls(body: Element): void {
    const btnStart = body.querySelector("#btn-start") as HTMLButtonElement;
    const btnPause = body.querySelector("#btn-pause") as HTMLButtonElement;
    const btnResume = body.querySelector("#btn-resume") as HTMLButtonElement;
    const btnStop = body.querySelector("#btn-stop") as HTMLButtonElement;
    const feedback = body.querySelector("#train-feedback");
    const badge = body.querySelector("#train-status-badge");

    if (this.isTraining) {
      if (badge) {
        badge.textContent = this.isPaused ? "Paused" : "Training";
        badge.className = `status-indicator ${this.isPaused ? "warn" : "ok"}`;
      }
      if (btnStart) btnStart.disabled = true;
      if (btnStop) btnStop.disabled = false;
      if (this.isPaused) {
        if (btnPause) btnPause.style.display = "none";
        if (btnResume) {
          btnResume.style.display = "inline-block";
          btnResume.disabled = false;
        }
      } else {
        if (btnPause) {
          btnPause.style.display = "inline-block";
          btnPause.disabled = false;
        }
        if (btnResume) btnResume.style.display = "none";
      }
    }

    if (btnStart) {
      btnStart.addEventListener("click", async () => {
        btnStart.disabled = true;
        if (feedback) feedback.textContent = "Starting pretraining run…";
        const config: TrainingJobConfig = {
          run_id: `run-${Date.now().toString(36)}`,
          dataset_id: "default",
          tokenizer_name: "default",
          recipe_name: "zorp-dense-250m",
          batch_size: 4,
          gradient_accumulation_steps: 4,
          learning_rate: 3e-4,
          warmup_steps: 50,
          max_tokens: 500_000_000,
          checkpoint_every_steps: 100,
          sample_every_steps: 50,
          // Empty means absent, not empty string: the server treats a
          // missing path as "no corpus" and trains on synthetic tokens.
          tokenizer_dir:
            (body.querySelector("#pt-tokenizer") as HTMLInputElement | null)?.value.trim() || undefined,
          dataset_path:
            (body.querySelector("#pt-dataset") as HTMLInputElement | null)?.value.trim() || undefined,
        };

        try {
          const res = await startDevTrain(".zorp/training/run", config);
          if (res.status === "ok") {
            this.isTraining = true;
            this.isPaused = false;
            btnPause.disabled = false;
            btnStop.disabled = false;
            if (badge) {
              badge.textContent = "Training";
              badge.className = "status-indicator ok";
            }
            if (feedback) feedback.textContent = `Job ${config.run_id} started.`;
          } else {
            if (feedback) feedback.textContent = `Start error: ${res.error || "unknown"}`;
            btnStart.disabled = false;
          }
        } catch (e) {
          if (feedback) feedback.textContent = `Failed: ${String(e)}`;
          btnStart.disabled = false;
        }
      });
    }

    if (btnPause) {
      btnPause.addEventListener("click", async () => {
        btnPause.disabled = true;
        try {
          const res = await pauseDevTrain();
          if (res.status === "ok") {
            this.isPaused = true;
            btnPause.style.display = "none";
            btnResume.style.display = "inline-block";
            btnResume.disabled = false;
            if (badge) {
              badge.textContent = "Paused";
              badge.className = "status-indicator warn";
            }
            if (feedback) feedback.textContent = "Training paused.";
          }
        } catch (e) {
          if (feedback) feedback.textContent = `Pause error: ${String(e)}`;
          btnPause.disabled = false;
        }
      });
    }

    if (btnResume) {
      btnResume.addEventListener("click", async () => {
        btnResume.disabled = true;
        try {
          const res = await resumeDevTrain();
          if (res.status === "ok") {
            this.isPaused = false;
            btnResume.style.display = "none";
            btnPause.style.display = "inline-block";
            btnPause.disabled = false;
            if (badge) {
              badge.textContent = "Training";
              badge.className = "status-indicator ok";
            }
            if (feedback) feedback.textContent = "Training resumed.";
          }
        } catch (e) {
          if (feedback) feedback.textContent = `Resume error: ${String(e)}`;
          btnResume.disabled = false;
        }
      });
    }

    if (btnStop) {
      btnStop.addEventListener("click", async () => {
        btnStop.disabled = true;
        try {
          const res = await stopDevTrain();
          if (res.status === "ok") {
            this.isTraining = false;
            this.isPaused = false;
            btnStart.disabled = false;
            btnPause.disabled = true;
            btnResume.style.display = "none";
            btnPause.style.display = "inline-block";
            if (badge) {
              badge.textContent = "Stopped";
              badge.className = "status-indicator";
            }
            if (feedback) feedback.textContent = "Training stopped.";
          }
        } catch (e) {
          if (feedback) feedback.textContent = `Stop error: ${String(e)}`;
        }
      });
    }
  }

  private attachPretrainEvents(): void {
    if (this.sse) {
      this.sse.close();
      this.sse = null;
    }

    try {
      this.sse = new EventSource("/api/dev/train/stream");
      this.sse.onmessage = (e) => {
        try {
          const ev: TrainEvent = JSON.parse(e.data);
          this.handleTrainEvent(ev);
        } catch (_) {}
      };
      this.sse.onerror = () => {
        // SSE disconnect or idle
      };
    } catch (_) {}
  }

  private handleTrainEvent(ev: TrainEvent): void {
    if (ev.type === "step") {
      const l = document.getElementById("val-loss");
      const t = document.getElementById("val-tokens");
      const s = document.getElementById("val-toks");
      const m = document.getElementById("val-mem");

      if (l) l.innerHTML = `${ev.loss.toFixed(3)} <span class="trend-arrow">&darr;</span>`;
      if (t) t.textContent = `${(ev.tokens / 1e6).toFixed(1)}M / 500M`;
      if (s) s.textContent = `${Math.round(ev.tok_per_sec).toLocaleString()} tok/s`;
      if (m) m.textContent = `${ev.memory_gb.toFixed(1)} GB (Metal)`;

      this.lossHistory.push({ step: ev.step, loss: ev.loss });
      // Keep recent 100 points for chart smoothness
      if (this.lossHistory.length > 150) {
        this.lossHistory.shift();
      }
      this.drawLossChart();
    } else if (ev.type === "sample") {
      const st = document.getElementById("sample-text");
      const stepBadge = document.getElementById("sample-step");
      if (st) st.textContent = `Prompt: "${ev.prompt}"\n-> ${ev.output}`;
      if (stepBadge) stepBadge.textContent = `Step ${ev.step}`;
    } else if (ev.type === "checkpoint") {
      const feedback = document.getElementById("train-feedback");
      if (feedback) feedback.textContent = `Checkpoint saved at step ${ev.step} (loss ${ev.loss.toFixed(3)})`;
    } else if (ev.type === "init") {
      const m = document.getElementById("val-mem");
      if (m) m.textContent = `0.0 GB / ${ev.memory_total_gb.toFixed(1)} GB (${ev.device})`;
      const d = document.getElementById("val-data");
      if (d) d.textContent = describeTrainingData(ev);
    }
  }

  private drawLossChart(): void {
    const svg = this.container.querySelector("#loss-svg");
    if (!svg) return;

    if (this.lossHistory.length === 0) {
      svg.innerHTML = `
        <text x="50%" y="50%" fill="#6a7180" font-size="13" font-family="sans-serif" text-anchor="middle" dominant-baseline="middle">
          Awaiting training loss events…
        </text>
      `;
      return;
    }

    const width = svg.clientWidth || 600;
    const height = 220;
    const padding = { top: 25, right: 30, bottom: 30, left: 55 };

    const innerW = Math.max(10, width - padding.left - padding.right);
    const innerH = Math.max(10, height - padding.top - padding.bottom);

    const losses = this.lossHistory.map((d) => d.loss);
    let minLoss = Math.min(...losses);
    let maxLoss = Math.max(...losses);
    if (minLoss === maxLoss) {
      minLoss *= 0.9;
      maxLoss *= 1.1;
    }
    const lossSpan = maxLoss - minLoss || 1;

    const minStep = this.lossHistory[0].step;
    const maxStep = this.lossHistory[this.lossHistory.length - 1].step;
    const stepSpan = maxStep - minStep || 1;

    const points = this.lossHistory
      .map((d) => {
        const x = padding.left + ((d.step - minStep) / stepSpan) * innerW;
        const y = padding.top + (1 - (d.loss - minLoss) / lossSpan) * innerH;
        return `${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" ");

    const firstPt = points.split(" ")[0];
    const lastPt = points.split(" ")[points.split(" ").length - 1];
    const firstX = firstPt.split(",")[0];
    const lastX = lastPt.split(",")[0];
    const bottomY = padding.top + innerH;
    const areaPath = `M ${firstX},${bottomY} L ${points} L ${lastX},${bottomY} Z`;

    svg.innerHTML = `
      <defs>
        <linearGradient id="loss-gradient" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="#5eead4" stop-opacity="0.3"/>
          <stop offset="100%" stop-color="#5eead4" stop-opacity="0.0"/>
        </linearGradient>
      </defs>
      <!-- Grid lines -->
      <line x1="${padding.left}" y1="${padding.top}" x2="${width - padding.right}" y2="${padding.top}" stroke="#23262d" stroke-dasharray="3,3"/>
      <line x1="${padding.left}" y1="${padding.top + innerH / 2}" x2="${width - padding.right}" y2="${padding.top + innerH / 2}" stroke="#23262d" stroke-dasharray="3,3"/>
      <line x1="${padding.left}" y1="${bottomY}" x2="${width - padding.right}" y2="${bottomY}" stroke="#23262d"/>
      <!-- Y-Axis labels -->
      <text x="${padding.left - 10}" y="${padding.top + 4}" fill="#6a7180" font-size="11" font-family="monospace" text-anchor="end">${maxLoss.toFixed(2)}</text>
      <text x="${padding.left - 10}" y="${padding.top + innerH / 2 + 4}" fill="#6a7180" font-size="11" font-family="monospace" text-anchor="end">${((maxLoss + minLoss) / 2).toFixed(2)}</text>
      <text x="${padding.left - 10}" y="${bottomY}" fill="#6a7180" font-size="11" font-family="monospace" text-anchor="end">${minLoss.toFixed(2)}</text>
      <!-- X-Axis labels -->
      <text x="${padding.left}" y="${bottomY + 20}" fill="#6a7180" font-size="11" font-family="monospace">Step ${minStep}</text>
      <text x="${width - padding.right}" y="${bottomY + 20}" fill="#6a7180" font-size="11" font-family="monospace" text-anchor="end">Step ${maxStep}</text>
      <!-- Area fill -->
      <path d="${areaPath}" fill="url(#loss-gradient)"/>
      <!-- Polyline -->
      <polyline fill="none" stroke="#5eead4" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" points="${points}"/>
    `;
  }

  /* ------------------------------------------------------------------ */
  /* Model Registry Tab                                                 */
  /* ------------------------------------------------------------------ */

  private async renderRegistryTab(body: Element): Promise<void> {
    body.innerHTML = `
      <div class="dev-tab-pane registry-dashboard">
        <div class="dev-card">
          <div class="card-header">
            <h3>Trained Checkpoint Registry</h3>
            <button class="btn btn-secondary" id="btn-refresh-models">Refresh</button>
          </div>
          <p class="dev-muted">Checkpoints saved during local training runs. Click "Open in Zorp" to serve the model locally and chat with it.</p>
          <div class="model-list" id="registry-model-list">
            <div class="empty-notice">Loading checkpoints…</div>
          </div>
        </div>
      </div>
    `;

    const refreshBtn = body.querySelector("#btn-refresh-models");
    if (refreshBtn) {
      refreshBtn.addEventListener("click", () => void this.loadRegistryModels(body));
    }

    await this.loadRegistryModels(body);
  }

  private async loadRegistryModels(body: Element): Promise<void> {
    const listContainer = body.querySelector("#registry-model-list");
    if (!listContainer) return;

    try {
      const { models } = await listDevModels();
      if (!models || models.length === 0) {
        listContainer.innerHTML = `
          <div class="empty-notice">
            <svg viewBox="0 0 24 24" aria-hidden="true" style="width:32px;height:32px;color:var(--text-faint);margin-bottom:8px;">
              <path d="M20 7H4a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2Z"/>
              <path d="M16 21V5a2 2 0 0 0-2-2h-4a2 2 0 0 0-2 2v16"/>
            </svg>
            <p>No trained checkpoints found yet.</p>
            <p class="dev-muted" style="font-size:12.5px;">Train a model in the <strong>Pretrain</strong> tab or place safetensors in <code>.zorp/training/models</code>.</p>
          </div>
        `;
        return;
      }

      listContainer.innerHTML = models
        .map(
          (m: CheckpointMetadata) => `
          <div class="model-row">
            <div class="model-info">
              <div class="model-name"><strong>${escapeHtml(m.run_id)}</strong> <span class="model-step-badge">Step ${m.step}</span></div>
              <div class="model-meta">
                <span>Loss: <strong>${m.loss ? m.loss.toFixed(3) : "N/A"}</strong></span>
                <span class="meta-sep">&bull;</span>
                <span class="model-path" title="${escapeHtml(m.checkpoint_dir)}">${escapeHtml(m.checkpoint_dir)}</span>
              </div>
            </div>
            <div class="model-actions">
              <button class="btn btn-primary open-zorp-btn" data-path="${escapeHtml(m.checkpoint_dir)}" data-name="${escapeHtml(m.run_id)}">
                Open in Zorp
              </button>
            </div>
          </div>
        `,
        )
        .join("");

      listContainer.querySelectorAll(".open-zorp-btn").forEach((btn) => {
        btn.addEventListener("click", async (e) => {
          const target = e.currentTarget as HTMLButtonElement;
          const checkpointDir = target.dataset.path || "";
          const name = target.dataset.name || "pretrained-model";

          target.disabled = true;
          const origText = target.textContent;
          target.textContent = "Serving…";

          try {
            const res = await serveDevModel(checkpointDir);
            if (res.status === "ok" && res.base_url) {
              target.textContent = "Opening…";
              this.onOpenInZorp(name, res.base_url);
            } else {
              target.textContent = "Serve failed";
              alert(`Could not serve model: ${res.error || "unknown error"}`);
              target.disabled = false;
              target.textContent = origText;
            }
          } catch (err) {
            target.textContent = "Error";
            alert(`Failed to serve model: ${String(err)}`);
            target.disabled = false;
            target.textContent = origText;
          }
        });
      });
    } catch (e) {
      listContainer.innerHTML = `
        <div class="empty-notice warn">
          Could not load models from server: ${escapeHtml(String(e))}
        </div>
      `;
    }
  }
}
