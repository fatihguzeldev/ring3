import type { Snapshot, WorkerInput, WorkerOutput } from "./types.js";

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
const stop = element<HTMLButtonElement>("stop");
const state = element("state");
const error = element("error");
const windows = element("windows");
let worker: Worker | undefined;
let frames = 0;
let began = 0;
let windowSignature = "";

function send(message: WorkerInput): void {
  worker?.postMessage(message);
}

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
  state.textContent = "execution stopped";
  error.textContent = message;
  error.hidden = false;
  pause.disabled = true;
  resume.disabled = true;
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
  renderWindows(snapshot);
  if (finished) {
    windows.hidden = true;
    start.disabled = false;
  }
}

start.onclick = (): void => {
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
  const current = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
  worker = current;
  current.onmessage = ({ data }: MessageEvent<WorkerOutput>): void => {
    if (worker === current) receive(data);
  };
  current.onerror = (event): void => {
    if (worker === current) failed(event.message);
  };
  send({ type: "start", token });
};
pause.onclick = (): void => send({ type: "pause" });
resume.onclick = (): void => send({ type: "resume" });
stop.onclick = (): void => {
  worker?.terminate();
  worker = undefined;
  start.disabled = false;
  pause.disabled = true;
  resume.disabled = true;
  stop.disabled = true;
  windows.hidden = true;
  state.textContent = "stopped";
};
window.addEventListener("pagehide", (): void => worker?.terminate());
