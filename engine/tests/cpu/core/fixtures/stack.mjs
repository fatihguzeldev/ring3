import {instantiateCore,coreImports} from './link.mjs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync, openSync, closeSync} from 'node:fs';
import {readEngine} from '../../../support/wasm/engine.mjs';

const [enginePath, reportPath] = process.argv.slice(2);
assert(enginePath && reportPath, 'usage: stack.mjs engine.wasm fresh-report.json');
const engine = readEngine(enginePath), output = openSync(reportPath, 'wx');
const hash = value => createHash('sha256').update(value).digest('hex');
const hex = value => Buffer.from(value).toString('hex');
const report = {schema: 1, complete: false, engineSha256: engine.sha256,
  sources: [import.meta.url,new URL('./link.mjs',import.meta.url).href, new URL('../../../support/wasm/engine.mjs', import.meta.url).href]
    .map(url => ({url, sha256: hash(readFileSync(new URL(url)))})), cases: []};
const checkpoint = () => writeFileSync(reportPath, JSON.stringify(report, null, 2));
const seedRegisters = esp => [0x11223344, 0x55667788, 0x99aabbcc, 0xddeeff00, esp,
  0x10203040, 0x50607080, 0x90a0b0c0];
const flags = 0x893;
let stage = 'initialization';

function runCase(name, codeHex, options, expected) {
  stage = name;
  const instance = instantiateCore(engine.module, {ring3_observability: {now_ns: () => 0n}});
  const memory = instance.exports.memory;
  const api = Object.fromEntries(Object.entries(instance.exports)
    .filter(([name]) => name.startsWith('ring3_abi_v1_')).map(([name, value]) => [name.slice(13), value]));
  const view = () => new DataView(memory.buffer);
  const slice = (pointer, length) => new Uint8Array(memory.buffer).slice(pointer, pointer + length);
  const put = (pointer, value) => view().setUint32(pointer, value >>> 0, true);
  const word = pointer => view().getUint32(pointer, true);
  const result = {name, codeHex, expected, controls: []}; report.cases.push(result);
  let opened = false, active = false, arena = 0, context = 0;
  const status = (operation, wanted, ...args) => {
    const actual = api[operation](...args) >>> 0;
    result.controls.push({operation, args, wanted, actual});
    assert.equal(actual, wanted, `${name}: ${operation}`);
  };
  try {
    status('open', 0, 16, 97, 0); opened = true; arena = api.arena_ptr() >>> 0;
    const pages = new Map([[0x1000, 7], [0x3000, 3], [0x4000, 3]]);
    for (const [address, permissions] of pages) {
      status('map', 0, address, 1, permissions);
      new Uint8Array(memory.buffer, arena + 140, 4096).fill(0x5a);
      if (address === 0x1000) {
        new Uint8Array(memory.buffer, arena + 140, 4096).fill(0x90);
        new Uint8Array(memory.buffer, arena + 140, codeHex.length / 2 + 2)
          .set(Buffer.from(`${codeHex}0f0b`, 'hex'));
      }
      status('upload', 0, address, 4096);
    }
    for (const [address, width, value] of options.initial ?? []) {
      for (let index = 0; index < width; index++) new Uint8Array(memory.buffer)[arena + 140 + index] = value >>> (index * 8);
      status('upload', 0, address, width);
    }
    if (options.readOnlyData) status('protect', 0, 0x3000, 1, 1);
    const registers = options.registers ?? seedRegisters(0x4800);
    new Uint8Array(memory.buffer, arena, 56).fill(0);
    new Uint8Array(memory.buffer, arena, 4).set([82, 51, 83, 84]);
    view().setUint16(arena + 4, 3, true); view().setUint16(arena + 6, 1, true); put(arena + 8, 56);
    registers.forEach((value, index) => put(arena + 16 + index * 4, value));
    put(arena + 48, 0x1000); put(arena + 52, flags);
    const originalBanks = [[4236, 128], [4364, 160], [4612, 144]]
      .map(([offset, length]) => ({offset, bytes: slice(arena + offset, length)}));
    const assertBanks = label => {
      for (const bank of originalBanks) assert.deepEqual(slice(arena + bank.offset, bank.bytes.length), bank.bytes,
        `${name}: ${label} full numerical record at ${bank.offset}`);
    };
    status('core_prepare', 0, 0x1000, 16, 4);
    context = api.core_context_ptr() >>> 0; assert(context > 0 && context % 8 === 0);
    const moduleBytes = slice(api.core_code_ptr() >>> 0, api.core_code_len() >>> 0);
    const module = new WebAssembly.Module(moduleBytes);
    assert.deepEqual(coreImports(module), [
      {module: 'ring3', name: 'memory', kind: 'memory'}, {module: 'ring3', name: 'read', kind: 'function'},
      {module: 'ring3', name: 'write', kind: 'function'}, {module: 'ring3', name: 'numeric', kind: 'function'},
    ]);
    const run = instantiateCore(module, {ring3: {memory, read: api.core_read,
      write: api.core_write, numeric: api.core_numeric}}).exports.run;
    result.module = {sha256: hash(moduleBytes), wasmHex: hex(moduleBytes)};
    status('core_begin', 0); active = true;
    const backing = word(context + 64), size = word(context + 68) * 4096;
    const physical = address => {
      const slot = word(word(context + 56) + (address >>> 12) * 4);
      assert(slot > 0); return backing + (slot - 1) * 4096 + (address & 4095);
    };
    const beforeContext = slice(context, 128), wantedContext = beforeContext.slice();
    const wanted = new DataView(wantedContext.buffer), wantedRam = slice(backing, size);
    for (const [address, width, value] of expected.writes ?? []) {
      for (let index = 0; index < width; index++) wantedRam[physical(address + index) - backing] = value >>> (index * 8);
    }
    const versionPointer = word(context + 80), beforeVersion = view().getBigUint64(versionPointer, true);
    const budget = options.budget ?? 1, stop = run(context, budget) >>> 0;
    result.executed = {stop, budget, context: hex(slice(context, 128)), arena: hex(slice(arena, api.arena_len())),
      backing: hex(slice(backing, size)), beforeContext: hex(beforeContext), transactionDelta: String(view().getBigUint64(versionPointer, true) - beforeVersion)};
    checkpoint();
    (expected.registers ?? registers).forEach((value, index) => wanted.setUint32(index * 4, value >>> 0, true));
    wanted.setUint32(32, expected.pc, true); wanted.setUint32(40, expected.retired, true);
    wanted.setUint32(44, budget - expected.retired, true);
    if (expected.fault) {
      wanted.setUint32(48, expected.fault[0], true); wanted.setUint32(52, expected.fault[1], true);
      wanted.setUint32(116, expected.fault[2], true);
    }
    assert.equal(stop, expected.stop, `${name}: exact stop`);
    assert.deepEqual(slice(context, 128), wantedContext, `${name}: full context including unchanged flags`);
    assert.deepEqual(slice(backing, size), wantedRam, `${name}: whole canonical backing`);
    assert.equal(result.executed.transactionDelta, expected.retired ? '1' : '0', `${name}: one transaction or no writes`);
    assertBanks('unpublished');
    status('core_end', 0); active = false;
    status('core_export', 0, stop, expected.retired);
    assertBanks('published unchanged');
    result.final = {context: hex(slice(context, 128)), arena: hex(slice(arena, api.arena_len())), backing: hex(slice(backing, size))};
    result.complete = true; checkpoint();
  } finally {
    if (active) api.core_end();
    if (opened) api.close();
  }
}

try {
  for (const width of [2, 4]) {
    const name = width === 2 ? 'pusha16' : 'pusha32', code = width === 2 ? '6660' : '60';
    const original = seedRegisters(0x4800), registers = [...original]; registers[4] -= 8 * width;
    runCase(`${name}-all-registers-and-original-sp`, code, {}, {stop: 2, pc: 0x1000 + code.length / 2, retired: 1, registers,
      writes: original.map((value, index) => [0x4800 - (index + 1) * width, width, value])});
    const faultRegisters = seedRegisters(0x4000 + 7 * width);
    runCase(`${name}-last-push-denial-is-whole-frame-neutral`, code, {registers: faultRegisters, readOnlyData: true},
      {stop: 3, pc: 0x1000, retired: 0, fault: [0x4000 - width, width | 2 << 8 | 2 << 16, width << 8 | 14]});
    const codeRegisters = seedRegisters(0x1000 + 8 * width), committed = [...codeRegisters]; committed[4] = 0x1000;
    runCase(`${name}-code-write-commits-whole-frame-before-stop`, code, {registers: codeRegisters, budget: 8},
      {stop: 6, pc: 0x1000 + code.length / 2, retired: 1, registers: committed,
        writes: codeRegisters.map((value, index) => [0x1000 + (7 - index) * width, width, value])});
    const enterCode = width === 2 ? '66c8200000' : 'c8200000', enterRegisters = seedRegisters(0x4800);
    const enterExpected = [...enterRegisters]; enterExpected[4] = 0x4800 - width - 32;
    enterExpected[5] = width === 2 ? 0x102047fe : 0x47fc;
    runCase(`enter${width * 8}-allocates-without-initializing-locals`, enterCode, {},
      {stop: 2, pc: 0x1000 + enterCode.length / 2, retired: 1, registers: enterExpected, writes: [[0x4800 - width, width, enterRegisters[5]]]});
    runCase(`enter${width * 8}-allocated-frame-denial-preserves-staged-push`, enterCode,
      {registers: seedRegisters(0x4000 + width), readOnlyData: true},
      {stop: 3, pc: 0x1000, retired: 0, fault: [0x3fe0, width | 2 << 8 | 2 << 16, width << 8 | 14]});
  }
  const nested = seedRegisters(0x4800); nested[5] = 0x4840;
  const nestedExpected = [...nested]; nestedExpected[4] = 0x47d4; nestedExpected[5] = 0x47fc;
  runCase('enter32-nested-frame-copy', 'c8200002', {registers: nested, initial: [[0x483c, 4, 0xcafebabe]]},
    {stop: 2, pc: 0x1004, retired: 1, registers: nestedExpected,
      writes: [[0x47fc, 4, 0x4840], [0x47f8, 4, 0xcafebabe], [0x47f4, 4, 0x47fc]]});
  const overlap = seedRegisters(0x4800); overlap[5] = 0x4800;
  runCase('enter32-overlapping-read-observes-staged-push', 'c8200002', {registers: overlap},
    {stop: 2, pc: 0x1004, retired: 1, registers: nestedExpected,
      writes: [[0x47fc, 4, 0x4800], [0x47f8, 4, 0x4800], [0x47f4, 4, 0x47fc]]});
  const unmapped = seedRegisters(0x4800); unmapped[5] = 0x6004;
  runCase('enter32-frame-read-fault-keeps-staged-first-push-private', 'c8200002', {registers: unmapped},
    {stop: 3, pc: 0x1000, retired: 0, fault: [0x6000, 4 | 1 << 8 | 1 << 16, 4 << 8 | 14]});
  report.complete = true; checkpoint();
  console.log(JSON.stringify({complete: true, cases: report.cases.length, engineSha256: engine.sha256}));
} catch (error) {
  report.failure = {stage, message: String(error), stack: error.stack}; checkpoint(); throw error;
} finally {closeSync(output);}
