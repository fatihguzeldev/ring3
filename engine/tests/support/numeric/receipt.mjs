import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { lstatSync, readFileSync, readdirSync } from "node:fs";
import { isAbsolute, relative, resolve, sep } from "node:path";

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const support = "engine/tests/support/numeric";
const supportFiles = ["backend.rs", "build.py", "contract.json", "receipt.mjs", "receipt.test.mjs"];
const fixed = ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "engine/Cargo.toml", ".cargo/config.toml"];
const completeInputs = ["observability/rust/Cargo.toml", "observability/catalog/categories.json",
  "engine/assets/fonts/NotoSans-Regular.ttf", "engine/assets/fonts/NotoSansMono-Regular.ttf",
  "engine/assets/fonts/OFL.txt", "engine/assets/fonts/provenance.json"];
const evidenceFiles = ["build.json", "build.log", "cargo.log", "metadata.json", "metadata.log",
  "rustc.log", "sysroot.log", "target-libdir.log", "toolchain.json", "vendor.log"];
const hook = '\n#[path = "../tests/support/numeric/backend.rs"]\nmod numeric_proof;\n';

const historicalOwners = {
  foundation: [
    "src/abi/wasm/avx_proof.rs", "src/abi/wasm/avx_proof/foundation.rs",
    "src/cpu/x86/avx/mod.rs", "src/cpu/x86/avx/arithmetic.rs",
    "src/cpu/x86/avx/arithmetic/memory/fixtures/runtime/run.mjs",
    "src/cpu/x86/simd.rs", "src/cpu/x86/simd/float.rs", "src/math/ieee.rs",
  ],
  compound: [
    "src/abi/wasm/avx_proof.rs", "src/cpu/x86/avx/mod.rs", "src/cpu/x86/avx/compound/mod.rs",
    "src/cpu/x86/avx/compound/fixtures/runtime/run.mjs", "src/cpu/x86/simd.rs",
    "src/cpu/x86/simd/float.rs", "src/cpu/x86/simd/packed.rs", "src/math/ieee.rs",
  ],
};
const currentOwners = [
  "engine/src/lib.rs", "engine/src/cpu/x86/avx/mod.rs", "engine/src/cpu/x86/avx/dispatch.rs",
  "engine/src/cpu/x86/avx/operation.rs", "engine/src/cpu/x86/avx/operation/factory.rs",
  "engine/src/cpu/x86/avx/operation/factory/floating.rs",
  "engine/src/cpu/x86/avx/operation/factory/logical.rs",
  "engine/src/cpu/x86/avx/operation/factory/compound.rs",
  "engine/src/cpu/x86/avx/arithmetic.rs", "engine/src/cpu/x86/avx/compound/mod.rs",
  "engine/src/cpu/x86/avx/arithmetic/memory/fixtures/runtime/run.mjs",
  "engine/src/cpu/x86/avx/compound/fixtures/runtime/run.mjs", "engine/src/cpu/x86/simd/mod.rs", "engine/src/cpu/x86/simd/state.rs", "engine/src/cpu/x86/simd/operation.rs", "engine/src/cpu/x86/simd/descriptor.rs", "engine/src/cpu/x86/simd/execute.rs", "engine/src/cpu/x86/xstate/fxsave.rs", "engine/src/process/cpu/simd/arena.rs",
  "engine/src/cpu/x86/simd/float.rs", "engine/src/cpu/x86/simd/packed.rs", "engine/src/math/ieee.rs",
];

function sourcePath(root, path, strict) {
  assert.equal(typeof path, "string");
  assert(path.length > 0 && !isAbsolute(path), "backend source path must be relative");
  const result = resolve(root, path);
  const fromRoot = relative(root, result);
  assert(fromRoot !== ".." && !fromRoot.startsWith(`..${sep}`) && !isAbsolute(fromRoot),
    "backend source path escapes its root");
  if (strict) {
    assert.equal(path, fromRoot.split(sep).join("/"), "source path must be canonical");
    let parent = root;
    assert(!lstatSync(parent).isSymbolicLink(), "source root must not be a symlink");
    for (const part of fromRoot.split(sep)) {
      parent = resolve(parent, part);
      assert(!lstatSync(parent).isSymbolicLink(), "source symlink is not permitted");
    }
  }
  return result;
}

function pathsBelow(root, directory) {
  const result = [];
  for (const name of readdirSync(sourcePath(root, directory, true)).sort()) {
    const path = `${directory}/${name}`;
    const stat = lstatSync(sourcePath(root, path, true));
    assert(stat.isDirectory() || stat.isFile(), "source must be an ordinary file or directory");
    if (stat.isDirectory()) result.push(...pathsBelow(root, path));
    else result.push(path);
  }
  return result;
}

function verifyInventory(root, entries, required, strict) {
  assert(Array.isArray(entries) && entries.length > 0);
  const checked = entries.map((entry) => {
    assert(Array.isArray(entry) && entry.length === 2, "expected a source path and SHA-256");
    const [path, expected] = entry;
    assert.match(expected, /^[a-f0-9]{64}$/);
    assert.equal(sha256(readFileSync(sourcePath(root, path, strict))), expected,
      `backend source differs: ${path}`);
    return [path, expected];
  });
  const paths = checked.map(([path]) => path);
  assert.equal(new Set(paths).size, paths.length, "duplicate source owner");
  assert.deepEqual(paths.toSorted(), required.toSorted(),
    "backend receipt must retain every required source owner");
  return checked;
}

export function verifyNumericReceipt(binary, root, receipt, family) {
  assert(Object.hasOwn(historicalOwners, family), "unknown numeric proof family");
  assert.match(receipt.backendSha256, /^[a-f0-9]{64}$/);
  assert.equal(sha256(binary), receipt.backendSha256, "backend differs from its receipt");
  if (receipt.version === undefined) {
    return verifyInventory(root, receipt.sourcePaths, historicalOwners[family], false);
  }
  assert.equal(receipt.schema, "ring3-private-numeric-backend");
  assert([2, 3].includes(receipt.version), "unknown numeric receipt version");
  assert.equal(receipt.protocolVersion, 1);
  assert.equal(receipt.publicAVX, false);
  assert.deepEqual(receipt.families, ["compound", "foundation"]);
  const workspaceOwners = receipt.version === 3 ? ["observability/rust/src/lib.rs"] : [];
  const expected = [...fixed, ...pathsBelow(root, "engine/src"), ...pathsBelow(root, "engine/tests"),
    ...(receipt.version === 3
      ? [...completeInputs, ...pathsBelow(root, "observability/rust/src")] : [])];
  for (const owner of [...currentOwners, ...workspaceOwners, ...supportFiles.map((name) => `${support}/${name}`)]) {
    assert(expected.includes(owner), `missing current owner: ${owner}`);
  }
  const sources = verifyInventory(root, receipt.sourcePaths, expected, true);
  verifyInventory(root, receipt.dependencyPaths, pathsBelow(root, "vendor"), true);
  verifyInventory(root, receipt.evidencePaths,
    evidenceFiles.map((name) => `.numeric-proof/${name}`), true);
  const read = (path) => readFileSync(sourcePath(root, path, true));
  const contract = read(`${support}/contract.json`);
  assert.equal(sha256(contract), receipt.contractSha256);
  assert.deepEqual(JSON.parse(contract), JSON.parse(readFileSync(new URL("./contract.json", import.meta.url))),
    "numeric protocol differs from the current runner contract");
  const lib = read("engine/src/lib.rs").toString();
  assert(lib.endsWith(hook), "private hook must be the isolated library suffix");
  assert.equal(sha256(Buffer.from(lib.slice(0, -hook.length))), receipt.originalLibSha256);
  const build = JSON.parse(read(".numeric-proof/build.json"));
  const toolchain = JSON.parse(read(".numeric-proof/toolchain.json"));
  assert.equal(build.cwd, root);
  assert.equal(build.target, "wasm32-unknown-unknown");
  assert.equal(build.profile, "release");
  assert.equal(build.locked, true);
  assert.equal(build.offline, true);
  assert.match(toolchain.channel, /^\d+\.\d+\.\d+$/);
  assert.equal(toolchain.channel, read("rust-toolchain.toml").toString()
    .match(/^\s*channel\s*=\s*"([^"]+)"/m)?.[1]);
  assert(toolchain.rustc.startsWith(`rustc ${toolchain.channel} `));
  assert.match(toolchain.cargo, /^cargo \d+\.\d+\.\d+ /);
  assert.equal(toolchain.rustc, read(".numeric-proof/rustc.log").toString());
  assert.equal(toolchain.cargo, read(".numeric-proof/cargo.log").toString());
  assert.equal(toolchain.target, build.target);
  for (const files of [toolchain.targetLibraries, toolchain.compilerArtifacts]) {
    assert(Array.isArray(files) && files.length > 0);
    assert.equal(new Set(files.map(([path]) => path)).size, files.length);
    for (const [path, hash] of files) {
      assert(!isAbsolute(path) && !path.split("/").includes(".."));
      assert.match(hash, /^[a-f0-9]{64}$/);
    }
  }
  for (const path of ["bin/rustc", "bin/cargo"]) {
    assert(toolchain.compilerArtifacts.some(([entry]) => entry === path));
  }
  assert(toolchain.compilerArtifacts.some(([entry]) => entry.endsWith("/bin/rust-lld")));
  assert.deepEqual(build.arguments, ["rustup", "run", toolchain.channel, "cargo", "build",
    "--locked", "--offline", "--release", "--target", build.target, "--manifest-path",
    resolve(root, "Cargo.toml"), "-p", "ring3-engine", "--lib", "--target-dir",
    resolve(root, "../cargo-target")]);
  assert.deepEqual(build.environment, { CARGO_HOME: resolve(root, "../cargo-home") });
  const metadata = JSON.parse(read(".numeric-proof/metadata.json"));
  const lock = read("Cargo.lock").toString().split(/\n\[\[package\]\]\n/).slice(1);
  assert(metadata.packages.some((entry) => entry.name === "ring3-engine" && entry.source === null));
  for (const entry of metadata.packages.filter((entry) => entry.source !== null)) {
    assert(entry.source.startsWith("registry+"), "only locked registry dependencies are admitted");
    const directory = `vendor/${entry.name}-${entry.version}`;
    const checksum = JSON.parse(read(`${directory}/.cargo-checksum.json`));
    assert.match(checksum.package, /^[a-f0-9]{64}$/);
    const locked = lock.find((block) => block.match(/^name = "([^"]+)"/m)?.[1] === entry.name
      && block.match(/^version = "([^"]+)"/m)?.[1] === entry.version
      && block.match(/^source = "([^"]+)"/m)?.[1] === entry.source);
    assert(locked, `dependency is absent from Cargo.lock: ${entry.name}`);
    assert.equal(checksum.package, locked.match(/^checksum = "([^"]+)"/m)?.[1]);
    verifyInventory(root, Object.entries(checksum.files).map(([path, hash]) => [`${directory}/${path}`, hash]),
      pathsBelow(root, directory).filter((path) => path !== `${directory}/.cargo-checksum.json`), true);
  }
  return sources;
}
