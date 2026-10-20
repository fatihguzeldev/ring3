import { createSession, selectedFolder } from "/sdk/index.js";
import { VisibilityController } from "./visibility.js";
import { CategoryPicker } from "./observability/categories.js";
import { commandLineForArguments } from "./command-line.js";
import { frameRateLabel } from "./frame-rate.js";

const $ = (id) => document.getElementById(id);
const ui = Object.fromEntries(
  [
    "state",
    "game-folder",
    "choose-folder",
    "folder-name",
    "executable",
    "command-line",
    "run",
    "pause",
    "stop",
    "capture",
    "fullscreen",
    "mute",
    "volume",
    "stage",
    "session-display",
    "message",
    "metrics",
    "frame-rate",
    "log",
    "clear-log",
    "display-hint",
    "pointer-status",
    "observation-level",
    "observation-categories",
    "observation-selection",
    "observation-path",
  ].map((id) => [id, $(id)]),
);
const categoryPicker = new CategoryPicker(
  ui["observation-categories"],
  ui["observation-selection"],
);
const entries = [];
let game = null,
  ready = false,
  muted = false,
  captured = false,
  sessionExists = false,
  lastStatus = "",
  lastDiagnostic = 0;
let visibility;
let frameRateTimer;
const serialize = (value) =>
  JSON.stringify(value, (_key, item) => (typeof item === "bigint" ? item.toString() : item));
function report(event) {
  fetch("/api/diagnostics", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: serialize(event),
  }).catch(() => {});
}
function log(message, detail) {
  const text = `${new Date().toLocaleTimeString()}  ${message}${detail ? `\n${typeof detail === "string" ? detail : serialize(detail)}` : ""}`;
  entries.push(text);
  if (entries.length > 250) entries.shift();
  const atBottom = ui.log.scrollHeight - ui.log.scrollTop - ui.log.clientHeight < 40;
  ui.log.textContent = entries.join("\n");
  if (atBottom) ui.log.scrollTop = ui.log.scrollHeight;
  report({ type: "diagnostic", message, detail });
}
function status(info) {
  clearTimeout(frameRateTimer);
  ui["frame-rate"].textContent = frameRateLabel(info);
  if (["running", "waiting"].includes(info.state)) {
    const received = performance.now();
    frameRateTimer = setTimeout(() => {
      ui["frame-rate"].textContent = frameRateLabel(info, Math.max(2000, performance.now() - received));
    }, 2000);
  }
  if (info.state === "stopped") sessionExists = false;
  visibility?.status(info);
  ui.state.textContent = info.state;
  ui.state.dataset.state = info.state;
  ui.message.textContent = info.message || info.state;
  const active = ["checking", "starting", "loading", "running", "paused", "waiting"].includes(
    info.state,
  );
  ui.run.disabled =
    active ||
    sessionExists ||
    !ready ||
    info.state === "error" ||
    !game?.configured ||
    !ui.executable.value ||
    !navigator.gpu;
  ui.stop.disabled = !sessionExists;
  ui.pause.disabled = !["running", "paused"].includes(info.state);
  ui.pause.textContent = info.state === "paused" ? "resume" : "pause";
  ui.capture.disabled = info.state !== "running";
  const frameProbe = document.getElementById("frame-probe");
  if (frameProbe) frameProbe.disabled = info.state !== "running";
  for (const field of [
    ui["game-folder"],
    ui["choose-folder"],
    ui.executable,
    ui["command-line"],
    ui["observation-level"],
  ])
    field.disabled = active;
  categoryPicker.setDisabled(active || ui["observation-level"].value === "off");
  ui["display-hint"].hidden = active || info.state === "exited";
  const parts = [];
  if (info.pc !== undefined) parts.push(`pc 0x${Number(info.pc).toString(16)}`);
  if (info.stats)
    for (const [name, value] of Object.entries(info.stats))
      if (["number", "bigint", "string"].includes(typeof value)) parts.push(`${name} ${value}`);
  ui.metrics.textContent = parts.join(" · ");
  const key = `${info.state}:${info.message}`;
  if (key !== lastStatus) {
    lastStatus = key;
    log(info.message || info.state);
  }
  if (performance.now() - lastDiagnostic > 1000 || !active) {
    lastDiagnostic = performance.now();
    report({ type: "status", ...info });
  }
}
const session = createSession({
  container: ui["session-display"],
  onStatus: status,
  onDiagnostic: log,
  onObservation: (info) => {
    const state =
      info.sinkError || info.recordingError
        ? `error: ${info.sinkError ?? info.recordingError}`
        : info.state;
    ui["observation-path"].textContent =
      `${info.path} · ${state} · dropped events ${info.droppedEvents}, chunks ${info.droppedChunks}${["host-only", "unsupported"].includes(info.engine) ? " · engine instrumentation unavailable" : ""}`;
  },
  onAudioStats: (stats) => {
    ui.mute.title = `${stats.queuedFrames} frames queued · ${stats.underruns} underruns`;
  },
  onCapture: (value) => {
    captured = value;
    ui.capture.textContent = value ? "release mouse" : "capture mouse";
    ui["pointer-status"].textContent = value
      ? "mouse captured · Esc releases it"
      : "click the display to focus · Esc releases the mouse";
  },
});
visibility = new VisibilityController(session, {
  hidden: () => document.hidden,
  onError: (error) => log(error.message),
  onDiagnostic: log,
});
window.playground = Object.freeze({ session });

async function loadGame(body) {
  const response = await fetch(
    "/api/game",
    body
      ? {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify(body),
        }
      : {},
  );
  const result = await response.json();
  if (!response.ok)
    throw new Error(result.error || result.message || `game host returned HTTP${response.status}`);
  return showGame(result);
}
function showGame(result) {
  game = result;
  ui["folder-name"].textContent = result.root || "no folder selected";
  ui.executable.replaceChildren();
  for (const executable of result.executables || []) {
    const option = document.createElement("option");
    option.value = executable.path;
    option.textContent = executable.label || executable.path;
    ui.executable.append(option);
  }
  if (result.selectedExecutable) ui.executable.value = result.selectedExecutable;
  if (ui.executable.options.length === 0) {
    const option = document.createElement("option");
    option.value = "";
    option.textContent = "no executable found in this folder";
    ui.executable.append(option);
  }
  ready = Boolean(navigator.gpu);
  status({
    state: "idle",
    message: result.configured ? "local game files ready" : "select the local game folder",
  });
  return result;
}
ui["choose-folder"].addEventListener("click", () => ui["game-folder"].click());
ui["game-folder"].addEventListener("change", () => {
  try {
    const selected = selectedFolder(ui["game-folder"].files);
    if (selected) showGame(selected);
  } catch (error) {
    log(error.message);
    ui.message.textContent = error.message;
  } finally {
    ui["game-folder"].value = "";
  }
});
ui.executable.addEventListener("change", () => {
  if (game.localFiles) {
    game.selectedExecutable = ui.executable.value;
    game.manifest.executable = ui.executable.value;
  } else loadGame({ executable: ui.executable.value }).catch((error) => log(error.message));
});
ui.run.addEventListener("click", () => {
  if (sessionExists || !ready || !game?.configured) return;
  sessionExists = true;
  visibility.reset();
  const observability = {
    level: ui["observation-level"].value,
    categories: categoryPicker.categories(),
  };
  if (observability.level === "off") ui["observation-path"].textContent = "recording off";
  session
    .launch({
      executable: ui.executable.value,
      commandLine: commandLineForArguments(ui.executable.value, ui["command-line"].value),
      engineUrl: game.engineUrl || "/engine.wasm",
      manifest: game.manifest,
      localFiles: game.localFiles,
      observability,
      frameProbe: new URLSearchParams(location.search).get("frameProbe") === "1",
    })
    .catch((error) => log(error.message));
});
ui["observation-level"].addEventListener("change", () =>
  categoryPicker.setDisabled(
    ui["observation-level"].disabled || ui["observation-level"].value === "off",
  ),
);
if (new URLSearchParams(location.search).get("frameProbe") === "1") {
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = "capture 8 frames";
  button.id = "frame-probe";
  button.disabled = true;
  button.addEventListener("mousedown", event => event.preventDefault());
  button.addEventListener("click", () => session.armFrameProbe());
  ui.capture.after(button);
}
ui.pause.addEventListener("click", () => visibility.manualToggle());
ui.stop.addEventListener("click", () => session.stop().catch((error) => log(error.message)));
ui.capture.addEventListener("click", () => {
  if (captured) document.exitPointerLock();
  else session.captureMouse().catch((error) => log(error.message));
});
ui.fullscreen.addEventListener("click", async () => {
  try {
    if (document.fullscreenElement) await document.exitFullscreen();
    else await ui.stage.requestFullscreen();
  } catch (error) {
    log(error.message);
  }
});
ui.mute.addEventListener("click", () => {
  muted = !muted;
  ui.mute.setAttribute("aria-pressed", String(muted));
  ui.mute.textContent = muted ? "unmute" : "mute";
  session.setVolume(ui.volume.value / 100, muted);
});
ui.volume.addEventListener("input", () => session.setVolume(ui.volume.value / 100, muted));
ui["clear-log"].addEventListener("click", (event) => {
  event.preventDefault();
  entries.length = 0;
  ui.log.textContent = "";
});
document.addEventListener("visibilitychange", () => visibility.changed());
window.addEventListener("pagehide", () => clearTimeout(frameRateTimer));
window.addEventListener("error", (event) => log(event.message));
window.addEventListener("unhandledrejection", (event) =>
  log(event.reason?.message || String(event.reason)),
);
loadGame()
  .then(() => {
    ready = Boolean(navigator.gpu);
    status({
      state: "ready",
      message: game.configured
        ? "ready · run initializes the engine and graphics"
        : "select the local game folder",
    });
  })
  .catch((error) => {
    status({ state: "error", message: error.message });
  });
if (!navigator.gpu) log("WebGPU is unavailable in this browser; rendering cannot start");
