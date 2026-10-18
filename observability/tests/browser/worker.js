import { createCapture, epochNowNs } from "/observability/typescript/dist/index.js";

self.onmessage = async ({ data }) => {
  const capture = createCapture({
    level: data.level,
    producerId: 2,
    output: {
      async write(bytes) {
        postMessage({ type: "bytes", bytes: bytes.buffer }, [bytes.buffer]);
      },
      async flush() {},
    },
  });
  let error;
  try {
    const observer = capture.observer("host.worker");
    const bytes = await observer.scopeAsync("engine.fetch", {}, async () => {
      const response = await fetch("/engine.wasm");
      if (!response.ok) throw new Error(`engine HTTP ${response.status}`);
      return response.arrayBuffer();
    });
    const { instance } = await observer.scopeAsync("engine.instantiate", {}, () =>
      WebAssembly.instantiate(bytes, { ring3_observability: { now_ns: epochNowNs } }),
    );
    const api = Object.fromEntries(
      Object.entries(instance.exports)
        .filter(([name]) => name.startsWith("ring3_abi_v1_"))
        .map(([name, value]) => [name.slice(13), value]),
    );
    const call = (name, ...args) => {
      const code = api[name](...args);
      if (code !== 0) throw new Error(`${name}: ${code}`);
    };
    call("open", 16, 0x12345678, 0x76543210);
    const base = api.arena_ptr() >>> 0,
      transfer = () => new Uint8Array(instance.exports.memory.buffer, base + 140, 4096);
    const drain = (operation) => {
      call(operation);
      const bytes = transfer().slice(),
        header = new DataView(bytes.buffer),
        length = header.getUint32(0, true);
      if (length > 4080) throw new Error("invalid trace length");
      const chunk = {
        bytes: bytes.slice(16, 16 + length),
        finished: header.getUint32(4, true) !== 0,
        droppedEvents: header.getBigUint64(8, true),
      };
      if (chunk.finished) {
        call("observability_status");
        chunk.openScopes = new DataView(transfer().buffer, transfer().byteOffset, 40).getBigUint64(
          32,
          true,
        );
      }
      return chunk;
    };
    capture.attachEngine(
      {
        supported: typeof api.configure_observability === "function",
        configure(config) {
          const context = new TextEncoder().encode(config.contexts.join("\n"));
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
        drain: () => drain("observability_drain"),
        finish: () => drain("observability_finish"),
      },
      { producerId: 3 },
    );
    call("map", 0x1000, 1, 7);
    transfer().set([0xb8, 42, 0, 0, 0]);
    call("upload", 0x1000, 5);
    const descriptor = new DataView(transfer().buffer, transfer().byteOffset, 8);
    descriptor.setUint32(0, 0x1000, true);
    descriptor.setUint32(4, 5, true);
    observer.scope("fixture.compile", {}, () => call("compile", 1));
    observer.counter("fixture.compilations", 1);
  } catch (problem) {
    error = problem.message;
    capture.markIncomplete(error);
  }
  const result = await capture.finish(error ? "failed" : "complete", error);
  postMessage({ type: "complete", result, ...(error ? { error } : {}) });
};
