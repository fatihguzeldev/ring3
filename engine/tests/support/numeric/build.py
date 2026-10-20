#!/usr/bin/env python3
"""build the private numeric backend in an isolated, pinned source snapshot."""

import argparse
from contextlib import ExitStack
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tomllib


SUPPORT = "engine/tests/support/numeric"
SUPPORT_FILES = ["backend.rs", "build.py", "contract.json", "receipt.mjs", "receipt.test.mjs"]
FIXED = ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "engine/Cargo.toml"]
HOOK = '\n#[path = "../tests/support/numeric/backend.rs"]\nmod numeric_proof;\n'
TARGET = "wasm32-unknown-unknown"


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def files(directory):
    result = []
    for entry in sorted(directory.rglob("*")):
        if entry.is_symlink():
            raise ValueError(f"source symlink is not permitted: {entry}")
        if entry.is_file():
            result.append(entry)
    return result


def inventory(root, paths):
    return [[path.relative_to(root).as_posix(), sha256(path.read_bytes())] for path in sorted(paths)]


def sources(root):
    return inventory(root, [root / path for path in FIXED + [".cargo/config.toml"]]
                     + files(root / "engine/src")
                     + files(root / "engine/tests"))


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def run(arguments, cwd, environment, log):
    result = subprocess.run(arguments, cwd=cwd, env=environment, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, check=False)
    log.write_bytes(result.stdout + result.stderr)
    if result.returncode:
        raise RuntimeError(f"command failed ({result.returncode}); see {log}")
    return result.stdout


def build(repository, output):
    source = output / "source"
    evidence = source / ".numeric-proof"
    evidence.mkdir(parents=True)
    selected = ([repository / path for path in FIXED] + files(repository / "engine/src")
                + files(repository / "engine/tests"))
    for path in SUPPORT_FILES:
        if repository / SUPPORT / path not in selected:
            raise ValueError(f"missing tracked numeric support owner: {path}")
    captured = {path: path.read_bytes() for path in selected}
    for path, data in captured.items():
        destination = source / path.relative_to(repository)
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(data)
    if any(path.read_bytes() != data for path, data in captured.items()):
        raise RuntimeError("checkout sources changed while capturing the snapshot")
    lib = source / "engine/src/lib.rs"
    original_lib = lib.read_bytes()
    if b"mod numeric_proof;" in original_lib or b"ring3_avx_probe_" in original_lib:
        raise ValueError("production library already contains a private numeric hook")
    lib.write_bytes(original_lib + HOOK.encode())
    channel = tomllib.loads((source / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    # retain only ordinary host/toolchain discovery and transport variables.
    environment = {key: value for key, value in os.environ.items() if key in {
        "PATH", "HOME", "USER", "TMPDIR", "TEMP", "TMP", "SYSTEMROOT", "RUSTUP_HOME",
        "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY", "SSL_CERT_FILE", "SSL_CERT_DIR",
    }}
    environment["CARGO_HOME"] = str(output / "cargo-home")
    cargo = ["rustup", "run", channel, "cargo"]
    rustc = ["rustup", "run", channel, "rustc"]
    rustc_version = run(rustc + ["--version", "--verbose"], source, environment,
                        evidence / "rustc.log").decode()
    cargo_version = run(cargo + ["--version"], source, environment,
                        evidence / "cargo.log").decode()
    vendor = source / "vendor"
    vendor_config = run(cargo + ["vendor", "--locked", "--versioned-dirs", str(vendor)],
                        source, environment, evidence / "vendor.log")
    # cargo vendor emits the exact directory-source replacement configuration.
    config = source / ".cargo/config.toml"
    config.parent.mkdir()
    config.write_bytes(vendor_config)
    metadata = run(cargo + ["metadata", "--locked", "--offline", "--format-version", "1",
                            "--filter-platform", TARGET], source, environment,
                   evidence / "metadata.log")
    metadata = json.loads(metadata)
    if any(package["source"] is not None and not package["source"].startswith("registry+")
           for package in metadata["packages"]):
        raise ValueError("this producer requires locked registry dependencies")
    write_json(evidence / "metadata.json", metadata)
    target_libdir = Path(run(rustc + ["--print", "target-libdir", "--target", TARGET],
                             source, environment, evidence / "target-libdir.log").decode().strip())
    toolchain_libraries = inventory(target_libdir, files(target_libdir))
    if not toolchain_libraries:
        raise ValueError(f"the {TARGET} target libraries are not installed")
    sysroot = Path(run(rustc + ["--print", "sysroot"], source, environment,
                       evidence / "sysroot.log").decode().strip())
    host = next(line.removeprefix("host: ") for line in rustc_version.splitlines()
                if line.startswith("host: "))
    compiler_files = [sysroot / "bin/rustc", sysroot / "bin/cargo",
                      sysroot / "lib/rustlib" / host / "bin/rust-lld"]
    compiler_files += [path for path in (sysroot / "lib").glob("*") if path.is_file()]
    write_json(evidence / "toolchain.json", {
        "channel": channel, "rustc": rustc_version, "cargo": cargo_version,
        "target": TARGET, "targetLibraries": toolchain_libraries,
        "compilerArtifacts": inventory(sysroot, compiler_files),
    })
    before = sources(source)
    dependencies = inventory(source, files(vendor))
    arguments = cargo + ["build", "--locked", "--offline", "--release", "--target", TARGET,
                         "--manifest-path", str(source / "Cargo.toml"), "-p", "ring3-engine",
                         "--lib", "--target-dir", str(output / "cargo-target")]
    run(arguments, source, environment, evidence / "build.log")
    if sources(source) != before or inventory(source, files(vendor)) != dependencies:
        raise RuntimeError("source or vendored dependencies changed during the build")
    binary = output / "private-numeric.wasm"
    shutil.copyfile(output / "cargo-target" / TARGET / "release/ring3_engine.wasm", binary)
    write_json(evidence / "build.json", {
        "cwd": str(source), "arguments": arguments,
        "environment": {"CARGO_HOME": environment["CARGO_HOME"]},
        "target": TARGET, "profile": "release", "locked": True, "offline": True,
    })
    write_json(output / "receipt.json", {
        "schema": "ring3-private-numeric-backend", "version": 2, "protocolVersion": 1,
        "families": ["compound", "foundation"], "publicAVX": False,
        "backendSha256": sha256(binary.read_bytes()),
        "contractSha256": sha256((source / SUPPORT / "contract.json").read_bytes()),
        "originalLibSha256": sha256(original_lib), "sourcePaths": before,
        "dependencyPaths": dependencies,
        "evidencePaths": inventory(source, files(evidence)),
    })
    print(json.dumps({"backend": str(binary), "sourceRoot": str(source),
                      "receipt": str(output / "receipt.json")}, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path, help="a new retained output directory")
    args = parser.parse_args()
    repository = Path(__file__).resolve().parents[4]
    output = args.output.resolve()
    # ancestor cargo configuration can inject flags, wrappers, or dependencies.
    for ancestor in [output, *output.parents]:
        for name in ["config", "config.toml"]:
            if (ancestor / ".cargo" / name).exists():
                raise ValueError(f"choose an output without ancestor cargo configuration: {ancestor}")
    output.mkdir(parents=True, exist_ok=False)
    with ExitStack() as caches:
        for name in ["cargo-home", "cargo-target"]:
            cache = output / name
            cache.mkdir()
            caches.callback(shutil.rmtree, cache)
        build(repository, output)


if __name__ == "__main__":
    main()
