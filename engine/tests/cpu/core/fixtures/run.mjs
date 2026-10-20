import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {arithmetic, loopOutcome} from './oracle.mjs';
import {runExtended} from './extended.mjs';
import {runBits} from './bits.mjs';

const directory = process.argv[2];
const PAGE = 4096, MAP = 0x10000, PERM = 0x410000;
let CTX = 0x100;
const EXEC = 0x510000, VERSION = 0x510100, PAGE_VERSION = 0x510200;
const BACKING = 0x520000, GENERATION = 0x530000;
const memory = new WebAssembly.Memory({initial: 96});
const bytes = new Uint8Array(memory.buffer), view = new DataView(memory.buffer);
const get = offset => view.getUint32(offset, true);
const put = (offset, value) => view.setUint32(offset, value >>> 0, true);
const hash = value => createHash('sha256').update(value).digest('hex');
const programs = new Map(), results = [];
const reportPath = process.env.RING3_CORE_REPORT ?? join(directory, 'result.json');
let active;

function fault(address, width, access, reason) {
  put(CTX + 48, address); put(CTX + 52, width | access << 8 | reason << 16);
}

function physical(address) {
  const slot = get(MAP + (address >>> 12) * 4);
  return slot === 0 ? null : BACKING + (slot - 1) * PAGE + (address & 4095);
}

function preflight(address, width, access, modify = false) {
  const locations = [];
  for (let index = 0; index < width; index++) {
    const current = (address + index) >>> 0, location = physical(current);
    const reportedAccess = modify ? 2 : access, required = modify ? 3 : access;
    if (location === null) { fault(current, width, reportedAccess, 1); return null; }
    const permissions = bytes[PERM + (current >>> 12)];
    if ((permissions & required) !== required) { fault(current, width, reportedAccess, 2); return null; }
    locations.push(location);
  }
  return locations;
}

function read(context, address, encodedWidth) {
  assert.equal(context, CTX); active.reads++;
  assert.equal(encodedWidth & ~0x1ff, 0);
  const width = encodedWidth & 255;
  assert.ok([1, 2, 4].includes(width));
  const locations = preflight(address >>> 0, width, 1, !!(encodedWidth & 0x100));
  if (!locations) return 1n << 32n;
  let value = 0;
  for (let index = 0; index < width; index++) value += bytes[locations[index]] * 2 ** (index * 8);
  return BigInt(value >>> 0);
}

function write(context, address, width, value) {
  assert.equal(context, CTX); active.writes++;
  const locations = preflight(address >>> 0, width, 2);
  if (!locations) return 1;
  for (let index = 0; index < width; index++) bytes[locations[index]] = value >>> (index * 8);
  const version = view.getBigUint64(VERSION, true) + 1n;
  view.setBigUint64(VERSION, version, true);
  let invalidated = false;
  for (const page of new Set(Array.from({length: width}, (_, index) => ((address + index) >>> 0) >>> 12))) {
    const slot = get(MAP + page * 4) - 1;
    view.setBigUint64(PAGE_VERSION + slot * 16 + 8, version, true);
    invalidated ||= get(EXEC + slot * 4) !== 0;
  }
  if (invalidated) view.setBigUint64(GENERATION, view.getBigUint64(GENERATION, true) + 1n, true);
  return invalidated ? 2 : 0;
}

for (const line of readFileSync(join(directory, 'programs.tsv'), 'utf8').trim().split('\n')) {
  const [name, hex, identity] = line.split('\t'), wasm = readFileSync(join(directory, `${name}.wasm`));
  assert.ok(WebAssembly.validate(wasm), `${name}: validates`);
  const module = new WebAssembly.Module(wasm);
  const imports = WebAssembly.Module.imports(module).sort((a, b) => a.name.localeCompare(b.name));
  assert.deepEqual(imports, [
    {module: 'ring3', name: 'memory', kind: 'memory'},
    {module: 'ring3', name: 'read', kind: 'function'},
    {module: 'ring3', name: 'write', kind: 'function'},
  ], `${name}: declared imports only`);
  const instance = new WebAssembly.Instance(module, {ring3: {memory, read, write}});
  assert.equal(typeof instance.exports.run, 'function');
  assert.equal(instance.exports.run.length, 2);
  programs.set(name, {code: Buffer.from(hex, 'hex'), identity: BigInt(identity), wasmSha256: hash(wasm), run: instance.exports.run});
}

function seed(name, options = {}) {
  active = {reads: 0, writes: 0};
  bytes.fill(0, CTX, CTX + 128); bytes.fill(0, BACKING, BACKING + 16 * PAGE);
  bytes.fill(0, EXEC, EXEC + 16 * 4); bytes.fill(0, VERSION, VERSION + 8);
  bytes.fill(0, PAGE_VERSION, PAGE_VERSION + 16 * 16);
  bytes.fill(0, GENERATION, GENERATION + 8);
  for (const page of [0, 1, 3, 4, 5, 0xfffff]) {
    put(MAP + page * 4, 0); bytes[PERM + page] = 0;
  }
  for (const [page, slot, permissions] of [[1, 7, 7], [3, 2, 3], [4, 11, 3], [5, 4, 3]]) {
    put(MAP + page * 4, slot + 1); bytes[PERM + page] = permissions;
  }
  if (options.mapZero) { put(MAP, 6); bytes[PERM] = 3; }
  put(EXEC + 7 * 4, 1);
  bytes.set(programs.get(name).code, physical(0x1000));
  const regs = options.regs ?? [5, 3, 7, 0x3000, 0x4800, 0, 0x8000, 0];
  regs.forEach((value, index) => put(CTX + index * 4, value));
  put(CTX + 32, 0x1000); put(CTX + 36, options.flags ?? 2);
  put(CTX + 40, options.retired ?? 0); put(CTX + 44, 0x12345678);
  [MAP, PERM, BACKING, 16, EXEC, 0, VERSION, PAGE_VERSION, GENERATION].forEach((value, index) => put(CTX + 56 + index * 4, value));
  put(CTX + 104, options.fsBase ?? 0);
  put(CTX + 112, options.inactive ? 0 : 1);
  view.setBigUint64(CTX + 120, programs.get(name).identity + (options.wrongIdentity ? 1n : 0n), true);
  put(physical(0x4800), 0x8000);
  for (const [address, width, value] of options.initial ?? []) {
    for (let index = 0; index < width; index++) {
      const target = physical((address + index) >>> 0); assert.notEqual(target, null);
      bytes[target] = value >>> (index * 8);
    }
  }
  if (options.readOnly) bytes[PERM + 3] = 1;
  if (options.unmapStack) { put(MAP + 4 * 4, 0); bytes[PERM + 4] = 0; }
  if (options.secondReadOnly) bytes[PERM + 4] = 1;
  return {regs: [...regs], flags: options.flags ?? 2, retired: options.retired ?? 0};
}

function check(name, label, budget, options, outcome) {
  const initial = seed(name, options), before = bytes.slice(CTX, CTX + 128);
  const expectedRam = bytes.slice(BACKING, BACKING + 16 * PAGE);
  const expectedVersions = new Uint8Array(16 * 16), versions = new DataView(expectedVersions.buffer);
  const expected = typeof outcome === 'function' ? outcome(initial) : outcome;
  for (const [ordinal, [address, width, value]] of (expected.writes ?? []).entries()) {
    for (let index = 0; index < width; index++) {
      const target = physical((address + index) >>> 0) - BACKING;
      expectedRam[target] = value >>> (index * 8);
      versions.setBigUint64(Math.floor(target / PAGE) * 16 + 8, BigInt(ordinal + 1), true);
    }
  }
  let reason, remainingBudget = budget;
  const moduleSequence = [];
  for (const [index, selected] of (options?.sequence ?? [name]).entries()) {
    const program = programs.get(selected); assert(program, `declared generated module ${selected}`);
    if (index > 0) view.setBigUint64(CTX + 120, program.identity, true);
    reason = program.run(CTX, remainingBudget) >>> 0;
    moduleSequence.push({name: selected, reason, budget: remainingBudget,
      context: Buffer.from(bytes.subarray(CTX, CTX + 128)).toString('hex')});
    remainingBudget = get(CTX + 44);
    if (reason !== 1) break;
  }
  const raw = {name, label, budget, reason, context: Buffer.from(bytes.subarray(CTX, CTX + 128)).toString('hex'),
    moduleSequence,
    ramSha256: hash(bytes.subarray(BACKING, BACKING + 16 * PAGE)), helperReads: active.reads, helperWrites: active.writes,
    transactionVersion: String(view.getBigUint64(VERSION, true)), codeGeneration: String(view.getBigUint64(GENERATION, true))};
  results.push(raw); writeFileSync(reportPath, JSON.stringify({schema: 1, complete: false, results}));
  assert.equal(moduleSequence.length, (options?.sequence ?? [name]).length, `${label}: all declared module boundaries`);
  assert(moduleSequence.slice(0,-1).every(step => step.reason === 1), `${label}: only NeedCode selects a following module`);
  assert.equal(reason, expected.reason, `${label}: stop reason`);
  const wanted = new DataView(before.buffer);
  (expected.regs ?? initial.regs).forEach((value, index) => wanted.setUint32(index * 4, value >>> 0, true));
  wanted.setUint32(32, expected.pc, true); wanted.setUint32(36, expected.flags ?? initial.flags, true);
  wanted.setUint32(40, initial.retired + expected.retired, true);
  if (expected.reason !== 8) wanted.setUint32(44, budget - (expected.work ?? expected.retired), true);
  if (expected.fault) { wanted.setUint32(48, expected.fault[0], true); wanted.setUint32(52, expected.fault[1], true); }
  assert.deepEqual(bytes.subarray(CTX, CTX + 128), before, `${label}: complete public context`);
  assert.deepEqual(bytes.subarray(BACKING, BACKING + 16 * PAGE), expectedRam, `${label}: whole code/data/stack backing`);
  assert.equal(active.reads, expected.reads ?? 0, `${label}: read slow path`);
  assert.equal(active.writes, expected.helpers ?? 0, `${label}: write slow path`);
  assert.equal(view.getBigUint64(VERSION, true), BigInt(expected.writes?.length ?? 0), `${label}: store transaction count`);
  assert.deepEqual(bytes.subarray(PAGE_VERSION, PAGE_VERSION + 16 * 16), expectedVersions, `${label}: all physical page versions`);
  assert.equal(view.getBigUint64(GENERATION, true), expected.reason === 6 ? 1n : 0n, `${label}: code generation`);
}

const arithmeticCases = [
  ['add', 0xffffffff, 2], ['add', 0x7fffffff, 2], ['sub', 0, 2], ['sub', 0x80000000, 2],
  ['xor', 0x80000000, 0x403], ['inc', 0xffffffff, 3], ['dec', 0x80000000, 3],
  ['adc', 0xfffffffe, 3], ['adc', 0x7ffffffe, 3], ['sbb', 0, 3], ['neg', 0x80000000, 2],
  ['add8', 0x123456ff, 2], ['add8', 0x1234567f, 2], ['add16', 0x1234ffff, 2], ['add16', 0x12347fff, 2],
];
for (const [name, lhs, flags] of arithmeticCases) {
  check(name, `${name}-${lhs.toString(16)}`, 8, {regs: [lhs, 3, 0x80000001, 0x3000, 0x4800, 0, 0x8000, 0], flags}, initial => {
    const width = name.endsWith('8') ? 8 : name.endsWith('16') ? 16 : 32;
    const kind = name.replace(/\d/g, ''), rhs = kind === 'xor' ? initial.regs[2] : 1;
    const value = arithmetic(kind, lhs, rhs, flags & 1, width, flags), regs = [...initial.regs];
    regs[0] = width === 32 ? value.value : ((lhs & (width === 8 ? 0xffffff00 : 0xffff0000)) | value.value) >>> 0;
    return {reason: 4, pc: 0x1000 + programs.get(name).code.length - 2, retired: 1, regs, flags: value.flags};
  });
}

for (const width of [8, 16, 32]) {
  const sign = 2 ** (width - 1), mask = 2 ** width - 1;
  for (const kind of ['add', 'sub', 'adc', 'sbb', 'xor', 'inc', 'dec', 'neg']) {
    for (const [index, low] of [0, mask, sign - 1, sign].entries()) {
      const prefix = width === 8 ? 0xa5a55a00 : width === 16 ? 0xa5a50000 : 0;
      const lhs = (prefix | low) >>> 0, flags = 0x402 | (index & 1), name = `flags${width}-${kind}`;
      check(name, `${name}-${index}`, 8, {regs: [lhs, 3, 0x81010181, 0x3000, 0x4800, 0, 0x8000, 0], flags}, initial => {
        const rhs = kind === 'xor' ? initial.regs[2] : 1;
        const value = arithmetic(kind, lhs, rhs, flags & 1, width, flags), regs = [...initial.regs];
        regs[0] = width === 32 ? value.value : (prefix | value.value) >>> 0;
        return {reason: 4, pc: 0x1000 + programs.get(name).code.length - 2, retired: 1, regs, flags: value.flags};
      });
    }
  }
}

for (const [name, kind, values] of [
  ['add-jo', 'add', [[0x7fffffff, true], [0, false]]],
  ['add-jb', 'add', [[0xffffffff, true], [1, false]]],
  ['sub-jl', 'sub', [[0, true], [1, false]]],
]) {
  for (const [lhs, taken] of values) {
    check(name, `${name}-${taken}`, 8, {regs: [lhs, 3, 7, 0x3000, 0x4800, 0, 0x8000, 0]}, initial => {
      const value = arithmetic(kind, lhs, 1, 0, 32, initial.flags), regs = [...initial.regs];
      regs[0] = value.value; regs[3] = taken ? 0x22222222 : 0x11111111;
      return {reason: 4, pc: 0x1011, retired: taken ? 3 : 4, regs, flags: value.flags};
    });
  }
}

const truth = [0x800, 0, 1, 0, 0x40, 0, 1, 0, 0x80, 0, 4, 0, 0x80, 0, 0x40, 0];
const falsity = [0, 0x800, 0, 1, 0, 0x40, 0, 0x40, 0, 0x80, 0, 4, 0, 0x80, 0, 0x40];
for (let condition = 0; condition < 16; condition++) {
  for (const [taken, flagBits] of [[true, truth[condition]], [false, falsity[condition]]]) {
    check(`jcc${condition}`, `jcc${condition}-${taken}`, 8, {flags: flagBits | 2}, initial => {
      const regs = [...initial.regs]; regs[3] = taken ? 0x22222222 : 0x11111111;
      return {reason: 4, pc: 0x100e, retired: taken ? 2 : 3, regs};
    });
  }
}

for (const [name, width, value] of [['load-store', 4, 0x89abcdef], ['load-store8', 1, 0xab], ['load-store16', 2, 0xcdef]]) {
  check(name, name, 8, {regs: [0x12345678, 3, 7, 0x3000, 0x4800, 0, 0x8000, 0], initial: [[0x3000, width, value]]}, initial => {
    const regs = [...initial.regs];
    regs[0] = width === 4 ? value : ((regs[0] & (width === 1 ? 0xffffff00 : 0xffff0000)) | value) >>> 0;
    return {reason: 4, pc: 0x1000 + programs.get(name).code.length - 2, retired: 2, regs, writes: [[0x3000 + width, width, value]]};
  });
}
check('load-store', 'load-budget-prefix', 1, {initial: [[0x3000, 4, 0x89abcdef]]}, initial => {
  const regs = [...initial.regs]; regs[0] = 0x89abcdef;
  return {reason: 2, pc: 0x1002, retired: 1, regs};
});
check('fs-load', 'fs-linear-base-wrap-valid', 8,
  {regs: [5, 3, 7, 0x10, 0x4800, 0, 0x8000, 0], fsBase: 0xfffffff0, mapZero: true, initial: [[0, 4, 0x89abcdef]]}, initial => {
    const regs = [...initial.regs]; regs[0] = 0x89abcdef;
    return {reason: 4, pc: 0x1003, retired: 1, regs};
  });
check('fs-load', 'logical-span-overflow-before-linear-wrap', 8,
  {regs: [5, 3, 7, 0xfffffffe, 0x4800, 0, 0x8000, 0], fsBase: 2, mapZero: true, initial: [[0, 4, 0x89abcdef]]},
  {reason: 4, pc: 0x1000, retired: 0});
check('rmw', 'rmw', 8, {initial: [[0x3000, 4, 0xffffffff]]}, {reason: 4, pc: 0x1003, retired: 1, flags: 0x57, writes: [[0x3000, 4, 0]]});
check('rmw', 'write-permission-preserves-flags', 8, {flags: 0x893, readOnly: true},
  {reason: 3, pc: 0x1000, retired: 0, reads: 1, fault: [0x3000, 4 | 2 << 8 | 2 << 16]});
check('rmw', 'modify-first-page-denial-precedes-next-page', 8,
  {flags: 0x893, regs: [5, 3, 7, 0x3ffe, 0x4800, 0, 0x8000, 0], readOnly: true, unmapStack: true},
  {reason: 3, pc: 0x1000, retired: 0, reads: 1, fault: [0x3ffe, 4 | 2 << 8 | 2 << 16]});
check('load-store', 'load-unmapped', 8, {regs: [5, 3, 7, 0x6000, 0x4800, 0, 0x8000, 0]},
  {reason: 3, pc: 0x1000, retired: 0, reads: 1, fault: [0x6000, 4 | 1 << 8 | 1 << 16]});
check('load-store', 'cross-page-load-atomic', 8, {regs: [5, 3, 7, 0x3ffe, 0x4800, 0, 0x8000, 0], unmapStack: true, initial: [[0x3ffe, 2, 0xabcd]]},
  {reason: 3, pc: 0x1000, retired: 0, reads: 1, fault: [0x4000, 4 | 1 << 8 | 1 << 16]});
check('store', 'cross-page-store-atomic', 8,
  {regs: [0x12345678, 3, 7, 0x3ffe, 0x4800, 0, 0x8000, 0], secondReadOnly: true, initial: [[0x3ffe, 2, 0xabcd]]},
  {reason: 3, pc: 0x1000, retired: 0, helpers: 1, fault: [0x4000, 4 | 2 << 8 | 2 << 16]});
check('store', 'code-store-commits-before-invalidating', 8,
  {regs: [0x12345678, 3, 7, 0x1000, 0x4800, 0, 0x8000, 0]},
  {reason: 6, pc: 0x1002, retired: 1, helpers: 1, writes: [[0x1000, 4, 0x12345678]]});
check('push-pop', 'push-pop', 8, {}, initial => {
  const regs = [...initial.regs]; regs[1] = regs[0];
  return {reason: 4, pc: 0x1002, retired: 2, regs, writes: [[0x47fc, 4, regs[0]]]};
});
check('push-pop', 'stack-span-overflow-is-neutral', 8,
  {regs: [5, 3, 7, 0x3000, 1, 0, 0x8000, 0]},
  {reason: 4, pc: 0x1000, retired: 0});
check('call', 'call-ret-full-across-explicit-module-boundaries', 16, {sequence: ['call', 'call-callee', 'call']}, initial => {
  const regs = [...initial.regs]; regs[0] = 10;
  return {reason: 4, pc: 0x1008, retired: 4, regs, flags: 6, writes: [[0x47fc, 4, 0x1005]]};
});
check('call', 'call-target-is-a-separate-cache-entry', 16, {}, initial => {
  const regs = [...initial.regs]; regs[4] -= 4;
  return {reason: 1, pc: 0x1010, retired: 1, regs, writes: [[0x47fc, 4, 0x1005]]};
});
check('call', 'call-budget-prefix', 1, {}, initial => {
  const regs = [...initial.regs]; regs[4] -= 4;
  return {reason: 2, pc: 0x1010, retired: 1, regs, writes: [[0x47fc, 4, 0x1005]]};
});
check('ret', 'return-outside-region', 8, {}, initial => {
  const regs = [...initial.regs]; regs[4] += 4;
  return {reason: 1, pc: 0x8000, retired: 1, regs};
});
check('ret', 'return-budget-before-missing-target', 1, {}, initial => {
  const regs = [...initial.regs]; regs[4] += 4;
  return {reason: 2, pc: 0x8000, retired: 1, regs};
});
check('indirect', 'indirect-outside-region', 8, {}, {reason: 1, pc: 0x8000, retired: 1});
check('indirect', 'indirect-budget-before-missing-target', 1, {}, {reason: 2, pc: 0x8000, retired: 1});
check('unsupported', 'unsupported-zero-retired', 8, {}, {reason: 4, pc: 0x1000, retired: 0});
check('unsupported', 'zero-budget', 0, {retired: 19}, {reason: 2, pc: 0x1000, retired: 0});
check('add', 'inactive-entry-is-neutral', 8, {inactive: true}, {reason: 8, pc: 0x1000, retired: 0});
check('add', 'wrong-code-identity-is-neutral', 8, {wrongIdentity: true}, {reason: 8, pc: 0x1000, retired: 0});
for (const flag of [0x100, 0x10000, 0x40000]) {
  check('add', `debug-entry-${flag}`, 8, {flags: flag | 2}, {reason: 5, pc: 0x1000, retired: 0});
}
for (const budget of [0, 1, 2, 3, 4, 5, 6, 9, 10, 14, 15, 16]) {
  check('loop', `loop-cut-${budget}`, budget, {retired: 23}, initial => loopOutcome(initial.regs, initial.flags, 3, budget));
}
runExtended(check);
runBits(check);
CTX = 0x101;
try {
  const leading = new Uint8Array(8).fill(0xa5), trailing = new Uint8Array(8).fill(0x5a);
  bytes.set(leading,CTX-8); bytes.set(trailing,CTX+128);
  check('rmw','unaligned-publication-preserves-all-fields-and-neighbors',17,
    {regs:[0x11223344,0x22334455,0x33445566,0x3000,0x4800,0x55667788,0x66778899,0x778899aa],
      flags:0x403,retired:37,initial:[[0x3000,4,0x7fffffff]]},
    {reason:4,pc:0x1003,retired:1,flags:0xc96,writes:[[0x3000,4,0x80000000]]});
  const guards = {leading:bytes.slice(CTX-8,CTX),trailing:bytes.slice(CTX+128,CTX+136)};
  Object.assign(results.at(-1),{contextAddress:CTX,contextGuards:Object.fromEntries(
    Object.entries(guards).map(([name,value])=>[name,Buffer.from(value).toString('hex')]))});
  writeFileSync(reportPath,JSON.stringify({schema:1,complete:false,results}));
  assert.deepEqual(guards.leading,leading,'publication preserves the eight leading context bytes');
  assert.deepEqual(guards.trailing,trailing,'publication preserves the eight trailing context bytes');
} finally {CTX=0x100;}
const report = {schema: 1, complete: true, node: process.version, cases: results.length,
  sources: Object.fromEntries(['./run.mjs', './oracle.mjs', './extended.mjs', './bits.mjs', '../wasm.rs'].map(path => [path, hash(readFileSync(new URL(path, import.meta.url)))])),
  modules: Object.fromEntries([...programs].map(([name, program]) => [name, {identity: String(program.identity), wasmSha256: program.wasmSha256, codeSha256: hash(program.code)}])),
  results};
writeFileSync(reportPath, JSON.stringify(report, null, 2));
console.log(JSON.stringify({complete: true, cases: results.length, modules: programs.size, reportSha256: hash(readFileSync(reportPath))}));
