import { commandLineForArguments } from "./command-line.js";

export function launchOptions(game, { runtime, executable, argumentsText = "", observability, frameProbe = false } = {}) {
  return {
    backend: runtime?.backend ?? "core",
    ...(runtime?.backend === "v86" && runtime.v86 ? { v86: { ...runtime.v86 } } : {}),
    executable,
    commandLine: commandLineForArguments(executable, argumentsText),
    engineUrl: game.engineUrl || "/engine.wasm",
    manifest: game.manifest,
    localFiles: game.localFiles,
    observability,
    frameProbe,
  };
}
