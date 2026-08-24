import assert from "node:assert/strict";
import test from "node:test";
import { readVoiceWaitStream, voiceWaitRequest } from "../src/voice-readiness.ts";

test("voice readiness uses a non-simple JSON post and requires ready", async () => {
  const response = new Response(
    'event: voice_model\ndata: {"status":"waiting","stage":"creating_environment","model":"qwen","detail":"one"}\n\n' +
      'event: voice_model\ndata: {"status":"waiting","stage":"installing","model":"qwen","detail":"two"}\n\n' +
      'event: voice_model\ndata: {"status":"waiting","stage":"downloading_model","model":"qwen","detail":"three"}\n\n' +
      'event: voice_model\ndata: {"status":"waiting","stage":"loading","model":"qwen","detail":"four"}\n\n' +
      'event: voice_model\ndata: {"status":"ready","stage":"ready","model":"qwen","detail":"five"}\n\n',
  );
  const events: string[] = [];
  await readVoiceWaitStream(response.body!, (event) => events.push(event.stage));
  assert.deepEqual(events, [
    "creating_environment",
    "installing",
    "downloading_model",
    "loading",
    "ready",
  ]);
  const request = voiceWaitRequest("");
  assert.equal(request.method, "POST");
  assert.equal((request.headers as Record<string, string>)["content-type"], "application/json");
  assert.equal(request.body, "{}");
});

test("voice readiness rejects a stream that ends without a terminal event", async () => {
  const response = new Response(
    'event: voice_model\ndata: {"status":"waiting","stage":"loading","model":"qwen","detail":"starting"}\n\n',
  );
  await assert.rejects(readVoiceWaitStream(response.body!, () => {}), /ended before.*ready or error/i);
});

test("voice readiness rejects an event without a known stage", async () => {
  for (const frame of [
    '{"status":"waiting","model":"qwen","detail":"missing"}',
    '{"status":"waiting","stage":"invented","model":"qwen","detail":"wrong"}',
  ]) {
    const response = new Response(`event: voice_model\ndata: ${frame}\n\n`);
    await assert.rejects(readVoiceWaitStream(response.body!, () => {}), /invalid event/i);
  }
});
