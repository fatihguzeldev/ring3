import assert from "node:assert/strict";
import { test } from "node:test";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import {
  categories,
  createCapture,
  encodeCategorySelection,
  epochNowNs,
} from "../../typescript/dist/index.js";
import { execFile } from "node:child_process";
import { promisify } from "node:util";

const root = resolve(import.meta.dirname, "../../..");
const artifact =
  process.env.RING3_OBSERVABILITY_ENGINE ??
  resolve(root, "target/observability-verification/engine.wasm");
const bytes = await readFile(artifact);
const module = new WebAssembly.Module(bytes);
const key = [0x12345678, 0x76543210];

function engine() {
  const instance = new WebAssembly.Instance(module, {
    ring3_observability: { now_ns: epochNowNs },
  });
  const api = Object.fromEntries(
    Object.entries(instance.exports)
      .filter(([name]) => name.startsWith("ring3_abi_v1_"))
      .map(([name, value]) => [name.slice(13), value]),
  );
  const call = (name, ...args) => assert.equal(api[name](...args), 0, name);
  call("open", 16, ...key);
  const base = api.arena_ptr() >>> 0;
  const transfer = () => new Uint8Array(instance.exports.memory.buffer, base + 140, 4096);
  const chunk = (operation) => {
    call(operation);
    const packet = transfer().slice(),
      header = new DataView(packet.buffer),
      size = header.getUint32(0, true);
    assert.ok(size <= 4080);
    call("observability_status");
    const status = new DataView(transfer().buffer, transfer().byteOffset, 40);
    return {
      bytes: packet.slice(16, 16 + size),
      finished: header.getUint32(4, true) !== 0,
      droppedEvents: header.getBigUint64(8, true),
      openScopes: status.getBigUint64(32, true),
    };
  };
  return {
    api,
    call,
    transfer,
    port: {
      supported: typeof api.configure_observability === "function",
      configure(config) {
        const context = new TextEncoder().encode(encodeCategorySelection(config.categories));
        transfer().set(context);
        call(
          "configure_observability",
          ["off", "basic", "detailed"].indexOf(config.level),
          config.maxBytes,
          config.sequenceId,
          Number(config.trackUuid & 0xffffffffn),
          Number(config.trackUuid >> 32n),
          context.length,
        );
      },
      drain: () => chunk("observability_drain"),
      finish: () => chunk("observability_finish"),
    },
  };
}

test("real Wasm codegen and host scopes share a complete standard capture without a game", async () => {
  const parts = [],
    capture = createCapture({
      level: "detailed",
      producerId: 31,
      output: {
        async write(bytes) {
          parts.push(bytes.slice());
        },
        async flush() {},
      },
    });
  const e = engine();
  capture.attachEngine(e.port, { producerId: 32, maxBytes: 128 * 1024 });
  e.call("map", 0x1000, 1, 7);
  e.transfer().set([0xb8, 1, 0, 0, 0]);
  e.call("upload", 0x1000, 5);
  const observer = capture.observer(categories.hostBrowser);
  for (let i = 0; i < 48; i++) {
    const descriptor = new DataView(e.transfer().buffer, e.transfer().byteOffset, 8);
    descriptor.setUint32(0, 0x1000, true);
    descriptor.setUint32(4, 5, true);
    observer.scope("compile-request", { iteration: i }, () => e.call("compile", 1));
  }
  const result = await capture.finish();
  assert.equal(result.complete, true, JSON.stringify(result));
  assert.equal(result.engine, "finished");
  assert.equal(result.droppedEvents, 0);
  assert.equal(result.droppedChunks, 0);
  const output = Buffer.concat(parts);
  assert.ok(output.length > 4096, "must drain beyond the first transfer chunk");
  const directory = resolve(root, "target/observability-verification");
  await mkdir(directory, { recursive: true });
  await writeFile(resolve(directory, "engine-host.pftrace"), output);
  e.call("close");
});

test("feature Wasm capture configuration validates without starting a guest", () => {
  const e = engine();
  assert.notEqual(e.api.configure_observability(3, 65536, 40, 1, 40, 0), 0);
  for (const input of ["unknown.category", "observability", "engine.cpu\nengine.cpu"]) {
    const selection = new TextEncoder().encode(input);
    e.transfer().set(selection);
    assert.notEqual(
      e.api.configure_observability(1, 65536, 40, 1, 40, selection.length),
      0,
      `invalid category selection: ${input}`,
    );
  }
  e.call("configure_observability", 1, 65536, 40, 1, 40, 0);
  assert.notEqual(
    e.api.configure_observability(1, 65536, 40, 1, 40, 0),
    0,
    "active capture cannot be discarded",
  );
  let chunk = e.port.finish();
  while (!chunk.finished) chunk = e.port.drain();
  e.call("configure_observability", 0, 65536, 41, 1, 41, 0);
  e.call("close");
});

test("actual Wasm catalog selection preserves all, none, parent and subset semantics", async () => {
  const processor =
    process.env.RING3_TRACE_PROCESSOR ??
    resolve(root, "target/observability-tools/trace_processor_shell");
  const directory = resolve(root, "target/observability-verification/catalog");
  await mkdir(directory, { recursive: true });
  const execute = promisify(execFile);
  const cases = [
    { name: "all", selection: undefined, decode: true, codegen: true, host: true },
    { name: "none", selection: [], decode: false, codegen: false, host: false },
    { name: "parent", selection: [categories.engineCpu], decode: true, codegen: true, host: false },
    {
      name: "decode",
      selection: [categories.engineCpuDecode],
      decode: true,
      codegen: false,
      host: false,
    },
    {
      name: "host",
      selection: [categories.hostBrowser],
      decode: false,
      codegen: false,
      host: true,
    },
  ];
  for (const item of cases) {
    const parts = [];
    const capture = createCapture({
      level: "detailed",
      categories: item.selection,
      producerId: 31,
      output: {
        async write(bytes) {
          parts.push(bytes.slice());
        },
        async flush() {},
      },
    });
    const e = engine();
    try {
      capture.attachEngine(e.port, { producerId: 32 });
      e.call("map", 0x1000, 1, 7);
      e.transfer().set([0xb8, 42, 0, 0, 0, 0xeb, 0xfe]);
      e.call("upload", 0x1000, 7);
      const descriptor = new DataView(e.transfer().buffer, e.transfer().byteOffset, 4);
      descriptor.setUint32(0, 0x1000, true);
      capture.observer(categories.hostBrowser).scope("host.compile", {}, () => {
        e.call("compile_entries", 1, 0);
        const block = new DataView(e.transfer().buffer, e.transfer().byteOffset, 8);
        block.setUint32(0, 0x1000, true);
        block.setUint32(4, 7, true);
        e.call("compile", 1);
      });
      assert.equal((await capture.finish()).complete, true, item.name);
      const trace = resolve(directory, `${item.name}.pftrace`);
      await writeFile(trace, Buffer.concat(parts));
      const query = async (sql) => (await execute(processor, ["query", trace, sql])).stdout.trim();
      for (const [category, expected] of [
        [categories.engineCpuDecode, item.decode],
        [categories.engineCpuCodegen, item.codegen],
        [categories.hostBrowser, item.host],
      ]) {
        const count = await query(`SELECT count(*) AS n FROM slice WHERE category = '${category}'`);
        assert.equal(Number(count.split("\n").at(-1)) > 0, expected, `${item.name}: ${category}`);
      }
      assert.equal(
        Number(
          (await query("SELECT count(*) AS n FROM slice WHERE name = 'capture.finished'"))
            .split("\n")
            .at(-1),
        ),
        2,
        `${item.name}: lifecycle metadata is retained`,
      );
      assert.equal(
        await query(
          "SELECT name, value FROM stats WHERE severity IN ('error','data_loss') AND value > 0",
        ),
        '"name","value"',
      );
    } finally {
      await capture.finish("failed");
      e.call("close");
    }
  }
});
