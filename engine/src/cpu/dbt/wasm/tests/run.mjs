import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {existsSync, readFileSync} from 'node:fs';
import {join} from 'node:path';

const [outputDir, root] = process.argv.slice(2);
assert.ok(outputDir && root, 'expected artifact directory and repo root');
const contexts = new Map();
const moduleHashes = new Map();
const defaults = {state: 128, exit: 256, cancel: 400};
const allowedFlags = 0x0cd7;
let runs = 0;

function hash(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function context(name) {
  if (contexts.has(name)) return contexts.get(name);
  const bytes = readFileSync(join(outputDir, `${name}.wasm`));
  assert.equal(WebAssembly.validate(bytes), true, `${name}: validation`);
  const module = new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'}], `${name}: imports`);
  assert.ok(WebAssembly.Module.exports(module).some(item => item.name === 'run' && item.kind === 'function'), `${name}: run export`);
  const memory = new WebAssembly.Memory({initial: 1});
  const instance = new WebAssembly.Instance(module, {env: {memory}});
  const result = {module, memory, run: instance.exports.run, bytes: new Uint8Array(memory.buffer), view: new DataView(memory.buffer)};
  contexts.set(name, result);
  moduleHashes.set(name, hash(bytes));
  return result;
}

function initial(eip = 0x1000, eflags = allowedFlags) {
  return {registers: [0x89abcdef, 3, 0x23456789, 0x3456789a, 0x456789ab, 0x56789abc, 0x6789abcd, 0x789abcde], eip, eflags};
}

function copyState(state) {
  return {...state, registers: [...state.registers]};
}

function header(bytes, view, pointer, magic, length) {
  bytes.set(Buffer.from(magic, 'ascii'), pointer);
  view.setUint16(pointer + 4, 1, true);
  view.setUint16(pointer + 6, 1, true);
  view.setUint32(pointer + 8, length, true);
  view.setUint32(pointer + 12, 0, true);
}

function writeState(ctx, state, pointer) {
  header(ctx.bytes, ctx.view, pointer, 'R3ST', 56);
  state.registers.forEach((value, index) => ctx.view.setUint32(pointer + 16 + index * 4, value, true));
  ctx.view.setUint32(pointer + 48, state.eip, true);
  ctx.view.setUint32(pointer + 52, state.eflags, true);
}

function reset(ctx, state, layout = defaults, cancel = 0) {
  ctx.bytes.fill(0xa5);
  writeState(ctx, state, layout.state);
  ctx.view.setUint32(layout.cancel, cancel, true);
}

function readState(ctx, pointer) {
  assert.equal(Buffer.from(ctx.bytes.subarray(pointer, pointer + 4)).toString('ascii'), 'R3ST');
  assert.equal(ctx.view.getUint16(pointer + 4, true), 1);
  assert.equal(ctx.view.getUint16(pointer + 6, true), 1);
  assert.equal(ctx.view.getUint32(pointer + 8, true), 56);
  assert.equal(ctx.view.getUint32(pointer + 12, true), 0);
  return {
    registers: Array.from({length: 8}, (_, index) => ctx.view.getUint32(pointer + 16 + index * 4, true)),
    eip: ctx.view.getUint32(pointer + 48, true),
    eflags: ctx.view.getUint32(pointer + 52, true),
  };
}

function canonicalExit(reason, retired) {
  const bytes = new Uint8Array(40);
  const view = new DataView(bytes.buffer);
  header(bytes, view, 0, 'R3EX', 40);
  view.setUint32(16, reason, true);
  view.setUint32(20, retired, true);
  return bytes;
}

function call(ctx, budget, reason, retired, expected, label, layout = defaults, cancel = 0) {
  ctx.bytes.fill(0xa5, layout.exit, layout.exit + 40);
  assert.equal(ctx.run(layout.state, layout.exit, budget, layout.cancel), 0, `${label}: host status`);
  runs++;
  assert.deepEqual(readState(ctx, layout.state), expected, `${label}: state`);
  assert.deepEqual(ctx.bytes.slice(layout.exit, layout.exit + 40), canonicalExit(reason, retired), `${label}: canonical exit`);
  assert.equal(ctx.view.getUint32(layout.cancel, true), cancel, `${label}: cancellation word unchanged`);
}

function execute(name, state, expected, label, budget = 1, reason = 1, retired = 1, layout = defaults, cancel = 0) {
  const ctx = context(name);
  reset(ctx, state, layout, cancel);
  call(ctx, budget, reason, retired, expected, label, layout, cancel);
}

function unchanged(before, after, label) {
  const difference = after.findIndex((byte, index) => byte !== before[index]);
  assert.equal(difference, -1, `${label}: changed byte ${difference}`);
}

function arithmetic(kind, left, right, oldFlags) {
  const a = BigInt(left), b = BigInt(right);
  const unsigned = kind === 'add' ? a + b : a - b;
  const result = BigInt.asUintN(32, unsigned);
  const carry = kind === 'add' ? unsigned > 0xffffffffn : a < b;
  const auxiliary = kind === 'add' ? (a % 16n) + (b % 16n) > 15n : a % 16n < b % 16n;
  const signA = a >= 0x80000000n, signB = b >= 0x80000000n, signR = result >= 0x80000000n;
  const overflow = kind === 'add' ? signA === signB && signR !== signA : signA !== signB && signR !== signA;
  let ones = 0;
  for (let byte = Number(result % 256n); byte !== 0; byte >>>= 1) ones += byte & 1;
  const flags = 2 | (oldFlags & 0x400) | (carry ? 1 : 0) | (ones % 2 === 0 ? 4 : 0) |
    (auxiliary ? 0x10 : 0) | (result === 0n ? 0x40 : 0) | (signR ? 0x80 : 0) | (overflow ? 0x800 : 0);
  return {result: Number(result), flags};
}

const literalArithmetic = [
  ['add', 0, 0, 0, 0x46],
  ['add', 0xffffffff, 1, 0, 0x57],
  ['add', 0x7fffffff, 1, 0x80000000, 0x896],
  ['add', 0x80000000, 0x80000000, 0, 0x847],
  ['add', 0xf, 1, 0x10, 0x12],
  ['add', 1, 2, 3, 6],
  ['sub', 0, 1, 0xffffffff, 0x97],
  ['sub', 0x80000000, 1, 0x7fffffff, 0x816],
  ['sub', 0x7fffffff, 0xffffffff, 0x80000000, 0x887],
  ['sub', 0x10, 1, 0xf, 0x16],
  ['sub', 1, 1, 0, 0x46],
];
for (const [kind, left, right, result, flags] of literalArithmetic) {
  assert.deepEqual(arithmetic(kind, left, right, 2), {result, flags}, `math oracle literal ${kind}/${left}/${right}`);
  for (const df of [0, 0x400]) {
    for (const name of kind === 'sub' ? ['sub', 'cmp'] : ['add']) {
      const state = initial(0x1000, 0x8d7 | df);
      state.registers[0] = left;
      state.registers[3] = right;
      const expected = copyState(state);
      if (name !== 'cmp') expected.registers[0] = result;
      expected.eflags = flags | df;
      expected.eip = 0x1002;
      execute(name, state, expected, `literal ${name}/${left}/${right}/df${df}`);
    }
  }
}

for (const name of ['add', 'sub', 'cmp']) {
  for (const left of [0, 1, 2, 3, 0xf, 0x10, 0x7fffffff, 0x80000000, 0xfffffffe, 0xffffffff]) {
    for (const right of [0, 1, 2, 3, 0xf, 0x10, 0x7fffffff, 0x80000000, 0xfffffffe, 0xffffffff]) {
      const state = initial(0x1000, ((left ^ right) & 1) ? allowedFlags : 0x8d7);
      state.registers[0] = left;
      state.registers[3] = right;
      const result = arithmetic(name === 'add' ? 'add' : 'sub', left, right, state.eflags);
      const expected = copyState(state);
      if (name !== 'cmp') expected.registers[0] = result.result;
      expected.eflags = result.flags;
      expected.eip = 0x1002;
      execute(name, state, expected, `math ${name}/${left}/${right}`);
    }
  }
}

for (const [name, kind, destination, rhs, length] of [
  ['add_imm', 'add', 0, () => 0xffffffff, 3],
  ['sub_imm', 'sub', 0, () => 127, 3],
  ['cmp_imm', 'cmp', 0, () => 0x12345678, 5],
  ['alias_add', 'add', 7, state => state.registers[1], 2],
  ['alias_sub', 'sub', 2, state => state.registers[1], 2],
  ['alias_cmp', 'cmp', 1, state => state.registers[2], 2],
  ['add_same', 'add', 0, state => state.registers[0], 2],
  ['sub_same', 'sub', 0, state => state.registers[0], 2],
  ['cmp_same', 'cmp', 0, state => state.registers[0], 2],
]) {
  for (const left of [0, 0xf, 0x7fffffff, 0x80000000, 0xffffffff]) {
    const state = initial();
    state.registers[destination] = left;
    const result = arithmetic(kind === 'add' ? 'add' : 'sub', left, rhs(state), state.eflags);
    const expected = copyState(state);
    if (kind !== 'cmp') expected.registers[destination] = result.result;
    expected.eflags = result.flags;
    expected.eip = 0x1000 + length;
    execute(name, state, expected, `alias/immediate ${name}/${left}`);
  }
}

const conditions = [
  flags => flags.of,
  flags => !flags.of,
  flags => flags.cf,
  flags => !flags.cf,
  flags => flags.zf,
  flags => !flags.zf,
  flags => flags.cf || flags.zf,
  flags => !flags.cf && !flags.zf,
  flags => flags.sf,
  flags => !flags.sf,
  flags => flags.pf,
  flags => !flags.pf,
  flags => flags.sf !== flags.of,
  flags => flags.sf === flags.of,
  flags => flags.zf || flags.sf !== flags.of,
  flags => !flags.zf && flags.sf === flags.of,
];
for (let bits = 0; bits < 128; bits++) {
  const masks = [1, 4, 0x10, 0x40, 0x80, 0x400, 0x800];
  const eflags = masks.reduce((flags, mask, index) => flags | ((bits & (1 << index)) ? mask : 0), 2);
  const flags = {cf: !!(eflags & 1), pf: !!(eflags & 4), zf: !!(eflags & 0x40), sf: !!(eflags & 0x80), of: !!(eflags & 0x800)};
  for (let condition = 0; condition < 16; condition++) {
    const state = initial(0x1000, eflags);
    const expected = copyState(state);
    expected.eip = conditions[condition](flags) ? 0x1004 : 0x1002;
    execute(`jcc_${condition}`, state, expected, `condition ${condition}/flags${eflags}`);
  }
}

for (const flags of [2, 0x8d7, allowedFlags]) {
  for (const [name, pointer, next, mutate] of [
    ['mov_reg', 0x1000, 0x1002, state => {state.registers[7] = state.registers[3];}],
    ['mov_imm', 0x1000, 0x1005, state => {state.registers[6] = 0x12345678;}],
    ['nop', 0x1000, 0x1001, () => {}],
    ['jump', 0x1000, 0x1007, () => {}],
    ['jump_negative', 0x1000, 0x1000, () => {}],
    ['jump_wrap', 0xffffff00, 0x105, () => {}],
    ['nop_final', 0xffffffff, 0, () => {}],
  ]) {
    const state = initial(pointer, flags);
    const expected = copyState(state);
    mutate(expected);
    expected.eip = next;
    execute(name, state, expected, `flags preserved ${name}/${flags}`);
  }
}
execute('jump_negative', initial(), initial(), 'bounded direct self loop', 7, 1, 7);

const baseline = context('baseline');
assert.throws(() => new WebAssembly.Instance(baseline.module, {env: {memory: new WebAssembly.Memory({initial: 0})}}), WebAssembly.LinkError, 'import requires at least one page');
assert.throws(() => new WebAssembly.Instance(baseline.module, {env: {memory: new WebAssembly.Memory({initial: 1, maximum: 1, shared: true})}}), WebAssembly.LinkError, 'import must be unshared');

const sourcePath = join(root, 'engine/src/cpu/dbt/wasm/fixtures/baseline_oracle.c');
const sourceHash = hash(readFileSync(sourcePath));
let oraclePath = join(root, 'target/p2-baseline-fixtures/baseline_oracle');
let oracleReused = false;
const provenancePath = join(root, 'target/p2-baseline-fixtures/provenance.json');
if (existsSync(provenancePath) && existsSync(oraclePath)) {
  const provenance = JSON.parse(readFileSync(provenancePath, 'utf8'));
  oracleReused = provenance.sha256?.['engine/src/cpu/dbt/wasm/fixtures/baseline_oracle.c'] === sourceHash &&
    provenance.sha256?.['target/p2-baseline-fixtures/baseline_oracle'] === hash(readFileSync(oraclePath));
}
if (!oracleReused) {
  oraclePath = join(outputDir, 'baseline_oracle');
  execFileSync('clang', ['-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', sourcePath, '-o', oraclePath], {stdio: ['ignore', 'pipe', 'pipe']});
}
const nativeInputs = [0, 1, 2, 10, 100, 1000, 100000];
for (const input of nativeInputs) {
  const native = JSON.parse(execFileSync(oraclePath, [String(input)], {encoding: 'utf8'}));
  assert.deepEqual(native, {sum: Number(BigInt.asUintN(32, BigInt(input) * BigInt(input + 1) / 2n)), remaining: 0, retired: 5 * input + 3}, `native C oracle ${input}`);
  const state = initial();
  state.registers[1] = input;
  const expected = copyState(state);
  expected.registers[0] = native.sum;
  expected.registers[1] = native.remaining;
  expected.eip = 0x1011;
  expected.eflags = 0x46 | (state.eflags & 0x400);
  execute('baseline', state, expected, `native agreement ${input}`, native.retired + 10, 3, native.retired);
}

for (const [pc, next, eax, ecx, flags] of [
  [0x1000, 0x1005, 0, 3, 0x402],
  [0x1005, 0x1008, 10, 3, 0x406],
  [0x1008, 0x100a, 10, 3, 0x402],
  [0x100a, 0x100c, 13, 3, 0x402],
  [0x100c, 0x100f, 10, 2, 0x402],
  [0x100f, 0x1005, 10, 3, 0x402],
]) {
  const state = initial(pc, 0x402);
  state.registers[0] = 10;
  const expected = copyState(state);
  expected.registers[0] = eax;
  expected.registers[1] = ecx;
  expected.eip = next;
  expected.eflags = flags;
  execute('baseline', state, expected, `interior PC resume ${pc}`);
}

for (let index = 0; index < 64; index++) {
  for (const budget of [0, 1, 7, 64, 129]) {
    const state = initial(0x1000 + index);
    const expected = copyState(state);
    expected.eip = 0x1000 + (index + budget) % 64;
    execute('dense_resume', state, expected, `dense resume ${index}/${budget}`, budget, 1, budget);
  }
  const state = initial(0x1000 + index);
  execute('dense_resume', state, state, `dense cancel ${index}`, 129, 2, 0, defaults, 1);
}
for (const pc of [0x0fff, 0x1040, 0x1041]) {
  const state = initial(pc);
  execute('dense_resume', state, state, `dense unknown PC ${pc}`, 129, 3, 0);
}

const checkpoint = initial();
checkpoint.registers[1] = 10;
reset(baseline, checkpoint);
const afterMov = copyState(checkpoint);
afterMov.registers[0] = 0;
afterMov.eip = 0x1005;
call(baseline, 1, 1, 1, afterMov, 'first instruction checkpoint');
const afterCmp = copyState(afterMov);
afterCmp.eip = 0x1008;
afterCmp.eflags = 0x406;
call(baseline, 1, 1, 1, afterCmp, 'second instruction checkpoint');

const longState = initial();
longState.registers[1] = 1000;
reset(baseline, longState);
let total = 0, resumes = 0;
while (true) {
  baseline.bytes.fill(0xa5, defaults.exit, defaults.exit + 40);
  assert.equal(baseline.run(defaults.state, defaults.exit, 7, defaults.cancel), 0, 'long resume status');
  runs++;
  resumes++;
  const reason = baseline.view.getUint32(defaults.exit + 16, true);
  const retired = baseline.view.getUint32(defaults.exit + 20, true);
  assert.deepEqual(baseline.bytes.slice(defaults.exit, defaults.exit + 40), canonicalExit(reason, retired), 'long resume canonical exit');
  assert.ok(retired <= 7);
  assert.ok(reason === 1 || reason === 3);
  if (reason === 1) assert.equal(retired, 7);
  total += retired;
  if (reason === 3) break;
  assert.ok(resumes < 1000, 'long resume terminates');
}
const longExpected = copyState(longState);
longExpected.registers[0] = 500500;
longExpected.registers[1] = 0;
longExpected.eflags = 0x446;
longExpected.eip = 0x1011;
assert.deepEqual(readState(baseline, defaults.state), longExpected, 'long resume final state');
assert.equal(total, 5003, 'long resume retired total');

for (const [pc, budget, cancel, reason] of [
  [0x1000, 0, 0, 1],
  [0x1000, 0, 1, 2],
  [0x1000, 100, 1, 2],
  [0x12345678, 0, 0, 1],
  [0x12345678, 0, 1, 2],
  [0x12345678, 100, 0, 3],
  [0x12345678, 0xffffffff, 0, 3],
  [0x12345678, 100, 1, 2],
]) {
  const state = initial(pc);
  execute('baseline', state, state, `safepoint priority pc${pc}/budget${budget}/cancel${cancel}`, budget, reason, 0, defaults, cancel);
}
reset(baseline, checkpoint);
call(baseline, 1, 1, 1, afterMov, 'before cancellation resume');
baseline.view.setUint32(defaults.cancel, 1, true);
call(baseline, 100, 2, 0, afterMov, 'cancel between resumes', defaults, 1);
baseline.view.setUint32(defaults.cancel, 0, true);
const finished = copyState(checkpoint);
finished.registers[0] = 55;
finished.registers[1] = 0;
finished.eflags = 0x446;
finished.eip = 0x1011;
call(baseline, 100, 3, 52, finished, 'continue after cancellation cleared');

const nop = context('nop');
for (const layout of [
  defaults,
  {state: 129, exit: 257, cancel: 401},
  {state: 128, exit: 184, cancel: 224},
  {state: 65480, exit: 256, cancel: 400},
  {state: 128, exit: 65496, cancel: 400},
  {state: 128, exit: 256, cancel: 65532},
  {state: 65436, exit: 65492, cancel: 65532},
]) {
  const state = initial();
  const expected = copyState(state);
  expected.eip = 0x1001;
  execute('nop', state, expected, `valid layout ${JSON.stringify(layout)}`, 1, 1, 1, layout);
}

for (const layout of [
  {state: 65481, exit: 256, cancel: 400},
  {state: 128, exit: 65497, cancel: 400},
  {state: 128, exit: 256, cancel: 65533},
  {state: 0xffffffff, exit: 256, cancel: 400},
  {state: 128, exit: 0xffffffff, cancel: 400},
  {state: 128, exit: 256, cancel: 0xffffffff},
  {state: 65536, exit: 256, cancel: 400},
  {state: 128, exit: 65536, cancel: 400},
  {state: 128, exit: 256, cancel: 65536},
  {state: 128, exit: 160, cancel: 400},
  {state: 128, exit: 96, cancel: 400},
  {state: 128, exit: 128, cancel: 400},
  {state: 128, exit: 256, cancel: 130},
  {state: 128, exit: 256, cancel: 257},
  {state: 128, exit: 256, cancel: 182},
]) {
  reset(nop, initial());
  if (layout.state + 56 <= nop.bytes.length) writeState(nop, initial(), layout.state);
  const before = nop.bytes.slice();
  assert.equal(nop.run(layout.state, layout.exit, 1, layout.cancel), 1, `invalid pointers ${JSON.stringify(layout)}`);
  runs++;
  unchanged(before, nop.bytes, `invalid pointer atomicity ${JSON.stringify(layout)}`);
}
for (let offset = 0; offset < 16; offset++) {
  reset(nop, initial());
  nop.bytes[defaults.state + offset] ^= 0x80;
  const before = nop.bytes.slice();
  assert.equal(nop.run(defaults.state, defaults.exit, 1, defaults.cancel), 2, `bad state header byte ${offset}`);
  runs++;
  unchanged(before, nop.bytes, `bad state header atomicity ${offset}`);
}
for (const flags of [0, 0xcd5, 0xa, 0xcdf, 0x80000002]) {
  reset(nop, initial(0x1000, flags));
  const before = nop.bytes.slice();
  assert.equal(nop.run(defaults.state, defaults.exit, 1, defaults.cancel), 2, `bad state flags ${flags}`);
  runs++;
  unchanged(before, nop.bytes, `bad state flags atomicity ${flags}`);
}

const aggregate = [...moduleHashes].sort(([a], [b]) => a.localeCompare(b)).map(([name, digest]) => `${name}:${digest}`).join('\n');
console.log(JSON.stringify({modules: contexts.size, runs, native_cases: nativeInputs.length, resumes, oracle_reused: oracleReused, oracle_sha256: sourceHash, module_set_sha256: hash(aggregate), artifacts: outputDir}));
