"""Small loopback-only Qwen3-ASR server embedded by zorp-voice."""

import argparse
import base64
import binascii
import io
import ipaddress
import threading
import traceback

from flask import Flask, jsonify, request


def loopback_host(value):
    if value == "localhost":
        return "127.0.0.1"
    try:
        address = ipaddress.ip_address(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("host must be a loopback IP literal or localhost") from error
    if not address.is_loopback:
        raise argparse.ArgumentTypeError("host must be loopback")
    return str(address)


parser = argparse.ArgumentParser()
parser.add_argument("--model", required=True)
parser.add_argument("--host", required=True, type=loopback_host)
parser.add_argument("--port", required=True, type=int)
args = parser.parse_args()

app = Flask(__name__)
state_lock = threading.Lock()
inference_lock = threading.Lock()
state = {
    "stage": "downloading_model",
    "detail": "Downloading the local speech model.",
    "model": None,
}
ASRTranscription = None


def set_state(stage, model=None, detail=None):
    with state_lock:
        state["stage"] = stage
        if model is not None:
            state["model"] = model
        if detail is not None:
            state["detail"] = detail


def load_model():
    global ASRTranscription
    try:
        from huggingface_hub import snapshot_download
        from qwen_asr import Qwen3ASRModel
        from qwen_asr.inference.qwen3_asr import ASRTranscription as ResultType

        model_path = snapshot_download(args.model)
        set_state("loading", detail="Loading the local speech model.")
        loaded = Qwen3ASRModel.from_pretrained(model_path)
        ASRTranscription = ResultType
        set_state("ready", loaded, "Local speech recognition is ready.")
    except Exception as error:
        traceback.print_exc()
        set_state("error", detail=str(error))


@app.get("/health")
def health():
    with state_lock:
        stage = state["stage"]
        detail = state["detail"]
    return jsonify({"status": stage, "detail": detail})


@app.get("/v1/models")
def models():
    with state_lock:
        ready = state["stage"] == "ready"
    return jsonify({"data": [{"id": args.model}] if ready else []})


def normalize(array):
    import numpy as np

    if np.issubdtype(array.dtype, np.integer):
        limits = np.iinfo(array.dtype)
        return array.astype(np.float32) / float(limits.max)
    return array.astype(np.float32)


def mix_to_mono(array, channels=None):
    import numpy as np

    if array.ndim == 1:
        return array
    if channels and array.shape[0] == channels:
        return np.mean(array, axis=0, dtype=np.float32)
    if channels and array.shape[-1] == channels:
        return np.mean(array, axis=-1, dtype=np.float32)
    return np.mean(array, axis=0, dtype=np.float32)


def decode_audio(raw):
    import librosa
    import numpy as np
    import soundfile

    try:
        samples, sample_rate = soundfile.read(io.BytesIO(raw), dtype="float32", always_2d=False)
        samples = mix_to_mono(np.asarray(samples, dtype=np.float32))
    except Exception:
        import av

        chunks = []
        sample_rate = None
        with av.open(io.BytesIO(raw)) as container:
            for frame in container.decode(audio=0):
                sample_rate = frame.sample_rate or sample_rate
                channels = len(frame.layout.channels) if frame.layout else None
                chunks.append(mix_to_mono(normalize(frame.to_ndarray()), channels))
        if not chunks or sample_rate is None:
            raise ValueError("the recording contained no decodable audio")
        samples = np.concatenate(chunks)
    if samples.size == 0:
        raise ValueError("the recording was empty")
    if sample_rate != 16000:
        samples = librosa.resample(samples, orig_sr=sample_rate, target_sr=16000)
    return np.asarray(samples, dtype=np.float32), 16000


def audio_from_request(body):
    messages = body.get("messages")
    if not isinstance(messages, list) or len(messages) != 1:
        raise ValueError("one user message is required")
    message = messages[0]
    if message.get("role") != "user" or not isinstance(message.get("content"), list):
        raise ValueError("one user audio message is required")
    parts = message["content"]
    if len(parts) != 1 or parts[0].get("type") != "audio_url":
        raise ValueError("one audio_url part is required")
    audio_url = parts[0].get("audio_url")
    value = audio_url.get("url") if isinstance(audio_url, dict) else None
    if not isinstance(value, str) or not value.startswith("data:audio/"):
        raise ValueError("audio must be an audio data URL")
    header, separator, encoded = value.partition(",")
    if not separator or not header.endswith(";base64"):
        raise ValueError("audio must be base64 encoded")
    try:
        return base64.b64decode(encoded, validate=True)
    except (ValueError, binascii.Error) as error:
        raise ValueError("audio base64 is invalid") from error


def result_fields(result):
    item = result[0] if isinstance(result, (list, tuple)) else result
    if ASRTranscription is None or not isinstance(item, ASRTranscription):
        raise ValueError("Qwen3-ASR returned an unknown transcription type")
    language = item.language
    text = item.text
    if not isinstance(language, str) or not language.strip():
        raise ValueError("Qwen3-ASR returned no language")
    if not isinstance(text, str) or not text.strip():
        raise ValueError("Qwen3-ASR returned no transcript")
    return language.strip(), text.strip()


@app.post("/v1/chat/completions")
def chat_completions():
    with state_lock:
        stage = state["stage"]
        model = state["model"]
    if stage != "ready" or model is None:
        return jsonify({"error": "the local model is not ready"}), 503
    try:
        body = request.get_json(force=False, silent=False)
        if not isinstance(body, dict) or body.get("model") != args.model:
            raise ValueError("the configured model is required")
        samples, sample_rate = decode_audio(audio_from_request(body))
        audio = (samples, sample_rate)
        with inference_lock:
            result = model.transcribe(audio)
        language, text = result_fields(result)
        content = f"language {language}<asr_text>{text}"
        return jsonify({"choices": [{"message": {"role": "assistant", "content": content}}]})
    except Exception:
        traceback.print_exc()
        return jsonify({"error": "the local recording could not be transcribed"}), 400


threading.Thread(target=load_model, name="qwen3-asr-loader", daemon=True).start()
app.run(host=args.host, port=args.port, threaded=True, use_reloader=False)
