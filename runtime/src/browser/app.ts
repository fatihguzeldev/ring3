import type { Snapshot, WorkerInput, WorkerOutput } from "./types.js";
import { KeyboardState, scanCode } from "./keyboard.js";
import { pointerDelta } from "./pointer.js";

function element<T extends HTMLElement>(id: string): T {
  const value = document.getElementById(id);
  if (!value) throw new Error(`missing ui element: ${id}`);
  return value as T;
}
const canvas = element<HTMLCanvasElement>("screen");
const context = canvas.getContext("2d");
const start = element<HTMLButtonElement>("start");
const pause = element<HTMLButtonElement>("pause");
const resume = element<HTMLButtonElement>("resume");
const mouseLock = element<HTMLButtonElement>("mouse-lock");
const stop = element<HTMLButtonElement>("stop");
const state = element("state");
const error = element("error");
const windows = element("windows");
let worker: Worker | undefined;
let frames = 0;
let began = 0;
let windowSignature = "";
let pointerButtons = 0;
let lastPointer: { x: number; y: number } | undefined;
const keyboard = new KeyboardState();

function send(message: WorkerInput): void {
  worker?.postMessage(message);
}

function sendMouse(relativeX = 0, relativeY = 0, wheelSteps = 0): void {
  send({ type: "mouse", relativeX, relativeY, buttons: pointerButtons, wheelSteps });
}

function releaseKeyboard(): void {
  const keys = keyboard.clear();
  if (keys) send({ type: "keyboard", keys });
}

function unlockPointer(): void {
  if (document.pointerLockElement === canvas) document.exitPointerLock();
}

function releasePointer(): void {
  if (pointerButtons !== 0) {
    pointerButtons = 0;
    sendMouse();
  }
  lastPointer = undefined;
}

function buttonBit(button: number): number {
  return [1, 4, 2][button] ?? 0;
}

canvas.onpointerenter = (event): void => {
  if (event.pointerType === "mouse" && document.pointerLockElement !== canvas)
    lastPointer = { x: event.clientX, y: event.clientY };
};
canvas.onpointermove = (event): void => {
  if (event.pointerType !== "mouse") return;
  const locked = document.pointerLockElement === canvas;
  const previous = lastPointer;
  lastPointer = locked ? undefined : { x: event.clientX, y: event.clientY };
  if (!worker || pause.disabled) return;
  const bounds = canvas.getBoundingClientRect();
  const delta = pointerDelta(
    event,
    previous,
    locked,
    canvas.width,
    canvas.height,
    bounds.width,
    bounds.height,
  );
  if (delta) sendMouse(...delta);
};
canvas.onpointerleave = (): void => {
  if (pointerButtons === 0) lastPointer = undefined;
};
canvas.onpointerdown = (event): void => {
  const bit = buttonBit(event.button);
  if (event.pointerType !== "mouse" || bit === 0 || !worker || pause.disabled) return;
  canvas.focus();
  pointerButtons |= bit;
  lastPointer = { x: event.clientX, y: event.clientY };
  canvas.setPointerCapture(event.pointerId);
  sendMouse();
  event.preventDefault();
};
canvas.onpointerup = (event): void => {
  const bit = buttonBit(event.button);
  if (event.pointerType !== "mouse" || !(pointerButtons & bit)) return;
  pointerButtons &= ~bit;
  sendMouse();
  if (pointerButtons === 0) {
    if (canvas.hasPointerCapture(event.pointerId)) canvas.releasePointerCapture(event.pointerId);
    lastPointer = undefined;
  }
};
canvas.onpointercancel = (event): void => {
  if (event.pointerType === "mouse") releasePointer();
};
canvas.oncontextmenu = (event): void => {
  if (worker) event.preventDefault();
};
document.addEventListener("pointerlockchange", () => {
  lastPointer = undefined;
  mouseLock.textContent = document.pointerLockElement === canvas ? "release mouse" : "lock mouse";
  if (document.pointerLockElement !== canvas) {
    releasePointer();
    releaseKeyboard();
  }
});
mouseLock.onclick = (): void => {
  if (document.pointerLockElement === canvas) {
    unlockPointer();
    return;
  }
  canvas.focus();
  const blocked = (): void => {
    error.textContent = "mouse lock was blocked by the browser.";
    error.hidden = false;
  };
  try {
    void Promise.resolve(canvas.requestPointerLock()).catch(blocked);
  } catch {
    blocked();
  }
};
canvas.onwheel = (event): void => {
  if (!worker || pause.disabled || document.activeElement !== canvas || event.deltaY === 0) return;
  sendMouse(0, 0, Math.sign(-event.deltaY));
  event.preventDefault();
};
canvas.onkeydown = (event): void => {
  if (!worker || pause.disabled || scanCode(event.code) === undefined) return;
  const keys = keyboard.set(event.code, true);
  if (keys) send({ type: "keyboard", keys });
  event.preventDefault();
};
canvas.onkeyup = (event): void => {
  if (scanCode(event.code) === undefined) return;
  const keys = keyboard.set(event.code, false);
  if (keys) send({ type: "keyboard", keys });
  event.preventDefault();
};
canvas.addEventListener("blur", releaseKeyboard);
window.addEventListener("blur", () => {
  unlockPointer();
  releasePointer();
  releaseKeyboard();
});

function renderWindows(snapshot: Snapshot): void {
  const signature = JSON.stringify(snapshot.windows);
  if (signature === windowSignature) return;
  windowSignature = signature;
  windows.replaceChildren();
  for (const window of snapshot.windows.filter(
    (entry) => entry.parent === 0 && entry.style & 0x10000000,
  )) {
    const section = document.createElement("div");
    section.className = "guest-window";
    const heading = document.createElement("h3");
    heading.textContent = window.title || "game window";
    section.append(heading);
    for (const child of snapshot.windows.filter(
      (entry) => entry.parent === window.hwnd && entry.class === 0x80 && entry.style & 0x10000000,
    )) {
      const button = document.createElement("button");
      button.textContent = child.title.replaceAll("&", "") || `button ${child.id}`;
      button.disabled = Boolean(child.style & 0x08000000);
      button.onclick = (): void => send({ type: "button", hwnd: child.hwnd });
      section.append(button);
    }
    const activate = document.createElement("button");
    activate.textContent = "send activation message";
    activate.disabled = Boolean(window.style & 0x08000000);
    activate.onclick = (): void => send({ type: "activate", hwnd: window.hwnd });
    section.append(activate);
    windows.append(section);
  }
  windows.hidden = windows.childElementCount === 0;
}

function failed(message: string): void {
  unlockPointer();
  releasePointer();
  releaseKeyboard();
  state.textContent = "execution stopped";
  error.textContent = message;
  error.hidden = false;
  pause.disabled = true;
  resume.disabled = true;
  mouseLock.disabled = true;
  worker?.terminate();
  worker = undefined;
  start.disabled = false;
  stop.disabled = true;
  windows.hidden = true;
}

function receive(message: WorkerOutput): void {
  if (message.type === "error") {
    failed(message.message);
    return;
  }
  if (message.type === "frame") {
    if (!context || message.rgba.byteLength !== message.width * message.height * 4) {
      failed("invalid frame.");
      return;
    }
    canvas.width = message.width;
    canvas.height = message.height;
    context.putImageData(
      new ImageData(new Uint8ClampedArray(message.rgba), message.width, message.height),
      0,
      0,
    );
    element("empty").hidden = true;
    element("frames").textContent = String(++frames);
    return;
  }
  const { snapshot, paused } = message;
  const finished = snapshot.state === "stopped" || snapshot.state === "exited";
  const labels = {
    ready: "starting",
    running: "running",
    waiting: "waiting for window response",
    file: "loading file",
    exited: "program exited",
    stopped: "core limit reached",
  };
  state.textContent = paused ? "paused" : labels[snapshot.state];
  element("steps").textContent = (
    Number(snapshot.instructions) + Number(snapshot.apiCalls)
  ).toLocaleString("en-US");
  element("elapsed").textContent = `${Math.floor((performance.now() - began) / 1000)} s`;
  element("diagnostics").textContent =
    `${snapshot.reason}\nEIP: 0x${snapshot.eip.toString(16).padStart(8, "0")}\nCPU: ${snapshot.instructions} · API: ${snapshot.apiCalls}\nexecution: browser worker / webassembly`;
  pause.disabled = paused || finished || snapshot.state === "ready";
  resume.disabled = !paused || finished;
  mouseLock.disabled = paused || finished || snapshot.state === "ready";
  renderWindows(snapshot);
  if (finished) {
    unlockPointer();
    releaseKeyboard();
    windows.hidden = true;
    start.disabled = false;
  }
}

start.onclick = (): void => {
  unlockPointer();
  releasePointer();
  releaseKeyboard();
  worker?.terminate();
  const token = document.documentElement.dataset["token"];
  if (!token || token === "RING3_SESSION_TOKEN") {
    failed("open this page through the local server.");
    return;
  }
  if (!context) {
    failed("canvas is not supported.");
    return;
  }
  frames = 0;
  began = performance.now();
  windowSignature = "";
  error.hidden = true;
  windows.hidden = true;
  windows.replaceChildren();
  context.clearRect(0, 0, canvas.width, canvas.height);
  element("frames").textContent = "0";
  element("steps").textContent = "—";
  element("empty").textContent = "waiting for the first frame…";
  element("empty").hidden = false;
  state.textContent = "starting";
  start.disabled = true;
  stop.disabled = false;
  pause.disabled = true;
  resume.disabled = true;
  mouseLock.disabled = true;
  const current = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
  worker = current;
  canvas.focus();
  current.onmessage = ({ data }: MessageEvent<WorkerOutput>): void => {
    if (worker === current) receive(data);
  };
  current.onerror = (event): void => {
    if (worker === current) failed(event.message);
  };
  send({ type: "start", token });
};
pause.onclick = (): void => {
  unlockPointer();
  releasePointer();
  releaseKeyboard();
  send({ type: "pause" });
};
resume.onclick = (): void => {
  canvas.focus();
  send({ type: "resume" });
};
stop.onclick = (): void => {
  unlockPointer();
  releasePointer();
  releaseKeyboard();
  worker?.terminate();
  worker = undefined;
  start.disabled = false;
  pause.disabled = true;
  resume.disabled = true;
  mouseLock.disabled = true;
  stop.disabled = true;
  windows.hidden = true;
  state.textContent = "stopped";
};
window.addEventListener("pagehide", (): void => {
  unlockPointer();
  releasePointer();
  releaseKeyboard();
  worker?.terminate();
});
