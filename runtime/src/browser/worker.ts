import { Bridge } from "./bridge.js";
import type { Manifest, Snapshot, WorkerInput, WorkerOutput } from "./types.js";

const scope = self as unknown as {
  onmessage: ((event: MessageEvent<WorkerInput>) => void) | null;
  postMessage(message: WorkerOutput, transfer?: Transferable[]): void;
};
let bridge: Bridge | undefined;
let manifest: Manifest;
let snapshot: Snapshot;
let token = "";
let origin = 0;
let paused = false;
let running = false;
let started = false;
let remaining = 3_000_000_000;
let lastReport = 0;
const commands: Extract<WorkerInput, { hwnd: number }>[] = [];

const yieldTask = (milliseconds = 0): Promise<void> =>
  new Promise((resolve) => setTimeout(resolve, milliseconds));

function report(note = ""): void {
  scope.postMessage({ type: "status", snapshot, paused, note });
  lastReport = performance.now();
}

async function resource(path: string): Promise<Response> {
  const response = await fetch(path, { headers: { "X-Ring3-Token": token }, cache: "no-store" });
  if (!response.ok) throw new Error(`could not read local file (${response.status}): ${path}`);
  return response;
}

async function file(index: number): Promise<Uint8Array> {
  const entry = manifest.files[index];
  if (!entry || entry.size > 128 * 1024 * 1024) throw new Error("file transfer limit exceeded.");
  const response = await resource(`/files/${index}`);
  if (Number(response.headers.get("content-length")) !== entry.size)
    throw new Error("file size changed.");
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (bytes.length !== entry.size) throw new Error("incomplete file transfer.");
  return bytes;
}

async function start(value: string): Promise<void> {
  if (started) throw new Error("session already started.");
  started = true;
  token = value;
  manifest = (await (await resource("/manifest")).json()) as Manifest;
  if (!Array.isArray(manifest.files) || manifest.files.length > 100_000)
    throw new Error("invalid file list.");
  const wasm = await (await resource("/core.wasm")).arrayBuffer();
  bridge = new Bridge((await WebAssembly.instantiate(wasm, {})).instance);
  snapshot = bridge.command(0, 0, manifest);
  report("loading program files.");
  for (let index = 0; index < manifest.files.length; index++) {
    if (manifest.files[index]?.role !== "data") bridge.command(1, index, await file(index));
  }
  snapshot = bridge.command(2);
  origin = performance.now();
  report();
  await pump();
}

function postCommands(): void {
  if (!bridge) return;
  for (const command of commands.splice(0)) {
    snapshot = bridge.command(6);
    const window = snapshot.windows.find((entry) => entry.hwnd === command.hwnd);
    if (!window || !(window.style & 0x10000000) || window.style & 0x08000000) continue;
    const time = Math.floor(performance.now() - origin) >>> 0;
    if (command.type === "button" && window.class === 0x80 && window.parent !== 0) {
      snapshot = bridge.command(5, 0, {
        hwnd: window.parent,
        message: 0x111,
        wparam: window.id & 0xffff,
        lparam: window.hwnd,
        time,
      });
    } else if (command.type === "activate" && window.parent === 0) {
      snapshot = bridge.command(5, 0, {
        hwnd: window.hwnd,
        message: 0x1c,
        wparam: 1,
        lparam: 0,
        time,
      });
    }
  }
}

async function supply(): Promise<void> {
  const pending = snapshot.pending;
  if (!bridge || !pending) throw new Error("missing file request.");
  const index = manifest.files.findIndex(
    (entry) => entry.path.toLowerCase() === pending.path.toLowerCase(),
  );
  if (index < 0 || manifest.files[index]?.size !== pending.size)
    throw new Error(`file not listed: ${pending.path}`);
  const bytes = await file(index);
  const input = new Uint8Array(bytes.length + 8);
  new DataView(input.buffer).setBigUint64(0, BigInt(pending.id), true);
  input.set(bytes, 8);
  snapshot = bridge.command(4, 0, input);
}

async function pump(): Promise<void> {
  if (running || !bridge) return;
  running = true;
  try {
    while (!paused) {
      postCommands();
      if (snapshot.state === "file") await supply();
      if (paused) break;
      if (["exited", "stopped"].includes(snapshot.state)) {
        report();
        break;
      }
      if (snapshot.reason === "Some(WaitingForMessage)") {
        report("the game is waiting for a window response.");
        break;
      }
      const before = BigInt(snapshot.instructions) + BigInt(snapshot.apiCalls);
      snapshot = bridge.command(3, Math.min(100_000, remaining), {
        elapsedMs: Math.floor(performance.now() - origin),
      });
      const consumed = Number(BigInt(snapshot.instructions) + BigInt(snapshot.apiCalls) - before);
      remaining -= consumed;
      const rgba = bridge.frame(snapshot);
      if (rgba && snapshot.frame)
        scope.postMessage({ type: "frame", ...snapshot.frame, rgba }, [rgba]);
      if (performance.now() - lastReport > 250) report();
      if (remaining === 0) {
        paused = true;
        report("execution limit reached; you can resume.");
        break;
      }
      if (consumed === 0 && snapshot.state === "running")
        throw new Error("core returned without making progress.");
      await yieldTask(snapshot.state === "waiting" ? 16 : 0);
    }
    if (paused) report("paused.");
  } finally {
    running = false;
  }
}

function fail(error: unknown): void {
  paused = true;
  scope.postMessage({
    type: "error",
    message: error instanceof Error ? error.message : String(error),
  });
}

scope.onmessage = ({ data }): void => {
  if (data.type === "start") {
    void start(data.token).catch(fail);
    return;
  }
  if (data.type === "pause") {
    paused = true;
    if (bridge) report("paused.");
    return;
  }
  if (data.type === "button" || data.type === "activate") commands.push(data);
  paused = false;
  if (remaining === 0) remaining = 3_000_000_000;
  void pump().catch(fail);
};
