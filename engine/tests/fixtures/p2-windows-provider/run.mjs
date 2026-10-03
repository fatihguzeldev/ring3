import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [enginePath, outputDir, root] = process.argv.slice(2);
assert.ok(root, 'expected actual engine, output directory and repository root');
const fixtureRoot = join(root, 'engine/tests/fixtures/p2-windows-provider');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const oracleBytes = readFileSync(join(fixtureRoot, 'oracle.json')), oracle = JSON.parse(oracleBytes);
const size = 4236, transfer = 140, budget = 64;
const artifacts = {}, observations = [], identities = [];

function elfSymbols(object) {
  const view = new DataView(object.buffer, object.byteOffset, object.byteLength);
  assert.deepEqual([...object.subarray(0, 7)], [0x7f, 0x45, 0x4c, 0x46, 1, 1, 1]);
  assert.equal(view.getUint16(18, true), 3, 'LLVM i386 object');
  const offset = view.getUint32(32, true), stride = view.getUint16(46, true);
  const sections = Array.from({length: view.getUint16(48, true)}, (_, index) => {
    const at = offset + index * stride;
    return {type: view.getUint32(at + 4, true), offset: view.getUint32(at + 16, true), size: view.getUint32(at + 20, true), link: view.getUint32(at + 24, true), info: view.getUint32(at + 28, true), stride: view.getUint32(at + 36, true)};
  });
  const table = sections.find(section => section.type === 2);
  assert.ok(table, 'authored ELF symbols');
  const strings = sections[table.link], symbols = new Map();
  for (let at = table.offset; at < table.offset + table.size; at += table.stride) {
    const nameAt = strings.offset + view.getUint32(at, true);
    const name = object.subarray(nameAt, object.indexOf(0, nameAt)).toString('utf8');
    const section = view.getUint16(at + 14, true), start = view.getUint32(at + 4, true), length = view.getUint32(at + 8, true);
    if (section > 0 && section < sections.length && length > 0) {
      const source = sections[section];
      assert.ok(start + length <= source.size);
      assert.ok(!sections.some(item => [4, 9].includes(item.type) && item.info === section && item.size > 0), 'authored local CALLs need no ELF relocations');
      symbols.set(name, {offset: start, bytes: object.subarray(source.offset + start, source.offset + start + length)});
    }
  }
  return symbols;
}

const assembly = join(fixtureRoot, 'program.S'), objectPath = join(outputDir, 'program.o');
const assemble = ['-target', 'i386-unknown-linux-gnu', '-c', assembly, '-o', objectPath];
execFileSync('clang', assemble, {stdio: ['ignore', 'pipe', 'pipe']});
writeFileSync(join(outputDir, 'program.disassembly.txt'), execFileSync('xcrun', ['llvm-objdump', '-d', '--x86-asm-syntax=intel', objectPath]));
const symbols = elfSymbols(readFileSync(objectPath));
assert.equal(symbols.size, 3);
for (const expected of oracle.symbols) {
  const symbol = symbols.get(expected.name);
  assert.equal(symbol.offset, expected.offset);
  assert.equal(symbol.bytes.toString('hex'), expected.hex ?? oracle.code_hex, `${expected.name}: frozen independent encoding`);
  if (expected.size !== undefined) assert.equal(symbol.bytes.length, expected.size);
  writeFileSync(join(outputDir, `${expected.name}.x86`), symbol.bytes);
  artifacts[`${expected.name}.x86`] = hash(symbol.bytes);
}
writeFileSync(join(outputDir, 'authored-oracle.json'), oracleBytes);
writeFileSync(join(outputDir, 'program.S'), readFileSync(assembly));
writeFileSync(join(outputDir, 'run.mjs'), readFileSync(import.meta.filename));
const productionPaths = execFileSync('rg', ['--files', 'engine/src'], {cwd: root, encoding: 'utf8'}).trim().split('\n').sort();
const sourcePaths = [...productionPaths, 'Cargo.toml', 'engine/Cargo.toml', 'Cargo.lock', 'engine/tests/process_windows_provider_wasm.rs', 'engine/tests/fixtures/p2-windows-provider/program.S', 'engine/tests/fixtures/p2-windows-provider/oracle.json', 'engine/tests/fixtures/p2-windows-provider/run.mjs'];
const sources = Object.fromEntries(sourcePaths.map(file => [file, hash(readFileSync(join(root, file)))]));
writeFileSync(join(outputDir, 'source-sha256.json'), JSON.stringify(sources, null, 2));
const engineBytes = readFileSync(enginePath);
writeFileSync(join(outputDir, 'engine.wasm'), engineBytes);
assert.ok(WebAssembly.validate(engineBytes));
const engineModule = new WebAssembly.Module(engineBytes);
assert.deepEqual(WebAssembly.Module.imports(engineModule), [], 'engine-owned provider needs no JS imports');
const names = ['open', 'close', 'arena_ptr', 'map', 'protect', 'upload', 'compile_with_gates', 'compile_resident_with_gates', 'generation', 'module_ptr', 'module_len', 'guard', 'guard_resident', 'read32', 'write32', 'store32', 'store_resident32', 'capture_call', 'capture_resident_call', 'complete_call', 'complete_resident_call', 'complete_windows_call', 'complete_resident_windows_call', 'begin_callback', 'abort_callback'];
let nextInstance = 0;

function refresh(engine) {
  if (engine.buffer !== engine.memory.buffer) {
    engine.buffer = engine.memory.buffer;
    engine.bytes = new Uint8Array(engine.buffer); engine.view = new DataView(engine.buffer);
  }
  return engine;
}
function arena(engine) { return refresh(engine).bytes.slice(engine.base, engine.base + size); }
function record(magic, length, fields, version = 1) {
  const bytes = new Uint8Array(length), view = new DataView(bytes.buffer);
  bytes.set(Buffer.from(magic)); view.setUint16(4, version, true); view.setUint16(6, 1, true); view.setUint32(8, length, true);
  fields.forEach((value, index) => view.setUint32(16 + index * 4, value, true));
  return bytes;
}
function words(fields) {
  const bytes = new Uint8Array(fields.length * 4), view = new DataView(bytes.buffer);
  fields.forEach((value, index) => view.setUint32(index * 4, value, true));
  return bytes;
}
function cpu(registers, eip) { return record('R3ST', 56, [...registers, eip, oracle.eflags]); }
function exit(reason, retired, detail = 0, version = 3) { return record('R3EX', 40, [reason, retired, detail, 0, 0, 0], version); }
function helper(value = 0) { return record('R3MH', 40, [0, value, 0, 0, 0, 0]); }
function operation(engine, label, action, status = 0, patches = []) {
  const expected = arena(engine);
  for (const [offset, bytes] of patches) expected.set(bytes, offset);
  assert.equal(action(), status, `${label}: status`);
  assert.deepEqual(arena(engine), expected, `${label}: full4236 State/Exit/cancel/helper/transfer arena`);
}
function fresh(owner) {
  const instance = new WebAssembly.Instance(engineModule, {});
  const api = Object.fromEntries(names.map(name => {
    const fn = instance.exports[`ring3_abi_v1_${name}`];
    assert.equal(typeof fn, 'function', name); return [name, fn];
  }));
  assert.equal(api.complete_windows_call.length, 4); assert.equal(api.complete_resident_windows_call.length, 5);
  const ordinal = ++nextInstance, key = 0xa3400000c0000000n + BigInt(ordinal);
  const engine = {api, ordinal, owner, key, low: Number(key & 0xffffffffn), high: Number(key >> 32n), memory: instance.exports.memory};
  assert.ok(engine.memory instanceof WebAssembly.Memory);
  assert.equal(api.open(8, engine.low, engine.high), 0);
  engine.base = api.arena_ptr() >>> 0; refresh(engine);
  assert.ok(engine.base > 0 && engine.base + size <= engine.bytes.length);
  engine.bytes.fill(0xa5, engine.base, engine.base + size);
  return engine;
}
function read(engine, address, value, label) { operation(engine, label, () => engine.api.read32(address), 0, [[100, helper(value)]]); }
function stack(engine, values, label) { oracle.stack_addresses.forEach((address, index) => read(engine, address, values[index], `${label}: stack${index}`)); }
function descriptors(engine, blocks, gates) {
  const view = refresh(engine).view;
  [...blocks, ...gates].forEach(([pc, value], index) => {
    view.setUint32(engine.base + transfer + index * 8, pc, true);
    view.setUint32(engine.base + transfer + index * 8 + 4, value, true);
  });
}
function child(engine, label, blocks, gates, residentId) {
  descriptors(engine, blocks, gates);
  let bytes, low, high;
  if (residentId !== undefined) {
    const before = arena(engine);
    assert.equal(engine.api.compile_resident_with_gates(blocks.length, gates.length), 0);
    const after = arena(engine), metadata = new DataView(after.buffer, transfer, 24);
    low = metadata.getUint32(8, true); high = metadata.getUint32(12, true);
    assert.equal(BigInt(low) | (BigInt(high) << 32n), BigInt(residentId));
    const pointer = metadata.getUint32(16, true), length = metadata.getUint32(20, true);
    const expected = before.slice(); expected.set(words([1, 24, low, high, pointer, length]), transfer);
    assert.deepEqual(after, expected, `${label}: only metadata24 published`);
    refresh(engine); assert.ok(pointer > 0 && length > 8 && pointer + length <= engine.bytes.length);
    bytes = engine.bytes.slice(pointer, pointer + length);
    identities.push({instance: engine.ordinal, key: engine.key.toString(), unit: residentId, label, pointer, length});
  } else {
    operation(engine, label, () => engine.api.compile_with_gates(blocks.length, gates.length));
    assert.equal(engine.api.generation(), 1);
    const pointer = engine.api.module_ptr() >>> 0, length = engine.api.module_len() >>> 0;
    refresh(engine); assert.ok(pointer > 0 && length > 8 && pointer + length <= engine.bytes.length);
    bytes = engine.bytes.slice(pointer, pointer + length);
    identities.push({instance: engine.ordinal, key: engine.key.toString(), generation: 1, label, pointer, length});
  }
  assert.ok(WebAssembly.validate(bytes));
  const module = new WebAssembly.Module(bytes), guard = residentId === undefined ? 'guard' : 'guard_resident';
  const stores = blocks.some(([pc]) => pc < 0x2000) ? [residentId === undefined ? 'store32' : 'store_resident32'] : [];
  const sort = imports => imports.sort((a, b) => `${a.module}.${a.name}`.localeCompare(`${b.module}.${b.name}`));
  assert.deepEqual(sort(WebAssembly.Module.imports(module)), sort([{module: 'env', name: 'memory', kind: 'memory'}, ...[guard, ...stores].map(name => ({module: 'ring3', name, kind: 'function'}))]), `${label}: only actual guard/store imports`);
  const run = new WebAssembly.Instance(module, {env: {memory: engine.memory}, ring3: Object.fromEntries([guard, ...stores].map(name => [name, engine.api[name]]))}).exports.run;
  assert.equal(typeof run, 'function'); assert.equal(run.length, 4);
  const filename = `engine-${engine.ordinal}-${label}.wasm`;
  writeFileSync(join(outputDir, filename), bytes); artifacts[filename] = hash(bytes);
  return {run, low, high};
}
function setup(owner, getId = 0x10001) {
  const engine = fresh(owner);
  operation(engine, 'writable code staging', () => engine.api.map(0x1000, 2, 7));
  operation(engine, 'explicit caller stack mapping', () => engine.api.map(0x8000, 2, 3));
  for (const expected of oracle.symbols) {
    const symbol = symbols.get(expected.name);
    refresh(engine).bytes.set(symbol.bytes, engine.base + transfer);
    operation(engine, `upload authored ${expected.name}`, () => engine.api.upload(0x1000 + symbol.offset, symbol.bytes.length));
  }
  operation(engine, 'final code RX permissions', () => engine.api.protect(0x1000, 2, 5));
  oracle.stack_addresses.forEach((address, index) => operation(engine, 'startup stack sentinel', () => engine.api.write32(address, oracle.initial_stack_values[index]), 0, [[100, helper()]]));
  const gates = [[0x2000, getId], [0x2100, 0x10002]], gateBlocks = gates.map(([pc]) => [pc, 2]);
  let caller, gateOwner;
  if (owner === 'resident') {
    gateOwner = child(engine, 'gate-B', gateBlocks, gates, 1);
    caller = child(engine, 'caller-A', oracle.caller_blocks, [], 2);
    assert.equal(engine.api.generation(), 0);
  } else {
    caller = child(engine, 'replacement', [...oracle.caller_blocks, ...gateBlocks], gates);
    gateOwner = caller;
  }
  // the only host cpu initialization; even negative controls use real calls.
  refresh(engine).bytes.set(cpu(oracle.startup_registers, oracle.entry), engine.base);
  engine.bytes.fill(0, engine.base + 96, engine.base + 100);
  return {engine, caller, gateOwner};
}
function dispatch(ctx, stage, extra = '') {
  const {engine, caller, gateOwner} = ctx, resident = engine.owner === 'resident';
  operation(engine, `${engine.owner} ${extra} caller dispatch`, () => caller.run(engine.base, engine.base + 56, budget, engine.base + 96), 0, [
    [0, cpu(stage.stop_registers, stage.gate_pc)],
    [56, exit(resident ? 3 : 8, stage.retired, resident ? 0 : stage.id, resident ? 2 : 3)],
    [100, helper()],
  ]);
  if (resident) operation(engine, 'actual Gate B dispatch retires0', () => gateOwner.run(engine.base, engine.base + 56, budget, engine.base + 96), 0, [[56, exit(8, 0, stage.id)]]);
  stack(engine, stage.stack_values, `${engine.owner} ${extra} real PUSH/CALL words`);
}
function callRecord(stage, token, tag = 2) {
  return record('R3CF', 112, [token, stage.id, tag, stage.args.length, stage.gate_pc, stage.stop_registers[4], stage.return_pc, tag === 3 ? stage.stop_registers[1] : 0, ...Array.from({length: 16}, (_, index) => stage.args[index] ?? 0)]);
}
function capture(ctx, stage, token, tag = 2) {
  const {engine, gateOwner} = ctx;
  const action = engine.owner === 'resident'
    ? () => engine.api.capture_resident_call(engine.low, engine.high, gateOwner.low, gateOwner.high, tag, stage.args.length)
    : () => engine.api.capture_call(engine.low, engine.high, 1, tag, stage.args.length);
  operation(engine, `${stage.api}: exact private capture record`, action, 0, [[transfer, callRecord(stage, token, tag)]]);
}
function provider(ctx, token, options = {}) {
  const {engine, gateOwner} = ctx;
  const low = options.low ?? engine.low, high = options.high ?? engine.high;
  return engine.owner === 'resident'
    ? engine.api.complete_resident_windows_call(low, high, options.unitLow ?? gateOwner.low, options.unitHigh ?? gateOwner.high, token)
    : engine.api.complete_windows_call(low, high, options.generation ?? 1, token);
}
function complete(ctx, stage, token, returned = stage.returned_registers) {
  operation(ctx.engine, `${stage.api}: Rust-owned completion`, () => provider(ctx, token), 0, [[0, cpu(returned, stage.return_pc)], [56, exit(3, 0)]]);
}
function generic(ctx, stage, token, result, returned) {
  const {engine, gateOwner} = ctx;
  const action = engine.owner === 'resident'
    ? () => engine.api.complete_resident_call(engine.low, engine.high, gateOwner.low, gateOwner.high, token, result)
    : () => engine.api.complete_call(engine.low, engine.high, 1, token, result);
  operation(engine, 'negative control retains old scalar fallback', action, 0, [[0, cpu(returned, stage.return_pc)], [56, exit(3, 0)]]);
}
function close(ctx) {
  operation(ctx.engine, 'close preserves arena', () => ctx.engine.api.close());
  operation(ctx.engine, 'Closed first before key/owner/token', () => provider(ctx, 0, {low: 0, high: 0, generation: 0, unitLow: 0, unitHigh: 0}), 5);
}
function completionErrors(ctx, stage, token) {
  const {engine, caller, gateOwner} = ctx;
  operation(engine, 'wrong full high key', () => provider(ctx, token, {high: engine.high ^ 1}), 3);
  operation(engine, 'zero token', () => provider(ctx, 0), 14);
  operation(engine, 'wrong token', () => provider(ctx, token + 1), 14);
  if (engine.owner === 'resident') {
    operation(engine, 'nonzero unit high limb is significant', () => provider(ctx, token, {unitHigh: 1}), 3);
    operation(engine, 'current caller A is not captured Gate B owner', () => provider(ctx, token, {unitLow: caller.low, unitHigh: caller.high}), 14);
    operation(engine, 'replacement identity cannot claim resident pending', () => engine.api.complete_windows_call(engine.low, engine.high, 1, token), 3);
  } else {
    operation(engine, 'wrong generation', () => provider(ctx, token, {generation: 2}), 3);
    operation(engine, 'absent resident cannot claim replacement pending', () => engine.api.complete_resident_windows_call(engine.low, engine.high, 1, 0, token), 3);
  }
  operation(engine, 'pending caller stays parked', () => caller.run(engine.base, engine.base + 56, budget, engine.base + 96), 12);
  if (gateOwner !== caller) operation(engine, 'pending Gate B stays parked', () => gateOwner.run(engine.base, engine.base + 56, budget, engine.base + 96), 12);
  // corrupt only exit, then restore it; no poststartup host cpu writes.
  refresh(engine).bytes[engine.base + 68] = 1;
  engine.view.setUint32(engine.base + 96, 1, true);
  operation(engine, 'saved Exit equality precedes cancellation', () => provider(ctx, token), 15);
  engine.bytes[engine.base + 68] = 0;
  operation(engine, 'cancelled Set retains pending token for retry', () => provider(ctx, token), 16);
  engine.view.setUint32(engine.base + 96, 0, true);
  stack(engine, stage.stack_values, 'all rejected completion controls retain guest RAM');
  engine.bytes.fill(0xc3, engine.base + transfer, engine.base + transfer + 112);
  // the following successful set must use the privately captured full-u32 arg.
}

let isolationChecked = false;
for (const owner of ['replacement', 'resident']) {
  const ctx = setup(owner), {engine, caller} = ctx;
  let retired = 0;
  for (let index = 0; index < oracle.stages.length; index++) {
    const stage = oracle.stages[index], token = index + 1;
    dispatch(ctx, stage, stage.api); retired += stage.retired;
    capture(ctx, stage, token);
    if (index === 1) completionErrors(ctx, stage, token);
    complete(ctx, stage, token);
    stack(engine, stage.stack_values, `${owner}: provider completion writes no guest RAM`);
    operation(engine, 'consumed provider token cannot replay effect', () => provider(ctx, token), 14);
    observations.push({owner, api: stage.api, token, retired: stage.retired, windows_value: stage.last_error_after, state_after: {registers: stage.returned_registers, eip: stage.return_pc, eflags: oracle.eflags}});
    if (index === 1 && !isolationChecked) {
      const other = setup('resident'), first = oracle.stages[0];
      dispatch(other, first, 'independent instance Get'); capture(other, first, 1); complete(other, first, 1);
      close(other); isolationChecked = true;
      // the main context's following get must still return its private magic.
    }
  }
  const final = oracle.final;
  operation(engine, `${owner}: actual final caller LEA/JMP`, () => caller.run(engine.base, engine.base + 56, budget, engine.base + 96), 0, [[0, cpu(final.registers, final.eip)], [56, exit(final.reason, final.retired, 0, owner === 'resident' ? 2 : 3)]]);
  retired += final.retired; assert.equal(retired, oracle.total_retired);
  stack(engine, final.stack_values, `${owner}: final stack words and neighbors`);
  observations.push({owner, final_registers: final.registers, final_eip: final.eip, total_retired: retired});
  close(ctx);
}

// new provider rejection leaves the old generic completion available.
for (const [label, gateId, tag] of [['unknown gate', 0xabcdef01, 2], ['wrong convention', 0x10001, 1]]) {
  const ctx = setup('replacement', gateId), first = {...oracle.stages[0], id: gateId};
  dispatch(ctx, first, label); capture(ctx, first, 1, tag);
  operation(ctx.engine, label, () => provider(ctx, 1), 7);
  stack(ctx.engine, first.stack_values, `${label}: RAM preserved`);
  const returned = [55, 0x2468ace0, 0x89abcdef, 0x10203040, 0x9000, 0x55667788, 0x99aabbcc, 0xddeeff00];
  generic(ctx, first, 1, 55, returned); close(ctx);
  observations.push({rejection: label, status: 7, old_generic_fallback: true});
}

// a cancelled set cannot change private windows state before a later get.
{
  const ctx = setup('replacement'), [initial, setMagic, getMagic] = oracle.stages;
  dispatch(ctx, initial); capture(ctx, initial, 1); complete(ctx, initial, 1);
  dispatch(ctx, setMagic); capture(ctx, setMagic, 2);
  refresh(ctx.engine).view.setUint32(ctx.engine.base + 96, 1, true);
  operation(ctx.engine, 'cancelled Set effect is unpublished', () => provider(ctx, 2), 16);
  ctx.engine.view.setUint32(ctx.engine.base + 96, 0, true);
  generic(ctx, setMagic, 2, 0, setMagic.returned_registers);
  dispatch(ctx, getMagic, 'observe failed Set atomicity'); capture(ctx, getMagic, 3);
  const unchangedThread = [0, 0x2468ace0, 0x89abcdef, 0, 0x9000, 0x55667788, 0, 0xddeeff00];
  complete(ctx, getMagic, 3, unchangedThread); close(ctx);
  observations.push({cancelled_set_following_get: 0, old_generic_only_in_negative_control: true});
}

// a real nested replacement call retains the old callback lifecycle, while
// the new ordinary provider entry rejects callback-owned execution as busy.
{
  const ctx = setup('replacement'), {engine, caller} = ctx, first = oracle.stages[0];
  dispatch(ctx, first); capture(ctx, first, 1);
  const callbackRegisters = [0x13579bdf, 0x2468ace0, 0x89abcdef, 0x10203040, 0x8ff8, 0x55667788, 0x99aabbcc, 0xddeeff00];
  operation(engine, 'existing engine begins callback frame', () => engine.api.begin_callback(engine.low, engine.high, 1, 1, 0x1000, 0x2100, 0x10002, 0), 0, [[0, cpu(callbackRegisters, 0x1000)], [56, exit(3, 0)], [transfer, record('R3CB', 64, [2, 1, 1, 0, 0x1000, 0x8ff8, 0x2100, 0x10002, 0, 0, 1, 0])]]);
  operation(engine, 'suspended outer provider remains Busy', () => provider(ctx, 1), 12);
  const innerRegisters = [0x13579bdf, 0x2468ace0, 0x89abcdef, 0x10203040, 0x8ff4, 0x55667788, 0x99aabbcc, 0xddeeff00];
  operation(engine, 'real callback inner CALL reaches Get Gate', () => caller.run(engine.base, engine.base + 56, budget, engine.base + 96), 0, [[0, cpu(innerRegisters, 0x2000)], [56, exit(8, 1, 0x10001)], [100, helper()]]);
  const inner = {...first, stop_registers: innerRegisters};
  capture(ctx, inner, 3);
  operation(engine, 'matching inner owner is still outside provider scope', () => provider(ctx, 3), 12);
  stack(engine, [0x1005, 0x2100, 0x1005, 0xddeeff00], 'callback provider refusal preserves nested RAM');
  const innerReturned = [0, 0x2468ace0, 0x89abcdef, 0x10203040, 0x8ff8, 0x55667788, 0x99aabbcc, 0xddeeff00];
  generic(ctx, inner, 3, 0, innerReturned);
  operation(engine, 'old callback abort restores exact outer stop', () => engine.api.abort_callback(engine.low, engine.high, 2), 0, [[0, cpu(first.stop_registers, first.gate_pc)], [56, exit(8, 1, first.id)]]);
  complete(ctx, first, 1); close(ctx);
  observations.push({callback_outer_status: 12, callback_inner_status: 12, provider_after_abort: 'Get0'});
}

const provenance = {
  engine_sha256: hash(engineBytes), oracle_sha256: hash(oracleBytes), artifacts, sources, identities, observations,
  command: [process.execPath, process.argv[1], enginePath, outputDir, root], assembly_command: ['clang', ...assemble],
  tools: {node: process.version, v8: process.versions.v8, clang: execFileSync('clang', ['--version'], {encoding: 'utf8'}).split('\n')[0]},
  claim: 'actual engine-owned closed GetLastError/SetLastError semantics from real authored CALL/PUSH and real caller continuations, both replacement and resident caller-A/Gate-B ownership. Positive programme runs initialGet0→Setfull-u32→Getfull→Set0→Get0→callerLEA7, exactly13 emitted retirements. Literal LLVM/CPU/stack/provider/Exit oracles precede product; Gate and provider completion retire0. CPU written by host only at startup; Exit corruption/cancel/transfer tamper are deliberate negative host-input controls. Rust uses private captured metadata and owns semantics; positive paths never supply host scalar. Extra negative controls prove old scalar fallback, unpublished cancelled Set via later Get0, and active-callback Busy without adding callback providers. Independent instance Get0 does not clear main magic. Complete4236arena preservation on errors and stack checks surround captures/completions. Native-only name profile is copied in oracle, not a Wasm resolver/PE import/IAT/DLL/ordinal link claim. No other APIs, threads/TEB/TLS, async/pointee/handles/startup, SDK/browser/game/fullP2-V0 claim.',
};
writeFileSync(join(outputDir, 'provenance.json'), JSON.stringify(provenance, null, 2));
console.log(JSON.stringify({status: 'ok', engine_sha256: provenance.engine_sha256, oracle_sha256: provenance.oracle_sha256, owners: ['replacement', 'resident'], positive_retired_each: 13, output: outputDir}));
