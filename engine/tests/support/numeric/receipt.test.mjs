import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { verifyNumericReceipt } from "./receipt.mjs";

const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");
const binary = Buffer.from("receipt tests never instantiate a Wasm module");
const hook = '\n#[path = "../tests/support/numeric/backend.rs"]\nmod numeric_proof;\n';
const support = "engine/tests/support/numeric";
const currentOwners = [
  "lib.rs", "cpu/x86/avx/mod.rs", "cpu/x86/avx/dispatch.rs", "cpu/x86/avx/operation.rs",
  "cpu/x86/avx/operation/factory.rs", "cpu/x86/avx/operation/factory/floating.rs",
  "cpu/x86/avx/operation/factory/logical.rs", "cpu/x86/avx/operation/factory/compound.rs",
  "cpu/x86/avx/arithmetic.rs", "cpu/x86/avx/compound/mod.rs",
  "cpu/x86/avx/arithmetic/memory/fixtures/runtime/run.mjs",
  "cpu/x86/avx/compound/fixtures/runtime/run.mjs", "cpu/x86/simd/mod.rs", "cpu/x86/simd/state.rs",
  "cpu/x86/simd/operation.rs", "cpu/x86/simd/descriptor.rs", "cpu/x86/simd/execute.rs",
  "cpu/x86/xstate/fxsave.rs", "process/cpu/simd/arena.rs", "cpu/x86/simd/float.rs",
  "cpu/x86/simd/packed.rs", "math/ieee.rs",
].map((path) => `engine/src/${path}`);
function write(root, path, data) {
  mkdirSync(dirname(resolve(root, path)), { recursive: true });
  writeFileSync(resolve(root, path), data);
}
function inventory(root, directory = "") {
  return readdirSync(resolve(root, directory), { withFileTypes: true }).flatMap((entry) => {
    const path = directory ? `${directory}/${entry.name}` : entry.name;
    return entry.isDirectory() ? inventory(root, path) : [[path, sha(readFileSync(resolve(root, path)))]];
  }).sort(([a], [b]) => a.localeCompare(b, "en"));
}
function fixture() {
  const root = mkdtempSync(resolve(tmpdir(), "ring3-numeric-receipt-"));
  for (const path of [...currentOwners, "Cargo.toml", "Cargo.lock", "rust-toolchain.toml",
    "engine/Cargo.toml", ".cargo/config.toml"]) write(root, path, "source\n");
  write(root, "engine/src/lib.rs", `source\n${hook}`);
  write(root, "rust-toolchain.toml", '[toolchain]\nchannel = "1.97.1"\n');
  write(root, "Cargo.lock", '\n[[package]]\nname = "example"\nversion = "1.0.0"\n'
    + 'source = "registry+https://example.invalid/index"\nchecksum = "' + "a".repeat(64) + '"\n');
  for (const name of ["backend.rs", "build.py", "contract.json", "receipt.mjs", "receipt.test.mjs"]) {
    write(root, `${support}/${name}`, readFileSync(new URL(`./${name}`, import.meta.url)));
  }
  write(root, "vendor/example-1.0.0/lib.rs", "dependency\n");
  write(root, "vendor/example-1.0.0/.cargo-checksum.json", JSON.stringify({
    package: "a".repeat(64), files: { "lib.rs": sha(Buffer.from("dependency\n")) },
  }));
  const build = {
    cwd: root, target: "wasm32-unknown-unknown", profile: "release", locked: true, offline: true,
    arguments: ["rustup", "run", "1.97.1", "cargo", "build", "--locked", "--offline", "--release",
      "--target", "wasm32-unknown-unknown", "--manifest-path", resolve(root, "Cargo.toml"),
      "-p", "ring3-engine", "--lib", "--target-dir", resolve(root, "../cargo-target")],
    environment: { CARGO_HOME: resolve(root, "../cargo-home") },
  };
  for (const name of ["build.log", "metadata.log", "sysroot.log", "target-libdir.log", "vendor.log"]) {
    write(root, `.numeric-proof/${name}`, "evidence\n");
  }
  write(root, ".numeric-proof/rustc.log", "rustc 1.97.1 (test)\n");
  write(root, ".numeric-proof/cargo.log", "cargo 1.97.1 (test)\n");
  write(root, ".numeric-proof/build.json", JSON.stringify(build));
  write(root, ".numeric-proof/toolchain.json", JSON.stringify({
    channel: "1.97.1", rustc: "rustc 1.97.1 (test)\n", cargo: "cargo 1.97.1 (test)\n",
    target: build.target, targetLibraries: [["libstd.rlib", "b".repeat(64)]],
    compilerArtifacts: ["bin/rustc", "bin/cargo", "lib/rustlib/test-host/bin/rust-lld"]
      .map((path) => [path, "b".repeat(64)]),
  }));
  write(root, ".numeric-proof/metadata.json", JSON.stringify({ packages: [
    { name: "ring3-engine", source: null },
    { name: "example", version: "1.0.0", source: "registry+https://example.invalid/index" },
  ] }));
  const receipt = {
    schema: "ring3-private-numeric-backend", version: 2, protocolVersion: 1,
    families: ["compound", "foundation"], publicAVX: false, backendSha256: sha(binary),
    contractSha256: sha(readFileSync(resolve(root, `${support}/contract.json`))),
    originalLibSha256: sha(Buffer.from("source\n")),
    sourcePaths: inventory(root).filter(([path]) => !path.startsWith("vendor/") && !path.startsWith(".numeric-proof/")),
    dependencyPaths: inventory(root, "vendor"), evidencePaths: inventory(root, ".numeric-proof"),
  };
  return { root, receipt, cleanup: () => rmSync(root, { recursive: true, force: true }) };
}

test("current receipt admits both unchanged numeric proof families", () => {
  const { root, receipt, cleanup } = fixture();
  try {
    for (const family of ["foundation", "compound"]) {
      assert.deepEqual(verifyNumericReceipt(binary, root, receipt, family), receipt.sourcePaths);
    }
  } finally { cleanup(); }
});

test("current receipt refuses incomplete ownership, drift, protocol and build substitutions", () => {
  const changes = [
    (root, receipt) => { receipt.sourcePaths.pop(); },
    (root) => write(root, "engine/src/unlisted.rs", "extra owner\n"),
    (root, receipt) => {
      rmSync(resolve(root, "engine/src/math/ieee.rs"));
      receipt.sourcePaths = receipt.sourcePaths.filter(([path]) => path !== "engine/src/math/ieee.rs");
    },
    (root) => write(root, "vendor/example-1.0.0/lib.rs", "changed dependency\n"),
    (root, receipt) => { receipt.dependencyPaths.push(receipt.dependencyPaths[0]); },
    (root, receipt) => { receipt.version = 3; },
    (root, receipt) => { receipt.protocolVersion = 2; },
    (root, receipt) => { receipt.backendSha256 = "c".repeat(64); },
    (root, receipt) => { receipt.sourcePaths[0][0] = "../outside"; },
    (root) => {
      rmSync(resolve(root, "engine/src/math/ieee.rs"));
      symlinkSync(resolve(root, "engine/src/lib.rs"), resolve(root, "engine/src/math/ieee.rs"));
    },
    (root, receipt) => {
      const path = ".numeric-proof/build.json";
      const build = JSON.parse(readFileSync(resolve(root, path)));
      build.arguments = build.arguments.filter((part) => part !== "--locked");
      write(root, path, JSON.stringify(build));
      receipt.evidencePaths = inventory(root, ".numeric-proof");
    },
  ];
  for (const change of changes) {
    const { root, receipt, cleanup } = fixture();
    try {
      change(root, receipt);
      assert.throws(() => verifyNumericReceipt(binary, root, receipt, "foundation"));
    } finally { cleanup(); }
  }
});

test("historical receipts still require the exact original eight owners", () => {
  for (const family of ["foundation", "compound"]) {
    const root = mkdtempSync(resolve(tmpdir(), "ring3-numeric-legacy-"));
    try {
      const paths = family === "foundation" ? [
        "src/abi/wasm/avx_proof.rs", "src/abi/wasm/avx_proof/foundation.rs", "src/cpu/x86/avx/mod.rs",
        "src/cpu/x86/avx/arithmetic.rs", "src/cpu/x86/avx/arithmetic/memory/fixtures/runtime/run.mjs",
        "src/cpu/x86/simd.rs", "src/cpu/x86/simd/float.rs", "src/math/ieee.rs",
      ] : [
        "src/abi/wasm/avx_proof.rs", "src/cpu/x86/avx/mod.rs", "src/cpu/x86/avx/compound/mod.rs",
        "src/cpu/x86/avx/compound/fixtures/runtime/run.mjs", "src/cpu/x86/simd.rs",
        "src/cpu/x86/simd/float.rs", "src/cpu/x86/simd/packed.rs", "src/math/ieee.rs",
      ];
      for (const path of paths) write(root, path, "historical\n");
      const receipt = { backendSha256: sha(binary), sourcePaths: inventory(root) };
      verifyNumericReceipt(binary, root, receipt, family);
      receipt.sourcePaths.pop();
      assert.throws(() => verifyNumericReceipt(binary, root, receipt, family));
      receipt.sourcePaths = inventory(root);
      receipt.sourcePaths.push(receipt.sourcePaths[0]);
      assert.throws(() => verifyNumericReceipt(binary, root, receipt, family));
    } finally { rmSync(root, { recursive: true, force: true }); }
  }
});
