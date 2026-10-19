import assert from 'node:assert/strict';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {readEngine} from '../../../../../../support/wasm/engine.mjs';
import {pageFaultPacket} from '../../../../../../support/wasm/page-fault.mjs';
import {Fixture, PC, FP, RAM, defined, envelope, ext, packet} from './support.mjs';
import {verifyFinalStores} from './final-store.mjs';
import {verifyGroups} from './groups.mjs';

const [enginePath, output] = process.argv.slice(2);
const engine = readEngine(enginePath), observations = [], modules = new Map();
mkdirSync(output, {recursive: true});
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const read = (opcode, word = 3) => ({opcode, word, control: 0x401});
const reg = (opcode, word) => ({opcode, word, control: 1});
const programs = {
  readRegister: {name: 'read-register', code: [0xd9, 0x03, 0xd8, 0xc0, 0xff, 0xe2],
    steps: [read(0x103), reg(0xc0, 8)]},
  registerRead: {name: 'register-read', code: [0xd9, 0xe0, 0xd9, 0x07, 0xff, 0xe2],
    steps: [reg(0x1e0, 15), read(0x107)]},
  readRead: {name: 'read-read', code: [0xd9, 0x03, 0xd9, 0x07, 0xff, 0xe2],
    steps: [read(0x103), read(0x107)]},
  register: {name: 'register-register', code: [0xd9, 0xe0, 0xd9, 0xe1, 0xff, 0xe2],
    steps: [reg(0x1e0, 15), reg(0x1e1, 16)]},
};
function make(program, options) {
  const c = new Fixture(engine.module, program, options);
  const sha256 = hash(c.bytes);
  if (!modules.has(sha256)) {
    const file = `generated-${modules.size + 1}.wasm`;
    writeFileSync(join(output, file), c.bytes);
    modules.set(sha256, {file, sha256, bytes: c.bytes.length, imports: c.imports});
  }
  return c;
}
function header(result, reason, retired, pc, flags = 0x202, shadow = 0) {
  assert.equal(result.status, 0, 'actual generated run completes its ABI return');
  const a = result.arena;
  assert.deepEqual([a.readUInt32LE(72), a.readUInt32LE(76), a.readUInt32LE(48)],
    [reason, retired, pc], 'literal stop, retirement and exact PC');
  assert.equal(a.readUInt32LE(52), flags, 'literal live flags');
  assert.equal(a.readUInt32LE(12), shadow, 'literal debug-shadow metadata');
  assert.equal(a.readUInt32LE(4), 0x10003, 'full v3 state remains canonical');
  assert.equal(a.readUInt32LE(4604), 0, 'ordinary instructions do not fabricate REP work');
}
function fpValue(result, index, value) {
  assert.deepEqual(result.arena.subarray(FP + 40 + index * 10, FP + 50 + index * 10),
    ext(value), `literal FP physical register ${index} = ${value}`);
}
function compare(label, program, row, verify, {forceOne = false, cancelAfterFirst = false} = {}) {
  const scalar = make(program, {scalar: true}), paired = make(program);
  try {
    assert.deepEqual(paired.bytes, scalar.bytes, 'same generated module, independent real scalar helper arm');
    assert.ok(paired.imports.some(i => i.name === 'x87_resident_pair'), 'resident module actually imports pair helper');
    scalar.seed(row); paired.seed(row);
    scalar.cancelAfterFirst = paired.cancelAfterFirst = cancelAfterFirst;
    paired.forceOne = forceOne;
    const a = scalar.execute(row.budget ?? 2), b = paired.execute(row.budget ?? 2);
    assert.deepEqual(defined(b.arena), defined(a.arena), `${label}: full CPU/exit/helper/extended state`);
    assert.deepEqual(b.receipt, a.receipt, `${label}: real outer receipt and owner authority`);
    assert.equal(b.cr2, a.cr2, `${label}: CR2 parity`);
    verify(b, a);
    scalar.checkMemory(); paired.checkMemory();
    observations.push({label, budget: row.budget ?? 2, pc: row.pc ?? PC,
      reason: b.arena.readUInt32LE(72), retired: b.arena.readUInt32LE(76),
      arena_sha256: hash(defined(b.arena)), cr2: b.cr2,
      pairCalls: b.calls.filter(c => c.name === 'x87_resident_pair'),
      receipt: Object.fromEntries(Object.entries(b.receipt).map(([k, v]) => [k, typeof v === 'bigint' ? v.toString() : v]))});
  } finally {scalar.close(); paired.close();}
}

// every original instruction entry remains executable, including the second pair member.
for (const program of Object.values(programs)) for (const pc of [PC, PC + 2]) {
  for (const budget of [0, 1, 2, 3]) {
    compare(`${program.name}/entry${pc - PC}/budget${budget}`, program, {pc, budget}, result => {
      const available = pc === PC ? 3 : 2;
      const retired = Math.min(available, budget);
      const next = retired === available ? 0x1100 : pc + retired * 2;
      header(result, budget > available ? 3 : 1, retired, next);
      const calls = result.calls.filter(c => c.name === 'x87_resident_pair');
      assert.equal(calls.length, pc === PC && budget > 0 ? 1 : 0, 'side entry never replays the first operation');
      if (calls.length) {
        assert.equal(calls[0].args[8], Math.min(2, budget));
        assert.equal(BigInt(calls[0].result), envelope(0, Math.min(2, budget), budget >= 2 ? 1 : 0),
          'the native pair really completes two operations when eligible');
      }
      if (budget === 0) fpValue(result, 0, 1);
      else if (pc === PC + 2) {
        if (program === programs.readRegister) fpValue(result, 0, 2);
        else if (program === programs.register) fpValue(result, 0, 1);
        else fpValue(result, 7, 5);
      } else if (program === programs.readRegister) fpValue(result, 7, budget === 1 ? 3 : 6);
      else if (program === programs.register) fpValue(result, 0, budget === 1 ? -1 : 1);
      else if (program === programs.registerRead) {
        fpValue(result, 0, -1); if (budget >= 2) fpValue(result, 7, 5);
      } else {fpValue(result, 7, 3); if (budget >= 2) fpValue(result, 6, 5);}
    });
  }
}
const terminal = {...programs.register, name: 'terminal-register-pair',
  code: programs.register.code.slice(0, 4)};
for (const budget of [2, 3]) compare(`${terminal.name}/budget${budget}`, terminal, {budget}, result => {
  header(result, budget === 2 ? 1 : 3, 2, PC + 4);
  fpValue(result, 0, 1);
  const calls = result.calls.filter(c => c.name === 'x87_resident_pair');
  assert.equal(calls.length, 1);
  assert.equal(BigInt(calls[0].result), envelope(0, 2, 1));
});
for (const row of [
  {name: 'TF', flags: 0x302, reason: 16, retired: 1, pc: PC + 2},
  {name: 'RF', flags: 0x10202, reason: 1, retired: 2, pc: PC + 4},
  {name: 'SS-shadow', shadow: 1, reason: 1, retired: 2, pc: PC + 4},
]) compare(row.name, programs.readRegister, {...row, pc: PC}, result => {
  header(result, row.reason, row.retired, row.pc, row.flags === 0x302 ? 0x302 : 0x202);
  assert.equal(result.calls.find(c => c.name === 'x87_resident_pair').args[8], 1,
    'original TF/RF/shadow permits only one native step');
  fpValue(result, 7, row.retired === 1 ? 3 : 6);
});
compare('cancel precedes pending MF and pair import', programs.registerRead,
  {cancel: 1, status: 1, control: 0x37e}, result => {
    header(result, 2, 0, PC); fpValue(result, 0, 1);
    assert.equal(result.calls.length, 0);
  });
compare('first waiting MF precedes RF clearing and unmapped second operand', programs.registerRead,
  {status: 1, control: 0x37e, flags: 0x10202, secondAddress: 0x6000}, result => {
    header(result, 12, 0, PC, 0x10202);
    assert.equal(result.arena.readUInt16LE(FP + 18), 0x8081);
    assert.equal(result.cr2, null); assert.equal(result.calls.filter(c => c.name === 'x87_resident_pair').length, 0);
  });
compare('second typed PF preserves first register mutation', programs.registerRead,
  {secondAddress: 0x6000}, result => {
    header(result, 23, 1, PC + 2); fpValue(result, 0, -1);
    assert.deepEqual([result.arena.readUInt32LE(80), result.arena.readUInt32LE(84),
      result.arena.readUInt32LE(88), result.arena.readUInt32LE(92)], [4, 0x6000, 1, 4]);
    assert.equal(result.arena.readUInt32LE(104), 0x1000f);
    assert.equal(result.cr2, 0x6000);
    const progress = BigInt(result.calls.find(c => c.name === 'x87_resident_pair').result);
    assert.equal(progress, envelope(23, 1, 1), 'certified second PF has one completed prefix and second packet');
  });
compare('second typed PF preserves first load and its FP pointer', programs.readRead,
  {secondAddress: 0x6000}, result => {
    header(result, 23, 1, PC + 2); fpValue(result, 7, 3);
    assert.equal(result.arena.readUInt32LE(FP + 24), PC);
    assert.equal(result.arena.readUInt32LE(FP + 28), RAM);
    assert.equal(result.cr2, 0x6000);
  });
compare('second legacy segment fault preserves first register mutation', programs.registerRead,
  {nullDs: true}, result => {
    header(result, 11, 1, PC + 2); fpValue(result, 0, -1);
    assert.equal(result.arena.readUInt32LE(104), 0x10007);
    assert.equal(result.arena.readUInt32LE(116), 3);
    assert.equal(result.arena.readUInt32LE(136), 4);
    assert.equal(result.cr2, null);
  });
compare('second CS limit fault follows committed first load', programs.readRegister,
  {csLimit: PC + 1}, result => {
    header(result, 11, 1, PC + 2); fpValue(result, 7, 3);
    assert.equal(result.arena.readUInt32LE(136), 4, 'first successful packet is retained at pre-helper stop');
    assert.equal(BigInt(result.calls.find(c => c.name === 'x87_resident_pair').result), envelope(0, 1));
  });
const deferred = {name: 'deferred-MF', code: [0xd8, 0xf1, 0xd9, 0x07, 0xff, 0xe2],
  steps: [reg(0xf1, 8 | 4 << 8 | 0x10 << 16), read(0x107)]};
compare('newly pending MF defers to original second waiting boundary', deferred,
  {control: 0x37b, zeroSecondStack: true, secondAddress: 0x6000}, result => {
    header(result, 12, 1, PC + 2); fpValue(result, 0, 1);
    assert.equal(result.arena.readUInt16LE(FP + 18), 0x8084);
    assert.equal(result.cr2, null, 'pending MF precedes the otherwise-unmapped second read');
    assert.equal(BigInt(result.calls.find(c => c.name === 'x87_resident_pair').result), envelope(0, 1));
  });
compare('cancel at deferred second boundary retains first result', programs.readRegister, {}, result => {
  header(result, 2, 1, PC + 2); fpValue(result, 7, 3);
}, {forceOne: true, cancelAfterFirst: true});

const live = {name: 'live-EA-after-GPR-write',
  code: [0xbb, 4, 0x40, 0, 0, 0xd9, 0x03, 0xd9, 0xe0, 0xff, 0xe2],
  steps: [read(0x103), reg(0x1e0, 15)]};
compare('live Wasm GPR address wins over stale arena register', live, {budget: 3}, result => {
  header(result, 1, 3, PC + 9); fpValue(result, 7, -5);
  assert.equal(result.arena.readUInt32LE(28), RAM + 4);
  assert.equal(result.calls.find(c => c.name === 'x87_resident_pair').args[5], RAM + 4);
});

// malformed progress is rejected before trusting a forged count for retirement.
for (const [name, reply, row = {}] of [
  ['reserved bit', 1n << 35n], ['completed3', envelope(0, 3)],
  ['reported second with completed0', envelope(0, 0, 1)],
  ['completed2 reports first', envelope(0, 2, 0)],
  ['success reports no completion', envelope(0, 0)],
  ['second success missing second completion', envelope(0, 1, 1)],
  ['two completions exceed debug limit', envelope(0, 2, 1), {flags: 0x302}],
]) {
  const c = make(programs.readRegister);
  try {
    c.seed(row); const before = c.arena();
    c.synthetic = () => ({envelope: reply, packet: packet(reply >> 34n & 1n ? 0 : 4, row.flags ?? 0x202)});
    const result = c.execute(2, {consume: false});
    header(result, 7, 0, PC, row.flags ?? 0x202);
    assert.equal(result.arena.readUInt32LE(80), 2, name);
    assert.deepEqual(result.arena.subarray(FP, FP + 128), before.subarray(FP, FP + 128));
    assert.equal(result.receipt, undefined);
    observations.push({label: `malformed ${name}`, reason: 7, retired: 0});
  } finally {c.close();}
}
{
  const c = make(programs.readRegister);
  try {
    c.seed(); c.synthetic = () => ({envelope: envelope(7, 0), packet: packet(4)});
    const result = c.execute(2, {consume: false});
    header(result, 7, 0, PC); assert.equal(result.arena.readUInt32LE(80), 3);
    observations.push({label: 'non-packet transport cannot accept stale success packet', reason: 7, retired: 0});
  } finally {c.close();}
}
{
  const c = make(programs.registerRead);
  try {
    c.seed({secondAddress: 0x6000});
    c.synthetic = args => {
      const first = programs.registerRead.steps[0];
      assert.equal(c.api.x87_resident_op(...args.slice(0, 4), first.word,
        args[5], args[7], args[4], first.opcode, first.control), 0);
      return {envelope: envelope(23, 1, 1),
        packet: pageFaultPacket({errorCode: 4, address: 0x6000, access: 1, length: 4})};
    };
    const result = c.execute(2, {consume: false});
    assert.notEqual(result.status, 0, 'a forged typed PF cannot acquire the native paging witness');
    assert.equal(result.receipt, undefined);
    assert.equal(result.cr2, null);
    fpValue(result, 0, -1);
    assert.equal(result.arena.readUInt32LE(48), PC + 2);
    assert.equal(result.arena.readUInt32LE(76), 1, 'only the real first operation is retired');
    const key = [0x58503837, 0x50414952];
    const stopped = c.arena();
    assert.notEqual(c.api.take_cpu_event(), 0, 'uncertified PF has no architectural CPU event');
    assert.deepEqual(c.arena(), stopped, 'rejected event take is neutral');
    c.ok('finish_execution_receipt', ...key,
      Number(result.token & 0xffffffffn), Number(result.token >> 32n));
    const receipt = c.arena().subarray(140, 204), epoch = receipt.readBigUInt64LE(16);
    assert.deepEqual(receipt.subarray(0, 16), Buffer.from('52334552010001004000000000000000', 'hex'));
    assert.deepEqual(receipt.subarray(52), Buffer.alloc(12));
    assert.ok(epoch > 0n);
    assert.equal(receipt.readBigUInt64LE(24), c.id);
    assert.deepEqual([32, 36, 40, 44, 48].map(at => receipt.readUInt32LE(at)), [0, 1, 1, 23, 1],
      'active kind1 receipt accounts ownership and prefix, not a certified exception');
    const afterReceipt = c.arena();
    assert.notEqual(c.api.dispatch_resident_windows_exception(...key,
      Number(c.id & 0xffffffffn), Number(c.id >> 32n),
      Number(epoch & 0xffffffffn), Number(epoch >> 32n)), 0,
    'an accounting receipt cannot dispatch the uncertified PF');
    assert.deepEqual(c.arena(), afterReceipt, 'rejected exception dispatch is neutral');
    assert.notEqual(c.api.take_cpu_event(), 0);
    observations.push({label: 'forged second typed PF lacks native witness', status: result.status, retired: 1, receiptKind: 1});
  } finally {c.close();}
}
// replacement modules use the old scalar imports; the shared planner also has an unbound negative.
for (const program of [programs.readRegister, programs.register]) {
  const c = make(program, {resident: false});
  try {
    assert.ok(!c.imports.some(i => i.name === 'x87_resident_pair'));
    c.seed(); const result = c.execute(2);
    header(result, 1, 2, PC + 4);
    assert.equal(result.calls.filter(c => c.name === 'x87_op').length, 2);
    fpValue(result, program === programs.register ? 0 : 7, program === programs.register ? 1 : 6);
    c.checkMemory(); observations.push({label: `${program.name} replacement fallback`, reason: 1, retired: 2});
  } finally {c.close();}
}
{
  const c = make(programs.register);
  try {
    const module = new WebAssembly.Module(readFileSync(join(output, 'unbound-control.wasm')));
    assert.ok(WebAssembly.Module.imports(module).every(i => i.module === 'env' && i.name === 'memory'),
      'unbound inline control module has no pair or scalar native helper dependency');
    const run = new WebAssembly.Instance(module, {env: {memory: c.memory}}).exports.run;
    for (const budget of [2, 3]) {
      c.seed();
      assert.equal(run(c.base, c.base + 56, budget, c.base + 96), 0);
      const result = {status: 0, arena: c.arena()}, reason = budget === 2 ? 1 : 3;
      header(result, reason, 2, PC + 4);
      assert.equal(result.arena.readUInt16LE(FP + 16), 0x37f);
      assert.equal(result.arena.readUInt16LE(FP + 18), 0);
      assert.equal(result.arena.readUInt16LE(FP + 20), 0xffff);
      fpValue(result, 0, 1);
      c.checkMemory(); observations.push({label: `unbound inline control fallback/budget${budget}`, reason, retired: 2});
    }
  } finally {c.close();}
}
verifyFinalStores(make, observations);
verifyGroups(make, observations);

writeFileSync(join(output, 'result.json'), JSON.stringify({status: 'PASS',
  engine: {path: enginePath, sha256: engine.sha256, bytes: engine.bytes.length},
  modules: [...modules.values()], observations, claims: [
    'real generated resident pair import vs real scalar-helper relay on identical generated modules',
    'all original instruction entries, finite budgets, debug controls, fault priority and active receipt ownership',
    'full nontransfer arena and all mapped guest code/data bytes; literal independent values and fault geometry',
    'synthetic envelope controls test protocol acceptance only, not native arithmetic or numeric performance',
    'no timing or game support claim',
  ]}, null, 2) + '\n');
console.log(JSON.stringify({status: 'PASS', runs: observations.length, modules: modules.size, output}));
