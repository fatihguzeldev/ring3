#!/usr/bin/env python3
"""build or select a pinned engine artifact, then run registered Wasm tests."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[4]


def built_artifact(messages):
    paths = set()
    for line in messages.splitlines():
        event = json.loads(line)
        if event.get("reason") == "compiler-message":
            rendered = event["message"].get("rendered")
            if rendered:
                print(rendered, file=sys.stderr, end="")
        if event.get("reason") == "compiler-artifact" and event["target"]["name"] == "ring3_engine":
            paths.update(Path(path) for path in event["filenames"] if path.endswith(".wasm"))
    if len(paths) != 1:
        raise ValueError("cargo must report exactly one engine Wasm artifact")
    return paths.pop()


def select_artifact(path, expected):
    path = path.resolve(strict=True)
    actual = hashlib.sha256(path.read_bytes()).hexdigest()
    if expected is not None and actual != expected.lower():
        raise ValueError(f"artifact SHA-256 mismatch: expected {expected}, got {actual}")
    subprocess.run(["node", "-e", "new WebAssembly.Module(require('node:fs').readFileSync(process.argv[1]))", str(path)], check=True)
    return path, actual


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("targets", nargs="*", help="registered Cargo test targets ending in _wasm")
    parser.add_argument("--filter", help="Rust test-name filter")
    parser.add_argument("--engine", type=Path, help="reuse an explicit artifact instead of building")
    parser.add_argument("--sha256", help="required exact hash when reusing an artifact")
    parser.add_argument("--release", action="store_true", help="build the engine with the release profile")
    parser.add_argument("--preflight-only", action="store_true", help="build/select and validate without running tests")
    args = parser.parse_args()
    if bool(args.engine) != bool(args.sha256):
        parser.error("--engine and --sha256 must be supplied together")
    manifest = tomllib.loads((ROOT / "engine/Cargo.toml").read_text())
    registered = {item["name"] for item in manifest["test"] if item["name"].endswith("_wasm")}
    if not args.targets and not args.preflight_only:
        parser.error("select at least one Wasm test target")
    if unknown := set(args.targets) - registered:
        parser.error(f"unknown Wasm targets: {', '.join(sorted(unknown))}")
    if args.engine is None:
        build = ["cargo", "build", "--locked", "-p", "ring3-engine", "--target", "wasm32-unknown-unknown", "--message-format=json"]
        if args.release:
            build.append("--release")
        result = subprocess.run(build, cwd=ROOT, text=True, stdout=subprocess.PIPE)
        if result.returncode:
            for line in result.stdout.splitlines():
                event = json.loads(line)
                if event.get("reason") == "compiler-message" and event["message"].get("rendered"):
                    print(event["message"]["rendered"], file=sys.stderr, end="")
            result.check_returncode()
        args.engine = built_artifact(result.stdout)
    artifact, digest = select_artifact(args.engine, args.sha256)
    print(f"engine={artifact}\nsha256={digest}", flush=True)
    if args.preflight_only:
        return
    command = ["cargo", "test", "--locked", "-p", "ring3-engine"]
    for name in args.targets:
        command.extend(["--test", name])
    if args.filter:
        command.append(args.filter)
    subprocess.run(command, cwd=ROOT, env={**os.environ, "RING3_ENGINE_WASM": str(artifact), "RING3_ENGINE_SHA256": digest}, check=True)


if __name__ == "__main__":
    main()
