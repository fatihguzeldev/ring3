import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("numeric_build", Path(__file__).with_name("build.py"))
builder = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(builder)


class BuildLifecycleTests(unittest.TestCase):
    def run_fixture(self, directory, failure=None):
        repository = directory / "repository"
        for name in builder.FIXED + [f"{builder.SUPPORT}/{name}" for name in builder.SUPPORT_FILES]:
            destination = repository / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text("fixture source\n")
        (repository / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.97.1"\n')
        library = repository / "engine/src/lib.rs"
        library.parent.mkdir(parents=True)
        library.write_text("pub fn fixture() {}\n")
        sysroot = directory / "sysroot"
        for name in ["bin/rustc", "bin/cargo", "lib/rustlib/test-host/bin/rust-lld",
                     "lib/rustlib/wasm32-unknown-unknown/lib/libstd.rlib"]:
            destination = sysroot / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(b"toolchain fixture")
        output = directory / "retained"

        def fake_run(arguments, cwd, environment, log):
            cache = Path(environment["CARGO_HOME"])
            cache.mkdir(exist_ok=True)
            (cache / "download-cache").write_bytes(b"disposable")
            log.write_text("command fixture\n")
            if "vendor" in arguments:
                vendor = Path(arguments[-1])
                vendor.mkdir()
                (vendor / "dependency.rs").write_text("dependency source\n")
                if failure == "vendor":
                    raise RuntimeError("vendor failed")
                return b"[source.fixture]\ndirectory = 'vendor'\n"
            if "metadata" in arguments:
                return json.dumps({"packages": [{"name": "ring3-engine", "source": None}]}).encode()
            if "target-libdir" in arguments:
                return str(sysroot / "lib/rustlib/wasm32-unknown-unknown/lib").encode()
            if "sysroot" in arguments:
                return str(sysroot).encode()
            if "build" in arguments:
                target = Path(arguments[arguments.index("--target-dir") + 1])
                artifact = target / builder.TARGET / "release/ring3_engine.wasm"
                artifact.parent.mkdir(parents=True)
                artifact.write_bytes(b"final Wasm fixture")
                if failure == "build":
                    raise RuntimeError("build failed")
                return b""
            if "rustc" in arguments:
                return b"rustc 1.97.1 (fixture)\nhost: test-host\n"
            return b"cargo 1.97.1 (fixture)\n"

        with patch.object(builder, "__file__", str(repository / builder.SUPPORT / "build.py")), \
                patch.object(builder, "run", side_effect=fake_run), \
                patch("sys.argv", ["build.py", "--output", str(output)]), \
                contextlib.redirect_stdout(io.StringIO()):
            if failure:
                with self.assertRaisesRegex(RuntimeError, f"{failure} failed"):
                    builder.main()
            else:
                builder.main()
        return output

    def test_success_keeps_reproducible_inputs_and_final_artifact_without_cargo_caches(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = self.run_fixture(Path(temporary))
            self.assertEqual((output / "private-numeric.wasm").read_bytes(), b"final Wasm fixture")
            receipt = json.loads((output / "receipt.json").read_text())
            source = output / "source"
            for category in ["sourcePaths", "dependencyPaths", "evidencePaths"]:
                for name, digest in receipt[category]:
                    self.assertEqual(builder.sha256((source / name).read_bytes()), digest)
            self.assertFalse((output / "cargo-home").exists())
            self.assertFalse((output / "cargo-target").exists())

    def test_failure_keeps_source_and_diagnostic_logs_without_cargo_caches(self):
        for failure in ["vendor", "build"]:
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as temporary:
                output = self.run_fixture(Path(temporary), failure)
                self.assertTrue((output / "source/engine/src/lib.rs").is_file())
                self.assertTrue((output / f"source/.numeric-proof/{failure}.log").is_file())
                self.assertFalse((output / "receipt.json").exists())
                self.assertFalse((output / "cargo-home").exists())
                self.assertFalse((output / "cargo-target").exists())

    def test_existing_output_is_refused_without_cleaning_its_cache(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "retained"
            cache = output / "cargo-target"
            cache.mkdir(parents=True)
            sentinel = cache / "existing-evidence"
            sentinel.write_bytes(b"preserve")
            with patch("sys.argv", ["build.py", "--output", str(output)]):
                with self.assertRaises(FileExistsError):
                    builder.main()
            self.assertEqual(sentinel.read_bytes(), b"preserve")


if __name__ == "__main__":
    unittest.main()
