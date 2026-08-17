# zorp web UI

A chat interface for the zorp agent. Plain TypeScript, no framework, bundled
with esbuild into a single file. It talks to the `zorp-web` server over the
documented HTTP API, so the two are separate artifacts and can be served from
separate places.

## Safety

**This UI is only as safe as the server it points at.** It is a view. The
server behind it constructs a real agent that reads and writes files and runs
commands on the machine it was started on, in the directory it was started in.
Pointing this page at a `zorp-web` you do not control hands that page's
approvals to whoever runs it.

Approvals are the boundary. When the agent reaches a tool that needs one, the
turn stops and an approval card appears with the tool name and its arguments.
Nothing in this UI answers that card for you. There is no auto approve, no
remembered decision, and no way to skip it.

## Build

```bash
cd web
npm install
npm run build
```

That writes `dist/main.js`. The page is `index.html`, and the files it needs at
runtime are:

```
index.html
styles.css
dist/main.js
```

Nothing else has to be deployed. There is no runtime dependency, no CDN fetch,
and no server-side rendering, so any static host will do.

Other scripts:

| Script | What it does |
|---|---|
| `npm run build` | One bundle to `dist/main.js` |
| `npm run build:min` | The same, minified, with a source map |
| `npm run watch` | Rebuild on save |
| `npm run serve` | Rebuild on save and serve the directory on esbuild's dev port |
| `npm run check` | Typecheck with `tsc --noEmit`. esbuild does not typecheck |

## Running it

Start the server, then open the page.

```bash
cargo run -p zorp-web            # serves on http://127.0.0.1:7777
```

If the server serves these files itself, open its address and you are done. The
UI defaults to talking to its own origin.

You can also open `web/index.html` straight off disk. A page loaded over
`file:` has no origin to call, so it falls back to `http://127.0.0.1:7777`.
That is a cross origin request, so the server has to send permissive CORS
headers for it to work.

## Pointing it at a server on another origin

Set `window.ZORP_API_BASE` before the bundle loads. There is a commented block
in `index.html` for exactly this:

```html
<script>
  window.ZORP_API_BASE = "https://zorp.internal.example:7777";
</script>
```

Then serve `index.html`, `styles.css`, and `dist/` from wherever you like: an
nginx container, a static bucket, Cloudflare Pages. The server needs to allow
that origin in its CORS headers.

The server itself cannot move to a Workers style runtime. It needs a filesystem
and the ability to spawn processes, which is the whole point of the agent. The
UI is what is portable.

If the server was started with `--token`, set that too:

```html
<script>
  window.ZORP_API_TOKEN = "the token you passed to zorp-web";
</script>
```

The token goes out as an `Authorization: Bearer` header on ordinary requests
and as a `token` query parameter on every request including the event stream,
because `EventSource` cannot set headers.

Remember what `--token` is for. A `zorp-web` reachable on a network interface
is agent driven shell access to the machine it runs on.

## How it is put together

| File | What lives there |
|---|---|
| `src/api.ts` | Typed client for every endpoint, plus `streamEvents` over `EventSource`. Exports the event union |
| `src/main.ts` | The UI. Message list, composer, sidebar, activity lines, approval cards |
| `styles.css` | One hand written stylesheet. No framework |
| `index.html` | The shell, and the config block for `ZORP_API_BASE` |

### Events

The server streams one JSON object per SSE frame, each with a numeric `seq` and
a `type`. The UI renders them like this:

| Event | Rendered as |
|---|---|
| `working`, `working_done` | The in progress indicator under the transcript |
| `tool` | An activity line: a bullet, the tool name, the summary. Same shape the CLI prints |
| `verify` | An activity line ending in `passed` or `failed` |
| `notice` | A dim activity line |
| `assistant` | A message block. Fenced code blocks and inline code are picked out |
| `approval_request` | The approval card |
| `error` | An error block in the transcript. Never swallowed |
| `done` | Ends the turn |

Every frame's SSE event id is its `seq`, so a dropped connection resumes with
`Last-Event-ID` and `EventSource` handles the retry itself. Frames at or below
the sequence number already seen are dropped, which keeps a generous replay
from doubling the transcript.

When you open a session from the sidebar, its stored messages are read from
`GET /api/sessions/:id` and the event stream's replay is buffered for a moment
first. A replay that ends in `done` describes a turn that already finished, so
it is discarded because the stored transcript covers it. A replay that does not
end in `done` is a turn still in flight, so it is rendered and the UI joins it.
That is what lets you reload the page mid turn and keep watching.

### Rendering and escaping

Everything from the server reaches the page through `textContent`. No server
value is ever assigned to `innerHTML`, so model output and tool summaries cannot
become markup.

## What is deliberately not here

- **No framework.** A message list, a composer, and an event stream do not need
  one, and this repository prefers a short dependency list. Revisit if the UI
  grows a track browser or an artifact viewer, which is a different UI anyway.
- **No markdown renderer.** Fenced code and inline code are handled by hand.
  Anything more wants a dependency and a sanitiser.
- **No test framework.** The behavior lives in the API, which is tested on the
  server side.
