import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

const [enginePath, output, root] = process.argv.slice(2);
const {readEngine} = await import(pathToFileURL(join(root, 'engine/tests/support/wasm/engine.mjs')));
const {createArena, ARENA_SIZE} = await import(pathToFileURL(join(root, 'engine/tests/support/wasm/arena.mjs')));
const engine = readEngine(enginePath), key = [0x81342715, 0x62594837], cases = [];
assert.ok(WebAssembly.Module.exports(engine.module).some(e => e.name === 'ring3_abi_v1_begin_owned_execution_receipt' && e.kind === 'function'),
  'C1 requires the additive begin_owned_execution_receipt export');
assert.ok(WebAssembly.Module.imports(engine.module).every(e => e.module === 'ring3_observability' && e.name === 'now_ns' && e.kind === 'function'), 'only the reviewed pure clock import');
const pair = n => [Number(n & 0xffffffffn), Number(n >> 32n)], digest = bytes => createHash('sha256').update(bytes).digest('hex');
const entry = 0x1000, destination = 0x1800, data = 0x4000, stack = 0x8000;
const operations = [
  [0x1000, 'call', 'e8fb010000'], [0x1005, 'dec', '49'], [0x1006, 'jnz', '0f85f4ffffff'], [0x100c, 'exit', 'ffe2'],
  [0x1200, 'fld', 'd903'], [0x1202, 'fmul', 'd84b04'], [0x1205, 'fadd', 'd84308'],
  [0x1208, 'fstp', 'd91f'], [0x120a, 'inc', '46'], [0x120b, 'ret', 'c3'],
].map(([pc, kind, hex]) => ({pc, kind, bytes: Buffer.from(hex, 'hex')}));
const code = Buffer.alloc(4096), input = Buffer.alloc(4096);
for (const op of operations) op.bytes.copy(code, op.pc - entry);
input.writeFloatLE(2, 0); input.writeFloatLE(0.5, 4); input.writeFloatLE(1, 8);
const ok = (e, name, ...args) => assert.equal(e.api[name](...args), 0, name);
const take = (e, size) => Buffer.from(new Uint8Array(e.memory.buffer, e.base + 140, size));
const arena = e => Buffer.from(new Uint8Array(e.memory.buffer, e.base, ARENA_SIZE));
const request = (e, bytes) => new Uint8Array(e.memory.buffer, e.base + 140, bytes.length).set(bytes);
const get = (e, offset) => new DataView(e.memory.buffer).getUint32(e.base + offset, true);
function header(bytes, magic, size) {
  assert.equal(bytes.subarray(0, 4).toString(), magic); assert.equal(bytes.readUInt32LE(4), 0x10001);
  assert.equal(bytes.readUInt32LE(8), size); assert.equal(bytes.readUInt32LE(12), 0);
}
function install(e, specs, slot) {
  const descriptors = Buffer.alloc(specs.length * 8);
  specs.forEach(([pc, length], i) => { descriptors.writeUInt32LE(pc, i * 8); descriptors.writeUInt32LE(length, i * 8 + 4); });
  request(e, descriptors); ok(e, 'compile_resident', specs.length);
  const descriptor = take(e, 24), id = descriptor.readBigUInt64LE(8), at = descriptor.readUInt32LE(16), size = descriptor.readUInt32LE(20);
  const bytes = new Uint8Array(e.memory.buffer).slice(at, at + size), module = new WebAssembly.Module(bytes), ring3 = {};
  for (const item of WebAssembly.Module.imports(module)) {
    if (item.module === 'ring3') {
      assert.equal(item.kind, 'function'); assert.equal(typeof e.api[item.name], 'function');
      ring3[item.name] = e.api[item.name];
      assert.equal(ring3[item.name], e.native[`ring3_abi_v1_${item.name}`], 'direct native generated import');
    } else assert.deepEqual(item, {module: 'env', name: 'memory', kind: 'memory'});
  }
  const run = new WebAssembly.Instance(module, {env: {memory: e.memory}, ring3}).exports.run;
  e.table.set(slot, run); ok(e, 'acknowledge_resident_installation', ...key, ...pair(id), slot);
  return {id, slot, run, sha256: digest(bytes)};
}
function fresh(owned) {
  const native = new WebAssembly.Instance(engine.module, {ring3_observability: {now_ns: () => 0n}}).exports;
  const api = Object.freeze(Object.fromEntries(Object.entries(native).filter(([name]) => name.startsWith('ring3_abi_v1_')).map(([name, value]) => [name.slice(13), value])));
  const e = {native, api, owned, memory: native.memory, table: new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8})};
  assert.equal(api.begin_owned_execution_receipt.length, 6); assert.equal(api.begin_execution_receipt.length, 6);
  ok(e, 'open', 16, ...key); e.base = api.arena_ptr() >>> 0; assert.equal(api.arena_len(), ARENA_SIZE);
  for (const [pc, permissions] of [[entry, 7], [data, 3], [stack, 3]]) ok(e, 'map', pc, 1, permissions);
  request(e, code); ok(e, 'upload', entry, code.length);
  e.units = [install(e, [[entry, 5], [0x1005, 7], [0x100c, 2]], 0), install(e, [[0x1200, 12]], 1)];
  ok(e, 'dispatcher_module', ...key);
  const d = take(e, 32), at = d.readUInt32LE(24), size = d.readUInt32LE(28);
  const bytes = new Uint8Array(e.memory.buffer).slice(at, at + size); e.dispatchSha = digest(bytes);
  e.run = new WebAssembly.Instance(new WebAssembly.Module(bytes), {env: {memory: e.memory, table: e.table},
    ring3: {guard_dispatch_entry: api.guard_dispatch_entry, find_installed_resident: api.find_installed_resident}}).exports.run;
  e.seed = arena(e); return e;
}
function reset(e, count, cw, {cancel = false, fault = false, ds = 0} = {}) {
  request(e, input); ok(e, 'upload', data, input.length);
  request(e, Buffer.alloc(4096)); ok(e, 'upload', stack, 4096);
  new Uint8Array(e.memory.buffer, e.base, ARENA_SIZE).set(e.seed);
  const v = new DataView(e.memory.buffer, e.base, ARENA_SIZE); v.setUint16(4, 3, true);
  [0x55555555, count, destination, data, 0x8800, 0, 0, fault ? 0x6000 : data + 0xa0].forEach((value, i) => v.setUint32(16 + i * 4, value, true));
  v.setUint32(48, entry, true); v.setUint32(52, 2, true); v.setUint32(96, Number(cancel), true); v.setUint16(4252, cw, true);
  if (ds) ok(e, 'set_segment_base', 3, ds);
  e.configured = arena(e);
}
function execute(e, budget, adversarial = false) {
  const begin = e.owned ? e.api.begin_owned_execution_receipt : e.api.begin_execution_receipt;
  if (adversarial) {
    const before = arena(e); assert.equal(begin(key[0] ^ 1, key[1], budget, ...pair(e.units[0].id), 1), 3); assert.deepEqual(arena(e), before);
  }
  assert.equal(begin(...key, budget, ...pair(e.units[0].id), 1), 0);
  const frame = take(e, 32); header(frame, 'R3EF', 32); const token = frame.readBigUInt64LE(16); assert.ok(token > 0n);
  assert.equal(e.run(e.base, e.base + 56, budget, e.base + 96), 0, 'actual engine-generated dispatcher');
  const reason = get(e, 72), retired = get(e, 76);
  if (adversarial) {
    const before = arena(e); assert.equal(e.api.finish_execution_receipt(...key, ...pair(token + 1n)), 14); assert.deepEqual(arena(e), before);
    if (e.owned) for (const [name, args] of [['map', [0xa000, 1, 3]], ['protect', [data, 1, 1]], ['unmap', [data, 1]], ['set_segment_base', [3, 0x1000]]]) {
      assert.equal(e.api[name](...args), 12, `${name} rejects active owned mutation`); assert.deepEqual(arena(e), before);
    }
  }
  ok(e, 'finish_execution_receipt', ...key, ...pair(token));
  const receipt = take(e, 64); header(receipt, 'R3ER', 64);
  assert.deepEqual(receipt.subarray(52), Buffer.alloc(12));
  assert.equal(receipt.readUInt32LE(36), retired); assert.equal(receipt.readUInt32LE(44), reason);
  assert.ok(receipt.readUInt32LE(40) <= retired && retired <= budget);
  if (retired) assert.ok(e.units.some(unit => unit.id === receipt.readBigUInt64LE(24) && unit.slot === receipt.readUInt32LE(32)));
  const finished = arena(e), ram = [entry, data, stack].map(pc => {
    const bytes = Buffer.alloc(4096);
    for (let offset = 0; offset < 4096; offset += 4) { ok(e, 'read32', pc + offset); assert.equal(get(e, 116), 0); bytes.writeUInt32LE(get(e, 120), offset); }
    return bytes;
  });
  const afterReads = arena(e); assert.deepEqual(afterReads.subarray(0, 100), finished.subarray(0, 100)); assert.deepEqual(afterReads.subarray(140), finished.subarray(140));
  assert.deepEqual(afterReads.subarray(100, 140), createArena().subarray(100, 140), 'literal final R3MH zero result at stack address 0x8ffc');
  return {frame, receipt, arena: finished, afterReads, ram, reason, retired};
}
// this oracle uses authored instruction semantics and literal exact dyadic values, never engine output.
function oracle(count, budget, cw, {cancel = false, fault = false, ds = 0} = {}) {
  assert.ok(count === 1 || count === 3, 'small literal flag and loop domain');
  const regs = [0x55555555, count, destination, data, 0x8800, 0, 0, fault ? 0x6000 : data + 0xa0];
  const ram = input.slice(), stackBytes = Buffer.alloc(4096), fp = createArena().subarray(4236, 4364);
  let pc = entry, retired = 0, flags = 2, top = 0, value = 0, fip = 0, fdp = 0, fop = 0, faulted = false;
  const arithmeticFlags = (value, before) => 2 | ([...value.toString(2)].filter(b => b === '1').length % 2 === 0 ? 4 : 0) | ((before ^ value ^ 1) & 16) | (value === 0 ? 64 : 0);
  while (!cancel && pc !== destination && retired < budget) {
    const op = operations.find(item => item.pc === pc); assert.ok(op, 'literal authored frontier'); let next = pc + op.bytes.length;
    if ((['fld', 'fmul', 'fadd'].includes(op.kind) && ds) || (op.kind === 'fstp' && fault)) { faulted = true; break; }
    if (op.kind === 'call') { regs[4] -= 4; stackBytes.writeUInt32LE(0x1005, regs[4] - stack); next = 0x1200; }
    else if (op.kind === 'ret') { next = stackBytes.readUInt32LE(regs[4] - stack); regs[4] += 4; }
    else if (op.kind === 'dec') { const before = regs[1]; flags = arithmeticFlags(--regs[1], before); }
    else if (op.kind === 'inc') { const before = regs[6]; flags = arithmeticFlags(++regs[6], before); }
    else if (op.kind === 'jnz') { if (regs[1]) next = entry; }
    else if (op.kind === 'exit') next = destination;
    else {
      fip = pc; fop = (op.bytes[0] & 7) << 8 | op.bytes[1];
      fdp = op.kind === 'fstp' ? regs[7] : data + ({fld: 0, fmul: 4, fadd: 8}[op.kind]);
      if (op.kind === 'fld') { top = 7; value = 2; }
      else if (op.kind === 'fmul') value *= 0.5;
      else if (op.kind === 'fadd') value += 1;
      else { ram.writeFloatLE(value, regs[7] - data); top = 0; }
    }
    pc = next; retired++;
  }
  fp.writeUInt16LE(cw, 16); fp.writeUInt16LE(top << 11, 18); fp.writeUInt16LE(top ? 0x3fff : 0xffff, 20);
  fp.writeUInt16LE(fop, 22); fp.writeUInt32LE(fip, 24); fp.writeUInt32LE(fdp, 28);
  if (fip) { fp.writeUInt16LE(0x1b, 32); fp.writeUInt16LE(0x23, 34); }
  if (value) { assert.ok(value === 1 || value === 2); fp.writeBigUInt64LE(0x8000000000000000n, 110); fp.writeUInt16LE(value === 1 ? 0x3fff : 0x4000, 118); }
  const state = Buffer.alloc(56); state.write('R3ST'); state.writeUInt32LE(0x10003, 4); state.writeUInt32LE(56, 8);
  regs.forEach((value, i) => state.writeUInt32LE(value, 16 + 4 * i)); state.writeUInt32LE(pc, 48); state.writeUInt32LE(flags, 52);
  return {state, fp, ram: [code, ram, stackBytes], retired, reason: cancel ? 2 : faulted ? 23 : retired >= budget ? 1 : 3, faulted};
}
function checkLiteral(e, result, count, budget, cw, options) {
  const expected = oracle(count, budget, cw, options);
  assert.deepEqual(result.arena.subarray(0, 56), expected.state, 'literal complete State32, flags and PC');
  assert.deepEqual(result.arena.subarray(4236, 4364), expected.fp, 'literal complete FP80 record, stale popped register, selectors and metadata');
  assert.deepEqual(result.ram, expected.ram, 'all code/operand/output/stack bytes against authored oracle');
  assert.deepEqual([result.retired, result.reason], [expected.retired, expected.reason], 'outer retirement and stop, independent of restored child seal');
  assert.deepEqual(result.arena.subarray(4364, 4524), e.seed.subarray(4364, 4524)); assert.deepEqual(result.arena.subarray(4612, 4756), e.seed.subarray(4612, 4756));
  assert.deepEqual(result.arena.subarray(4524, 4588), e.configured.subarray(4524, 4588)); assert.deepEqual(result.arena.subarray(4756), e.configured.subarray(4756));
  if (expected.faulted) assert.deepEqual([80, 84, 88, 92].map(offset => result.arena.readUInt32LE(offset)), [options.ds ? 4 : 6, options.ds ? 0x5000 : 0x6000, options.ds ? 1 : 2, 4]);
}
for (const cw of [0x003f, 0x023f]) for (const spec of [
  ...Array.from({length: 12}, (_, budget) => ({label: `cut-${budget}`, count: 1, budget})),
  ...[27, 28, 29].map(budget => ({label: `three-iterations-${budget}`, count: 3, budget})),
  {label: 'cancel', count: 1, budget: 50, cancel: true}, {label: 'store-fault', count: 1, budget: 50, fault: true},
  {label: 'wrong-key-token-and-owned-public-mutations', count: 1, budget: 2, adversarial: true},
  {label: 'second-activation-DS-freshness', count: 1, budget: 50, ds: 0x1000},
]) {
  const contexts = [], results = [];
  try {
    for (const owned of [false, true]) contexts.push(fresh(owned));
    assert.deepEqual(contexts[0].units.map(unit => unit.sha256), contexts[1].units.map(unit => unit.sha256)); assert.equal(contexts[0].dispatchSha, contexts[1].dispatchSha);
    for (const e of contexts) {
      if (spec.ds) { reset(e, 1, cw); const first = execute(e, 50); checkLiteral(e, first, 1, 50, cw, {}); }
      reset(e, spec.count, cw, spec); const result = execute(e, spec.budget, spec.adversarial); checkLiteral(e, result, spec.count, spec.budget, cw, spec); results.push(result);
    }
    assert.deepEqual(results[0], results[1], 'entire arena before/after diagnostic reads, raw frame/receipt and all mapped RAM: no normalization');
    cases.push({label: spec.label, cw, budget: spec.budget, retired: results[0].retired, reason: results[0].reason,
      receipt: results[0].receipt.toString('hex'), arenaSha256: digest(results[0].arena), ramSha256: results[0].ram.map(digest)});
  } finally { for (const e of contexts) ok(e, 'close'); }
}
const result = {status: 'PASS', engineSha256: engine.sha256, cases,
  coverage: 'raw native exports and engine-generated two-owner CALL/FP/store/RET dispatcher; exact positive dyadics under PC24/PC53 only, every one-iteration budget cut, finite multi-iteration cuts, cancellation, store/read faults, key/token neutrality and post-finish operand context freshness. Full arena/raw receipt/RAM parity is separate from the literal instruction oracle. No arbitrary rounding, nonfinite/negative FP, Windows/callback, browser responsiveness or performance claim.'};
writeFileSync(join(output, 'result.json'), JSON.stringify(result, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({status: result.status, engineSha256: engine.sha256, cases: cases.length}));
