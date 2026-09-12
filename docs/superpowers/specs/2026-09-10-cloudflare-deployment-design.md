# Cloudflare Deployment for Zorp Web

Date: 2026-09-10. Status: proposed design.

zorp's web interface consists of two distinct components: a static chat UI (`web/`) built in TypeScript with esbuild, and an agent backend server (`zorp-web`) written in Rust using Axum and Tokio. The agent executes shell commands, inspects and writes files in a disk workspace, queries SQLite and DuckDB databases, and streams live activity to the browser via Server-Sent Events (SSE).

Because standard Cloudflare Workers execute within sandboxed V8 WebAssembly isolates without OS process spawning or a local POSIX filesystem, the Rust agent backend cannot run directly as a serverless Worker function. Furthermore, Cloudflare Containers requires a Workers Paid plan which is not enabled on this account.

This design presents a production-grade internal staging deployment using the Cloudflare stack:
1. **Cloudflare Worker (`web/worker.ts`)**: Deployed on Cloudflare Workers, serving the static frontend assets from Cloudflare's global edge cache and acting as a single-origin reverse proxy for `/api/*` and SSE streams.
2. **Cloudflare Tunnel (`cloudflared`)**: Running alongside `zorp-web` on an internal staging host or VM, securely exposing port `7777` over an outbound-only encrypted tunnel without opening inbound firewall ports.
3. **Cloudflare Access (Zero Trust)**: Protecting the staging endpoint with team authentication (One-Time PIN / Google / GitHub) before requests reach the application.

---

## Architecture Diagram

```
                              Team Member (Browser)
                                        │
                                        ▼
                   ┌────────────────────────────────────────┐
                   │  Cloudflare Access (Zero Trust Login)  │
                   │   - Policy: Allowed Team Emails / OTP  │
                   └──────────────────┬─────────────────────┘
                                      │ (Authenticated)
                                      ▼
                   ┌────────────────────────────────────────┐
                   │        Cloudflare Worker (Edge)        │
                   │                                        │
                   │  ├── Static Routes (/, styles.css, …)  │
                   │  │     └─► env.ASSETS (dist-site)      │
                   │  │                                     │
                   │  └── API Routes (/api/*)               │
                   │        ├─► Attach ZORP_WEB_TOKEN       │
                   │        └─► Stream SSE (no buffer)      │
                   └──────────────────┬─────────────────────┘
                                      │ (TLS Outbound Proxy)
                                      ▼
                   ┌────────────────────────────────────────┐
                   │    Cloudflare Tunnel (cloudflared)     │
                   │   zorp-backend-internal.yourdomain.com │
                   └──────────────────┬─────────────────────┘
                                      │ (Internal loopback:7777)
                                      ▼
                   ┌────────────────────────────────────────┐
                   │       Internal Staging Machine         │
                   │                                        │
                   │  ┌──────────────────────────────────┐  │
                   │  │  zorp-web Rust Server (:7777)     │  │
                   │  │  - Shell & file tools             │  │
                   │  │  - SQLite (sessions.db)           │  │
                   │  │  - Workspace directory            │  │
                   │  └──────────────────────────────────┘  │
                   │                     │                  │
                   │                     ▼                  │
                   │          LLM Endpoint (API / Ollama)   │
                   └────────────────────────────────────────┘
```

---

## Components

### 1. Cloudflare Worker (`web/worker.ts` & `web/wrangler.jsonc`)

The Worker acts as a unified edge entrypoint:

* **Static Asset Delivery**: Static requests are served directly through Cloudflare Workers Static Assets (`env.ASSETS.fetch(request)`), caching `index.html`, `styles.css`, `dist/main.js`, and fonts globally.
* **API Proxying**: Requests matching `/api/*` are rewritten to the backend tunnel origin (`env.BACKEND_URL`).
* **Header & Secret Injection**: The Worker automatically injects `Authorization: Bearer <ZORP_WEB_TOKEN>` from Cloudflare encrypted secrets. The client browser never receives or stores the API token.
* **Unbuffered SSE Streaming**: `/api/sessions/:id/events` streams real-time token generation and tool progress. The Worker forwards the upstream `fetch()` response directly without buffering chunks or modifying Content-Type.
* **CORS Elimination**: Because the UI and API share the Worker's hostname, cross-origin restrictions are completely eliminated.

#### Worker Handler Logic (`web/worker.ts`)
```typescript
interface Env {
  ASSETS: Fetcher;
  BACKEND_URL: string;
  ZORP_WEB_TOKEN?: string;
}

export default {
  async fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    const url = new URL(request.url);

    // Route API requests to the backend tunnel
    if (url.pathname.startsWith("/api/")) {
      const backendUrl = new URL(url.pathname + url.search, env.BACKEND_URL);
      const headers = new Headers(request.headers);

      // Securely attach backend authentication token
      if (env.ZORP_WEB_TOKEN) {
        headers.set("Authorization", `Bearer ${env.ZORP_WEB_TOKEN}`);
      }

      // Preserve client IP and host context
      headers.set("X-Forwarded-Host", url.host);
      headers.set("X-Forwarded-Proto", url.protocol.replace(":", ""));

      const upstreamRequest = new Request(backendUrl.toString(), {
        method: request.method,
        headers,
        body: ["GET", "HEAD"].includes(request.method) ? undefined : request.body,
        redirect: "manual",
      });

      return fetch(upstreamRequest);
    }

    // Serve static assets for all other routes
    return env.ASSETS.fetch(request);
  },
};
```

#### Wrangler Configuration (`web/wrangler.jsonc`)
```jsonc
{
  "$schema": "node_modules/wrangler/config-schema.json",
  "name": "zorp-ui",
  "main": "worker.ts",
  "compatibility_date": "2026-08-17",
  "assets": {
    "directory": "./dist-site",
    "binding": "ASSETS"
  },
  "vars": {
    "BACKEND_URL": "https://zorp-backend-internal.yourdomain.com"
  }
}
```

---

### 2. Backend Agent Host (`zorp-web`)

The backend server runs on an internal server, staging VM, or container host:

* **Execution Environment**: A dedicated directory for the agent workspace (e.g. `/var/zorp/workspace`).
* **Storage**: Local state database (`sessions.db`).
* **Inference Endpoint**: Configured via `ZORP_BASE_URL`, `ZORP_MODEL`, and `ZORP_API_KEY` pointing to your LLM provider (OpenAI, Anthropic, or an internal Ollama gateway).
* **Launch Command (Native)**:
  ```bash
  ZORP_WEB_TOKEN="<random-secure-hex>" \
  ZORP_WORKSPACE="/var/zorp/workspace" \
  ZORP_BASE_URL="https://api.openai.com/v1" \
  ZORP_MODEL="gpt-4o" \
  ZORP_API_KEY="sk-..." \
  ./target/release/zorp-web --bind 127.0.0.1 --port 7777 --token "$ZORP_WEB_TOKEN"
  ```
* **Or via Docker Compose**:
  The existing `compose.yml` `server` service running with `ZORP_WEB_TOKEN` set and port `7777` bound to `127.0.0.1:7777`.

---

### 3. Cloudflare Tunnel (`cloudflared`)

Cloudflare Tunnel creates an encrypted outbound tunnel from the staging machine to Cloudflare's edge:

1. **Install `cloudflared`** on the staging host:
   ```bash
   brew install cloudflared # macOS
   # or: apt-get install cloudflared / docker run cloudflare/cloudflared
   ```
2. **Tunnel Authentication**:
   Authenticate via `cloudflared tunnel login` or configure via the Cloudflare Zero Trust Dashboard under **Networks > Tunnels**.
3. **Tunnel Ingress Rule**:
   Map the internal hostname (e.g., `zorp-backend-internal.yourdomain.com`) to `http://localhost:7777`.
4. **Daemon Execution**:
   Run `cloudflared tunnel run <tunnel-name>` as a systemd service or background process.

---

### 4. Cloudflare Access (Zero Trust Authentication)

To ensure the staging deployment remains private without implementing application-level user authentication:

1. In Cloudflare Zero Trust Dashboard, navigate to **Access > Applications**.
2. Add a Self-Hosted Application:
   - **Application Name**: `Zorp Web Staging`
   - **Domain**: `zorp-ui.<subdomain>.workers.dev` (or your staging subdomain `stage.zorp.dev`).
3. Configure Policy:
   - **Action**: Allow
   - **Rule**: Include emails ending in `@yourcompany.com` or specific test emails.
   - **Identity Provider**: One-Time PIN (default), Google, or GitHub.
4. When any team member navigates to the URL, Cloudflare intercepts the request, prompts for email authentication / SSO, and passes the authenticated session cookie through.

---

## Deployment Workflow

### Step 1: Prepare the Frontend & Worker
1. Create `web/worker.ts` with the asset and reverse proxy logic.
2. Update `web/wrangler.jsonc` to set `main: "worker.ts"`, asset binding `ASSETS`, and placeholder `BACKEND_URL`.
3. Add a TypeScript compilation check in `web/package.json` to verify the Worker types alongside the frontend code.

### Step 2: Establish the Cloudflare Tunnel
1. On the staging host, configure `cloudflared` to expose `http://127.0.0.1:7777`.
2. Verify connectivity by querying the tunnel hostname with `curl -i https://<tunnel-hostname>/api/capabilities`.

### Step 3: Deploy the Worker
1. Build frontend assets: `npm run build:site` inside `web/`.
2. Configure Worker secret:
   ```bash
   npx wrangler secret put ZORP_WEB_TOKEN
   ```
3. Deploy Worker:
   ```bash
   npx wrangler deploy
   ```

### Step 4: Protect with Cloudflare Access
1. Add the deployed Worker URL to Cloudflare Access in the Zero Trust dashboard.
2. Verify that unauthenticated browser visits trigger the Cloudflare Access login screen.

---

## Verification & Testing Checklist

1. **Static Delivery**: Navigate to the Worker URL; verify that `index.html`, stylesheets, and JavaScript bundle load with HTTP 200.
2. **API Handshake**: Open browser dev tools; verify `/api/capabilities` returns HTTP 200 with model and tool availability without CORS errors.
3. **Session Creation**: Create a new session in the UI; verify `POST /api/sessions` succeeds.
4. **SSE Streaming**: Send a prompt; verify that `/api/sessions/:id/events` streams `working`, `tool`, `assistant`, and `done` frames incrementally in real time.
5. **Approval Cards**: Run a tool requiring approval (e.g. file edit); verify approval card renders and approval submission works over the proxy.
6. **Access Gate**: Test in an incognito window without auth cookies; confirm Cloudflare Access blocks access until authenticated.
