import assert from 'node:assert/strict';

let input = '';
for await (const chunk of process.stdin) input += chunk;
const fixture = JSON.parse(input);
const initial = Buffer.from(fixture.arena, 'hex');
const PC = 0x1000;
const registers = [0x12345678, 0x11223344, 0x23456789, 0x3456789a, 0x456789ab, 0x56789abc, 0x6789abcd, 0x789abcde];
let controls = 0;

function expected(item, before, {entry = 0, budget = 1, flags = 0x244cd7, shadow = 0, cancel = 0, value = 0x80000000, limit = 0xfffff} = {}) {
  const want = Buffer.from(before);
  let pc = PC + entry, retired = 0, reason, nextFlags = flags, nextShadow = shadow;
  if (cancel) reason = 2;
  else if (budget === 0) reason = 1;
  else if (entry !== 0) reason = item.bound && pc > limit ? 11 : 3;
  else if (item.bound && PC + 1 > limit) reason = 11;
  else {
    if (item.name !== 'cmp') want.writeUInt32LE(0, 16 + item.register * 4);
    else assert.equal(want.readUInt32LE(16 + item.register * 4), value);
    nextFlags = ((flags & ~0x108d7) | 0x46) >>> 0;
    nextShadow = 0;
    pc += 2;
    retired = 1;
    if (flags & 0x100) reason = 16;
    else if (budget === 1) reason = 1;
    else reason = item.bound && pc > limit ? 11 : 3;
  }
  want.writeUInt32LE(nextShadow, 12);
  want.writeUInt32LE(pc, 48);
  want.writeUInt32LE(nextFlags, 52);
  const exit = Buffer.alloc(40);
  exit.write('R3EX');
  const exitVersion = reason === 16 ? 7 : reason === 11 ? 6 : 1;
  exit.writeUInt32LE(0x10000 | exitVersion, 4);
  exit.writeUInt32LE(40, 8);
  exit.writeUInt32LE(reason, 16);
  exit.writeUInt32LE(retired, 20);
  want.set(exit, 56);
  return {want, reason};
}

for (const [ordinal, item] of fixture.modules.entries()) {
  const module = new WebAssembly.Module(Buffer.from(item.bytes, 'hex'));
  const memory = new WebAssembly.Memory({initial: 1, maximum: 1});
  const data = new DataView(memory.buffer), bytes = new Uint8Array(memory.buffer);
  const set = (offset, value) => data.setUint32(offset, value >>> 0, true);
  let guards, finishes;
  const bindings = {};
  for (const imported of WebAssembly.Module.imports(module)) {
    if (imported.module === 'env' && imported.name === 'memory') continue;
    assert.equal(imported.module, 'ring3');
    if (imported.name === 'guard_detect' || imported.name === 'guard_detect_resident') {
      bindings[imported.name] = () => { guards++; return 1n; };
    } else if (imported.name === 'finish_exception_detection') {
      bindings[imported.name] = (...args) => { finishes.push(args); return 0; };
    } else throw new Error(`unexpected helper ${imported.name}`);
  }
  const {run} = new WebAssembly.Instance(module, {env: {memory}, ring3: bindings}).exports;
  function check(label, options = {}) {
    const {entry = 0, budget = 1, flags = 0x244cd7, shadow = 0, cancel = 0, value = 0x80000000} = options;
    bytes.fill(0x5a);
    bytes.set(initial);
    set(4, 0x10003);
    set(12, shadow);
    registers.forEach((value, index) => set(16 + index * 4, value));
    set(16 + item.register * 4, value);
    set(48, PC + entry);
    set(52, flags);
    set(96, cancel);
    if (options.limit !== undefined) {
      data.setUint16(fixture.segments + 4, 3, true);
      set(fixture.segments + 32, (data.getUint32(fixture.segments + 32, true) & ~0x010fffff) | options.limit);
    }
    const before = Buffer.from(bytes), oracle = expected(item, before, options);
    guards = 0; finishes = [];
    assert.equal(run(0, 56, budget, 96), 0, `${label}: transport`);
    assert.deepEqual(Buffer.from(bytes), oracle.want, `${label}: complete CPU, exit, FP, segment, scratch and RAM state`);
    assert.equal(guards, Number(item.bound), `${label}: unchanged entry guard`);
    const seals = item.bound && (oracle.reason === 3 || oracle.reason >= 10);
    assert.equal(finishes.length, Number(seals), `${label}: unchanged detection finish boundary`);
    if (seals) assert.equal(finishes[0][1], oracle.reason, `${label}: exact sealed reason`);
    controls++;
  }
  const label = `${ordinal}/${item.name}/r${item.register}`;
  for (const value of [0, 0x80000000, 0xffffffff]) check(`${label}/value${value}`, {value});
  for (const flags of [2, 0xcd7, 0x244cd7, 0x244102, 0x10002, 0x10102]) {
    check(`${label}/flags${flags}`, {flags});
  }
  for (const options of [
    {budget: 0, flags: 0x10102, shadow: 1},
    {cancel: 1, budget: 0, flags: 0x10102, shadow: 1},
    {budget: 2},
    {shadow: 1},
    {flags: 0x244102, shadow: 1, budget: 2},
    {entry: 1, flags: 0x10002, shadow: 1},
    {entry: 2, flags: 0x10002, shadow: 1},
  ]) check(`${label}/${JSON.stringify(options)}`, options);
  if (item.bound) {
    check(`${label}/instruction span fault before RF clear`, {limit: PC, flags: 0x10102, shadow: 1});
    check(`${label}/next PC fault retains completed flags`, {limit: PC + 1, budget: 2});
    check(`${label}/budget precedes next PC fault`, {limit: PC + 1, budget: 1});
  }
}
assert.equal(fixture.modules.length, 144, 'all eight registers, three operations, two tiers and three bindings');
console.log(`identical dword binary controls passed (${controls} cases)`);
