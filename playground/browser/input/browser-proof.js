import { InputHost } from "../input.js";

const canvas = document.getElementById("input-canvas");
const state = document.getElementById("state");
const log = document.getElementById("events");
const counts = document.getElementById("counts");
const records = [];
let batches = 0;
let inputs = 0;
let host;
const identity = (element) => element === window ? "window" : element === document
  ? "document" : element?.id || element?.tagName?.toLowerCase() || null;
const snapshot = () => ({
  document: {
    hasFocus: document.hasFocus(), visible: !document.hidden,
    activeElement: identity(document.activeElement), pointerLock: identity(document.pointerLockElement),
  },
  host: host && {
    enabled: host.enabled, blocked: host.blocked, windowFocused: host.windowFocused,
    focused: host.focused, available: host.available, active: host.active,
    pressedKeys: host.keys.size, pressedButtons: host.buttons.size,
  },
});
function record(kind, detail) {
  records.push({ atMs: Math.round(performance.now() * 1000) / 1000, kind, ...detail });
  if (records.length > 30) records.shift();
  log.textContent = records.map((row) => JSON.stringify(row)).join("\n");
  log.scrollTop = log.scrollHeight;
  state.textContent = JSON.stringify(snapshot(), null, 2);
  counts.textContent = `${batches} accepted batches · ${inputs} accepted input events`;
}
host = new InputHost(canvas, {
  send(events) {
    batches++;
    inputs += events.length;
    record("accepted batch", { batch: batches, events: events.slice(0, 16), omitted: Math.max(0, events.length - 16) });
  },
  onDiagnostic(message) { record("diagnostic", { message }); },
});

// observe after the real InputHost listeners; never manufacture or forward test events.
for (const type of ["keydown", "keyup", "mousedown", "mouseup", "mousemove", "wheel", "click"])
  window.addEventListener(type, (event) => {
    const detail = {
      type, target: identity(event.target), key: event.key ?? null, code: event.code ?? null,
      keyCode: event.keyCode ?? null, location: event.location ?? null, repeat: event.repeat ?? null,
      button: event.button ?? null, buttons: event.buttons ?? null,
      isTrusted: event.isTrusted, cancelable: event.cancelable,
    };
    queueMicrotask(() => record("DOM event", { ...detail, defaultPrevented: event.defaultPrevented, state: snapshot() }));
  });
for (const type of ["focus", "blur"])
  window.addEventListener(type, (event) => {
    const target = identity(event.target);
    queueMicrotask(() => record("focus history", { type, target, isTrusted: event.isTrusted, state: snapshot() }));
  }, true);
document.addEventListener("visibilitychange", () => record("visibility", { state: snapshot() }));
document.addEventListener("pointerlockchange", () => record("pointer lock", { state: snapshot() }));
document.getElementById("start").addEventListener("click", () => {
  host.enable();
  record("control", { action: "enable", state: snapshot() });
});
document.getElementById("disable").addEventListener("click", () => {
  host.disable();
  record("control", { action: "disable", state: snapshot() });
});
document.getElementById("clear").addEventListener("click", () => {
  records.length = 0;
  record("control", { action: "clear", state: snapshot() });
});
window.addEventListener("pagehide", () => host.close(), { once: true });
const drawing = canvas.getContext("2d");
drawing.fillStyle = "#e4e5e7";
drawing.font = "20px system-ui";
drawing.fillText("real keyboard and mouse input", 32, 96);
drawing.font = "16px system-ui";
drawing.fillText("press ArrowDown, Enter, or click after start", 32, 132);
record("ready", { state: snapshot() });
