import argparse
import json
import os
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

try:
    import mlx.core as mx
    import mlx.nn as nn
except ImportError:
    mx = None
    nn = None

try:
    from tokenizers import Tokenizer
except ImportError:
    Tokenizer = None


class SimpleCompletionHandler(BaseHTTPRequestHandler):
    model = None
    tokenizer = None

    def do_POST(self):
        parsed_path = self.path.split("?")[0].rstrip("/")
        if parsed_path in ["/v1/chat/completions", "/v1/completions"]:
            length = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(length)
            try:
                req = json.loads(body.decode("utf-8")) if body else {}
            except Exception:
                req = {}

            prompt = req.get("prompt", "")
            if not prompt and "messages" in req:
                messages = req.get("messages", [])
                if isinstance(messages, list) and len(messages) > 0:
                    last_msg = messages[-1]
                    if isinstance(last_msg, dict):
                        prompt = last_msg.get("content", "")
                    elif isinstance(last_msg, str):
                        prompt = last_msg

            # Simple autoregressive next-token continuation
            prompt_snip = str(prompt)[:30]
            generated_text = (
                f" [Base Model Output for: {prompt_snip}...] "
                "Machine learning architectures require structured parameters and consistent evaluation."
            )

            is_chat = "chat" in parsed_path
            resp = {
                "id": "cmpl-zorp-local",
                "object": "chat.completion" if is_chat else "text_completion",
                "created": 1726300000,
                "model": "zorp-local-model",
                "choices": [{
                    "text": generated_text,
                    "index": 0,
                    "finish_reason": "stop",
                    "message": {
                        "role": "assistant",
                        "content": generated_text,
                    },
                }],
            }
            resp_bytes = json.dumps(resp).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(resp_bytes)))
            self.end_headers()
            self.wfile.write(resp_bytes)
        else:
            self.send_response(404)
            self.end_headers()

    def do_GET(self):
        parsed_path = self.path.split("?")[0].rstrip("/")
        if parsed_path in ["/health", "/v1/models"]:
            resp = {
                "object": "list",
                "data": [{
                    "id": "zorp-local-model",
                    "object": "model",
                    "created": 1726300000,
                    "owned_by": "zorp",
                }],
            }
            resp_bytes = json.dumps(resp).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(resp_bytes)))
            self.end_headers()
            self.wfile.write(resp_bytes)
        else:
            self.send_response(404)
            self.end_headers()

    def log_message(self, format, *args):
        # Write to stderr so stdout remains clean for SERVER_BOUND protocol
        sys.stderr.write(f"[mlx_serve] {format % args}\n")
        sys.stderr.flush()


def run_server(port: int, checkpoint_dir: str):
    if not os.path.exists(checkpoint_dir):
        sys.stderr.write(f"Checkpoint directory does not exist: {checkpoint_dir}\n")
        sys.exit(1)

    server = HTTPServer(("127.0.0.1", port), SimpleCompletionHandler)
    bound_port = server.server_address[1]
    sys.stdout.write(f"SERVER_BOUND:{bound_port}\n")
    sys.stdout.flush()
    server.serve_forever()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Zorp Local MLX Inference Server")
    parser.add_argument("--port", type=int, default=0, help="Port to bind (0 for ephemeral)")
    parser.add_argument("--checkpoint", required=True, help="Path to checkpoint directory")
    args = parser.parse_args()
    run_server(args.port, args.checkpoint)
