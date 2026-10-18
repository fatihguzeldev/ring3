import assert from "node:assert/strict";
let input = "";
for await (const chunk of process.stdin) input += chunk;
const modules = JSON.parse(input);
const contexts = modules.map((hex) => {
  const bytes = Buffer.from(hex, "hex");
  assert.equal(WebAssembly.validate(bytes), true);
  const module = new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module), [
    { module: "env", name: "memory", kind: "memory" },
  ]);
  const memory = new WebAssembly.Memory({ initial: 1 });
  return {
    memory,
    view: new DataView(memory.buffer),
    run: new WebAssembly.Instance(module, { env: { memory } }).exports.run,
  };
});
function reset(context, { ecx = 0, eax = 0, flags = 2, pc = 0x1000, cancel = 0 } = {}) {
  const bytes = new Uint8Array(context.memory.buffer);
  bytes.fill(0);
  bytes.set(Buffer.from("R3ST"), 128);
  context.view.setUint16(132, 1, true);
  context.view.setUint16(134, 1, true);
  context.view.setUint32(136, 56, true);
  [eax, ecx, 0x12345678, 7, 0x8000, 0x33333333, 0x44444444, 0x55555555].forEach((value, index) =>
    context.view.setUint32(144 + index * 4, value, true),
  );
  context.view.setUint32(176, pc, true);
  context.view.setUint32(180, flags, true);
  context.view.setUint32(400, cancel, true);
}
function check(context, budget, { eax, ecx, pc, flags, retired, reason, cancel = 0 }) {
  assert.equal(context.run(128, 256, budget, 400), 0);
  assert.equal(context.view.getUint32(144, true), eax);
  assert.equal(context.view.getUint32(148, true), ecx);
  assert.equal(context.view.getUint32(156, true), 7);
  assert.equal(context.view.getUint32(176, true), pc);
  assert.equal(context.view.getUint32(180, true), flags);
  assert.equal(context.view.getUint32(272, true), reason);
  assert.equal(context.view.getUint32(276, true), retired);
  assert.equal(context.view.getUint32(400, true), cancel);
}
let runs = 0;
for (const ecx of [0, 1])
  for (const df of [0, 0x400]) {
    const c = contexts[0];
    reset(c, { ecx, flags: 2 | df });
    check(c, 100, { eax: ecx ? 10 : 12, ecx, pc: 0x1800, flags: 6 | df, retired: 7, reason: 3 });
    runs++;
    reset(c, { ecx, flags: 2 | df });
    check(c, 3, {
      eax: 1,
      ecx,
      pc: ecx ? 0x1009 : 0x1020,
      flags: (ecx ? 2 : 0x46) | df,
      retired: 3,
      reason: 1,
    });
    runs++;
    check(c, 100, { eax: ecx ? 10 : 12, ecx, pc: 0x1800, flags: 6 | df, retired: 4, reason: 3 });
    runs++;
  }
const diamond = contexts[0];
reset(diamond, { eax: 10, pc: 0x1020 });
check(diamond, 100, { eax: 21, ecx: 0, pc: 0x1800, flags: 0x12, retired: 4, reason: 3 });
runs++;
reset(diamond, { eax: 9, ecx: 1, flags: 6, cancel: 1 });
check(diamond, 100, { eax: 9, ecx: 1, pc: 0x1000, flags: 6, retired: 0, reason: 2, cancel: 1 });
runs++;
const loop = contexts[1];
reset(loop, { flags: 3 });
check(loop, 100, { eax: 3, ecx: 0, pc: 0x1800, flags: 7, retired: 8, reason: 3 });
runs++;
reset(loop, { flags: 3 });
check(loop, 4, { eax: 2, ecx: 2, pc: 0x1006, flags: 3, retired: 4, reason: 1 });
runs++;
check(loop, 100, { eax: 3, ecx: 0, pc: 0x1800, flags: 7, retired: 4, reason: 3 });
runs++;
const prefix = contexts[2];
reset(prefix, { eax: 9 });
check(prefix, 100, { eax: 9, ecx: 0, pc: 0x1011, flags: 2, retired: 2, reason: 3 });
runs++;
console.log(
  `${runs} actual Wasm arithmetic, branch, loop, interior resume, budget and cancellation cases passed`,
);
