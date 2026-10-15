import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

RUNNER = Path(__file__).with_name("run.py")
spec = importlib.util.spec_from_file_location("wasm_runner", RUNNER)
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class ArtifactPreflight(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / "engine.wasm"
        self.path.write_bytes(b"\0asm\x01\0\0\0")
        self.digest = hashlib.sha256(self.path.read_bytes()).hexdigest()

    def test_valid_pinned_module_is_selected(self):
        self.assertEqual(runner.select_artifact(self.path, self.digest), (self.path.resolve(), self.digest))

    def test_changed_bytes_do_not_satisfy_old_pin(self):
        self.path.write_bytes(b"\0asm\x01\0\0\0\0\x01\0")
        with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
            runner.select_artifact(self.path, self.digest)

    def test_matching_hash_does_not_make_invalid_wasm_valid(self):
        self.path.write_bytes(b"invalid wasm")
        digest = hashlib.sha256(self.path.read_bytes()).hexdigest()
        with self.assertRaises(subprocess.CalledProcessError):
            runner.select_artifact(self.path, digest)

    def test_reused_artifact_requires_explicit_hash(self):
        result = subprocess.run([sys.executable, str(RUNNER), "--engine", str(self.path), "--preflight-only"], capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn("--engine and --sha256", result.stderr)

    def test_build_selects_reported_configured_artifact(self):
        configured = self.path.parent / "custom-target/release/engine.wasm"
        event = {"reason": "compiler-artifact", "target": {"name": "ring3_engine"},
                 "filenames": [str(configured), str(configured.with_suffix(".rlib"))]}
        self.assertEqual(runner.built_artifact(json.dumps(event)), configured)

    def test_build_never_falls_back_when_no_wasm_is_reported(self):
        with self.assertRaisesRegex(ValueError, "exactly one"):
            runner.built_artifact(json.dumps({"reason": "build-finished", "success": True}))


if __name__ == "__main__":
    unittest.main()
