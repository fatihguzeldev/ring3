import assert from "node:assert/strict";
import test from "node:test";
import { launchOptions } from "./launch.js";

test("Run forwards the server-selected v86 backend and explicit experimental flags", () => {
  const runtime = { backend: "v86", v86: { relaxedFpu: true, fastPreset: true } };
  const game = { engineUrl: "/engine.wasm", manifest: { executable: "C:\\game.exe" } };
  const options = launchOptions(game, { runtime, executable: "C:\\game.exe", argumentsText: "-developer" });
  assert.equal(options.backend, "v86");
  assert.deepEqual(options.v86, runtime.v86);
  assert.equal(options.commandLine, '"C:\\game.exe" -developer');
  assert.equal(options.manifest, game.manifest);
});

test("a local folder keeps the separately stored server runtime configuration", () => {
  const runtime = { backend: "v86", v86: { relaxedFpu: false, fastPreset: false } };
  const serverGame = { configured: true, root: "/server/game" };
  const localFiles = new Map([["C:\\local.exe", {}]]);
  const localGame = { configured: true, manifest: { executable: "C:\\local.exe" }, localFiles };
  const first = launchOptions(serverGame, { runtime, executable: "C:\\server.exe" });
  const selected = launchOptions(localGame, { runtime, executable: "C:\\local.exe" });
  assert.equal(selected.backend, first.backend);
  assert.deepEqual(selected.v86, { relaxedFpu: false, fastPreset: false });
  assert.equal(selected.localFiles, localFiles);
  assert.equal(selected.manifest, localGame.manifest);
  assert.equal(selected.engineUrl, "/engine.wasm");
});

test("omitted runtime stays core and never enables relaxed FPU or the preset", () => {
  const options = launchOptions({}, { executable: "C:\\game.exe" });
  assert.equal(options.backend, "core");
  assert.equal(Object.hasOwn(options, "v86"), false);
  assert.equal(options.commandLine, undefined);
});
