"""FastAF Flow's local MLX worker. One JSON request/response per line.

Inference runs with Hub/Transformers offline mode. Only an explicitly launched
--online --once worker may discover or download public model artifacts.
"""
from __future__ import annotations

import contextlib
import gc
import json
import os
from pathlib import Path
import re
import sys
import time

SYSTEM = (
    "You are a text normalizer for speech-to-text transcripts. The input begins "
    "with a control line specifying the styling, structure, and context settings; "
    "clean the transcript to match those settings and output only the cleaned text."
)
STYLING = ("casual", "semi-casual", "semi-formal", "formal")
STRUCTURE = ("prose", "lists")
CONTEXT = ("general", "email")
_CACHE: dict = {}
_OUTPUT = sys.stdout
ONLINE = "--online" in sys.argv


def emit(value):
    _OUTPUT.write(json.dumps(value, ensure_ascii=False) + "\n")
    _OUTPUT.flush()


def progress(message):
    emit({"event": "progress", "message": message})


def messages_for(text, styling="semi-formal", structure="prose", context="general"):
    if styling not in STYLING or structure not in STRUCTURE or context not in CONTEXT:
        raise ValueError("Unsupported S1-mini style, structure, or context.")
    control = f"[Styling: {styling}] [Structure: {structure}] [Context: {context}]"
    return [{"role": "system", "content": SYSTEM},
            {"role": "user", "content": control + "\n" + text}]


def chunks_for(text, tokenizer, limit=850):
    """Split at sentence/word boundaries; hard token limit even for long words."""
    remaining = text.strip()
    while len(tokenizer.encode(remaining, add_special_tokens=False)) > limit:
        # Split characters, not decoded token slices: a token boundary can fall
        # inside a Unicode character, and stripping/re-encoding shifts offsets.
        low, high = 0, len(remaining)
        while low < high:
            middle = (low + high + 1) // 2
            if len(tokenizer.encode(remaining[:middle], add_special_tokens=False)) <= limit:
                low = middle
            else:
                high = middle - 1
        prefix = remaining[:low]
        cuts = [m.end() for m in re.finditer(r"[.!?]\s+|\n+", prefix)]
        cut = next((x for x in reversed(cuts) if x > len(prefix) // 2), 0)
        if not cut:
            word = prefix.rfind(" ")
            cut = word if word > len(prefix) // 2 else low
        if cut == 0:
            raise ValueError("Unable to split transcript safely.")
        piece = remaining[:cut].strip()
        if piece:
            yield piece
        remaining = remaining[cut:].lstrip()
    if remaining:
        yield remaining


def local_model(value):
    p = Path(value).expanduser().resolve(strict=True)
    if not p.is_dir() or not (p / "config.json").is_file():
        raise ValueError("Choose a downloaded model directory containing config.json.")
    return p


def release_speech():
    _CACHE.pop("stt", None)
    _CACHE.pop("stt_path", None)
    # mlx-whisper maintains a cache outside this module.
    legacy = sys.modules.get("mlx_whisper.transcribe")
    if legacy is not None:
        legacy.ModelHolder.model = None
        legacy.ModelHolder.model_path = None


def unload():
    _CACHE.clear()
    release_speech()
    gc.collect()
    if "mlx.core" in sys.modules:
        import mlx.core as mx
        mx.clear_cache()


def rewrite(req):
    import mlx.core as mx
    from mlx_lm import load, stream_generate
    from mlx_lm.sample_utils import make_sampler

    text = req.get("text", "").strip()
    if not text:
        return {"text": "", "seconds": 0.0}
    if len(text) > 100_000:
        raise ValueError("Rewrite up to 100,000 characters at a time.")
    # Validate before loading any weights.
    options = {k: req.get(k, default) for k, default in
               [("styling", "semi-formal"), ("structure", "prose"), ("context", "general")]}
    messages_for(text, **options)
    path = local_model(req["model"])
    started = time.monotonic()
    if not req.get("keep_models", True):
        unload()
    if _CACHE.get("rewrite_path") != str(path):
        _CACHE.pop("rewrite", None)
        gc.collect()
        mx.clear_cache()
        progress("Loading S1-mini by Superwhisper…")
        _CACHE["rewrite"] = load(str(path), tokenizer_config={"trust_remote_code": False})
        _CACHE["rewrite_path"] = str(path)
    model, tokenizer = _CACHE["rewrite"]
    output = []
    for part in chunks_for(text, tokenizer):
        prompt = tokenizer.apply_chat_template(
            messages_for(part, **options), tokenize=False,
            add_generation_prompt=True, enable_thinking=False,
        )
        input_length = len(tokenizer.encode(part, add_special_tokens=False))
        max_tokens = min(1400, int(input_length * 1.5) + 64)
        progress("Rewriting locally…")
        pieces = []
        last = None
        for item in stream_generate(model, tokenizer, prompt=prompt, max_tokens=max_tokens,
                                    sampler=make_sampler(temp=0.0)):
            pieces.append(item.text)
            last = item
        if last is not None and last.finish_reason == "length":
            raise ValueError("S1-mini reached its output limit. Your original transcript is preserved; try a shorter passage.")
        output.append("".join(pieces).strip())
    return {"text": "\n\n".join(x for x in output if x),
            "seconds": round(time.monotonic() - started, 3)}


def transcribe(req):
    import mlx.core as mx

    path = local_model(req["model"])
    audio = Path(req["audio"]).resolve(strict=True)
    if audio.suffix.lower() != ".wav":
        raise ValueError("Transcription expects a WAV audio file.")
    # Rust records mono WAV. For imported WAVs, MLX Audio resamples as needed.
    from mlx_audio.audio_io import read
    import numpy as np
    samples, _ = read(str(audio), dtype="float32", sample_rate=16000, nchannels=1)
    samples = np.asarray(samples).reshape(-1)
    if samples.size < 1600:
        raise ValueError("Record at least a tenth of a second.")
    if samples.size > 16000 * 300:
        raise ValueError("Recordings are limited to five minutes in v1.")
    if not np.all(np.isfinite(samples)):
        raise ValueError("The recording contains invalid audio samples.")
    if float(np.max(np.abs(samples))) < 0.002:
        return {"text": "", "seconds": 0.0}
    started = time.monotonic()
    if not req.get("keep_models", True):
        unload()
    legacy = (path / "weights.npz").exists() and not list(path.glob("*.safetensors"))
    language = req.get("language", "auto")
    kwargs = {} if language == "auto" else {"language": language}
    progress("Loading speech model…")
    if legacy:
        if "stt" in _CACHE:
            release_speech()
            gc.collect()
            mx.clear_cache()
        # Reuse existing mlx-whisper caches without conversion or duplicate downloads.
        import mlx_whisper
        result = mlx_whisper.transcribe(samples, path_or_hf_repo=str(path),
                                        verbose=None, **kwargs)
        text = result["text"]
    else:
        from mlx_audio.stt.utils import load_model
        if _CACHE.get("stt_path") != str(path):
            release_speech()
            gc.collect()
            mx.clear_cache()
            _CACHE["stt"] = load_model(str(path))
            _CACHE["stt_path"] = str(path)
        model = _CACHE["stt"]
        # Whisper uses ISO codes; Qwen3-ASR expects full language names.
        if "qwen3" in type(model).__module__ and language != "auto":
            kwargs["language"] = {"en": "English", "es": "Spanish", "fr": "French",
                                  "de": "German", "ja": "Japanese", "zh": "Chinese",
                                  "hi": "Hindi", "pt": "Portuguese", "it": "Italian",
                                  "ko": "Korean"}.get(language, language)
        # Only pass language to models that explicitly accept it.
        import inspect
        if "language" not in inspect.signature(model.generate).parameters:
            kwargs.pop("language", None)
        progress("Transcribing locally…")
        result = model.generate(audio=mx.array(samples), **kwargs)
        text = result.get("text", "") if isinstance(result, dict) else getattr(result, "text", None)
        if text is None:
            raise ValueError("This model does not return a dictation transcript. Choose an ASR model such as Qwen3-ASR, Parakeet, or Whisper.")
    detected = result.get("language") if isinstance(result, dict) else getattr(result, "language", None)
    if isinstance(detected, list):
        detected = detected[0] if detected else None
    return {"text": text.strip(), "language": detected,
            "seconds": round(time.monotonic() - started, 3)}


def download(req):
    if not ONLINE:
        raise ValueError("Downloads require an explicit online worker.")
    from huggingface_hub import snapshot_download
    repo = req["repo"]
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repo):
        raise ValueError("Enter a Hugging Face model ID in owner/model form.")
    progress(f"Downloading {repo}. This can take several minutes…")
    path = snapshot_download(repo_id=repo, allow_patterns=[
        "*.json", "*.safetensors", "*.npz", "*.model", "*.tiktoken",
        "*.txt", "*.jinja", "*.md", "LICENSE*", "NOTICE*",
    ])
    return {"path": path, "repo": repo}


def catalog(_req):
    if not ONLINE:
        raise ValueError("Catalog refresh requires an explicit online worker.")
    from huggingface_hub import HfApi
    api = HfApi()
    models = {}
    # Full iterators follow pagination. Also query the mlx-audio tag: some new
    # repositories have no pipeline_tag and would otherwise be invisible.
    for args in [dict(author="mlx-community", pipeline_tag="automatic-speech-recognition"),
                 dict(filter="mlx-audio"), dict(author="mlx-community", search="S1-mini")]:
        for model in api.list_models(**args):
            tags = model.tags or []
            ident = model.id
            lower = ident.lower()
            s1 = "s1-mini" in lower and ("mlx" in lower or "mlx" in tags)
            stt = ("mlx" in tags or "mlx" in lower) and (
                "automatic-speech-recognition" in tags or "speech-to-text" in tags or "stt" in tags)
            if s1 or stt:
                models[ident] = {"id": ident, "kind": "rewrite" if s1 else "speech"}
    return {"models": sorted(models.values(), key=lambda m: m["id"])}


def dispatch(req):
    op = req.get("op")
    if ONLINE and op not in ("catalog", "download", "ping"):
        raise ValueError("Online workers cannot receive transcripts or audio.")
    if op == "ping":
        import importlib.metadata
        import mlx.core as mx
        return {"ready": mx.metal.is_available(), "mlx_audio": importlib.metadata.version("mlx-audio"),
                "mlx_lm": importlib.metadata.version("mlx-lm")}
    if op == "transcribe":
        return transcribe(req)
    if op == "rewrite":
        return rewrite(req)
    if op == "download":
        return download(req)
    if op == "catalog":
        return catalog(req)
    if op == "unload":
        unload()
        return {"unloaded": True}
    raise ValueError(f"Unknown operation: {op}")


def main():
    os.environ["HF_HUB_DISABLE_TELEMETRY"] = "1"
    os.environ["DO_NOT_TRACK"] = "1"
    os.environ["TOKENIZERS_PARALLELISM"] = "false"
    if not ONLINE:
        os.environ["HF_HUB_OFFLINE"] = "1"
        os.environ["TRANSFORMERS_OFFLINE"] = "1"
    for line in sys.stdin:
        req = {}
        try:
            req = json.loads(line)
            if not isinstance(req, dict):
                raise ValueError("Each request must be a JSON object.")
            # Third-party print statements must never corrupt the JSON protocol.
            with contextlib.redirect_stdout(sys.stderr):
                result = dispatch(req)
            emit({"id": req.get("id"), "ok": True, "result": result})
        except Exception as exc:
            # Never echo the request: it may contain private dictation.
            emit({"id": req.get("id") if isinstance(req, dict) else None, "ok": False, "error": f"{type(exc).__name__}: {exc}"})
        if "--once" in sys.argv:
            break
    unload()


if __name__ == "__main__":
    main()
