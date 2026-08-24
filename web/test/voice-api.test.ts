import assert from "node:assert/strict";
import test from "node:test";
import { readVoiceWaitStream, voiceWaitRequest } from "../src/voice-readiness.ts";

test("voice readiness uses a non-simple JSON post and requires ready", async () => {
  const response = new Response(
    'event: voice_model\ndata: {"status":"waiting","model":"qwen","detail":"starting"}\n\n' +
      'event: voice_model\ndata: {"status":"ready","model":"qwen","detail":"ready"}\n\n',
  );
  const events: string[] = [];
  await readVoiceWaitStream(response.body!, (event) => events.push(event.status));
  assert.deepEqual(events, ["waiting", "ready"]);
  const request = voiceWaitRequest("");
  assert.equal(request.method, "POST");
  assert.equal((request.headers as Record<string, string>)["content-type"], "application/json");
  assert.equal(request.body, "{}");
});

test("voice readiness rejects a stream that ends without a terminal event", async () => {
  const response = new Response(
    'event: voice_model\ndata: {"status":"waiting","model":"qwen","detail":"starting"}\n\n',
  );
  await assert.rejects(readVoiceWaitStream(response.body!, () => {}), /ended before.*ready or error/i);
});
