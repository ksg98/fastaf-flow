"""Protocol and normalization contracts; no network, microphone, or model weights."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import unittest
from types import SimpleNamespace
from unittest.mock import patch

ENGINE = Path(__file__).resolve().parents[1] / "worker" / "engine.py"
spec = importlib.util.spec_from_file_location("engine", ENGINE)
engine = importlib.util.module_from_spec(spec)
spec.loader.exec_module(engine)


class CharacterTokenizer:
    def encode(self, text, **_kwargs):
        return list(text.encode("utf-8"))


class ContractTests(unittest.TestCase):
    def test_catalog_keeps_untagged_official_moonshine_models(self):
        # Both official checkpoints lack MLX tags. A tag-only Hub response must
        # not make them disappear when the user refreshes the model library.
        api = SimpleNamespace(list_models=lambda **_: iter([
            SimpleNamespace(id="mlx-community/test-asr", tags=["mlx", "stt"]),
        ]))
        hub = SimpleNamespace(HfApi=lambda: api)
        with patch.dict(sys.modules, {"huggingface_hub": hub}), patch.object(engine, "ONLINE", True):
            models = {m["id"]: m for m in engine.catalog({})["models"]}
        bundled = {m["id"]: m for m in json.loads((ENGINE.parent.parent / "assets/catalog.json").read_text())}
        for name in ("tiny", "base"):
            ident = f"moonshine-ai/moonshine-{name}"
            self.assertEqual(models[ident]["kind"], "speech")
            self.assertEqual(models[ident]["quant"], "FP32")
            self.assertGreater(models[ident]["bytes"], 100_000_000)
            self.assertEqual(models[ident], bundled[ident])
        self.assertIn("mlx-community/test-asr", models)

    def test_s1_trained_control_axes(self):
        for style in engine.STYLING:
            for structure in engine.STRUCTURE:
                for context in engine.CONTEXT:
                    messages = engine.messages_for("um test", style, structure, context)
                    self.assertEqual(messages[0]["content"], engine.SYSTEM)
                    self.assertEqual(messages[1]["content"],
                        f"[Styling: {style}] [Structure: {structure}] [Context: {context}]\num test")
        with self.assertRaises(ValueError):
            engine.messages_for("test", styling="professional")

    def test_long_text_chunking_preserves_words_and_unicode(self):
        text = "  Café 😀 costs five dollars.  Actually make that six dollars.\n" * 50
        parts = list(engine.chunks_for(text, CharacterTokenizer(), limit=100))
        self.assertEqual(" ".join(text.split()), " ".join(" ".join(parts).split()))
        self.assertTrue(all(len(p.encode("utf-8")) <= 100 for p in parts))
        self.assertNotIn("�", "".join(parts))

    def test_long_unbroken_input_is_bounded(self):
        text = "é" * 4000
        parts = list(engine.chunks_for(text, CharacterTokenizer(), limit=101))
        self.assertEqual("".join(parts), text)
        self.assertTrue(all(len(p.encode("utf-8")) <= 101 for p in parts))
        self.assertEqual(list(engine.chunks_for("   ", CharacterTokenizer())), [])

    def test_offline_worker_refuses_downloads_and_recovers_from_bad_input(self):
        requests = 'not json\n[]\n' + json.dumps({"id": 3, "op": "download", "repo": "a/b"}) + '\n' + json.dumps({"id": 4, "op": "unload"}) + '\n'
        result = subprocess.run([sys.executable, str(ENGINE)], input=requests,
                                capture_output=True, text=True, timeout=15, check=True)
        lines = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertFalse(lines[0]["ok"])
        self.assertFalse(lines[1]["ok"])
        self.assertIn("explicit online worker", lines[2]["error"])
        self.assertEqual(lines[3], {"id": 4, "ok": True, "result": {"unloaded": True}})

    def test_online_worker_refuses_private_inference(self):
        request = {"id": 1, "op": "rewrite", "text": "PRIVATE_TRANSCRIPT_CANARY", "model": "anywhere"}
        result = subprocess.run([sys.executable, str(ENGINE), "--online", "--once"],
                                input=json.dumps(request) + '\n', capture_output=True,
                                text=True, timeout=15, check=True)
        self.assertIn("cannot receive transcripts", result.stdout)
        self.assertNotIn("PRIVATE_TRANSCRIPT_CANARY", result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
