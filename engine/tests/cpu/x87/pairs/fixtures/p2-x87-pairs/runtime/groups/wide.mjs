import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {PC, FP, RAM, defined, packet, wideEnvelope} from '../support.mjs';
import {program, numeric, stop, memory} from '../groups.mjs';

const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const data = [{offset: 0, value: 1.234567}, {offset: 4, value: 0.7654321}];

function catalog(source) {
  const length = source.steps.length;
  source.groups = [];
  for (let first = 0; first < length; first += 8) {
    const count = Math.min(8, length - first);
    if (count >= 4) source.groups.push({first, length: count});
  }
  source.wideOrdinal = source.groups.length;
  source.wideGroups = {[source.wideOrdinal]: {first: 0, length}};
  return source;
}

function mixed(length, store = false) {
  const cycle = ['mul', 'add', 'div', 'twice'];
  const names = ['load', ...Array.from({length: length - 1}, (_, index) => cycle[index % cycle.length])];
  if (store) names[length - 1] = 'store';
  return catalog(program(`wide-${store ? 'store' : 'mixed'}${length}`, names, data));
}

function calls(result) { return result.calls.filter(call => call.name === 'x87_resident_group32'); }

export function verifyWideGroups(make, observations) {
  function compare(label, source, row, verify, {writes = [], forceOne = false, cancelAfterFirst = false} = {}) {
    const scalar = make(source, {scalar: true, wide: true}), grouped = make(source, {wide: true});
    try {
      assert.deepEqual(grouped.bytes, scalar.bytes, 'scalar relay uses the identical generated wide module');
      assert.ok(grouped.imports.some(item => item.name === 'x87_resident_group32'));
      assert.ok(grouped.imports.some(item => item.name === 'x87_resident_group'), 'legacy interior-entry wire remains present');
      scalar.seed(row); grouped.seed(row);
      scalar.forceOne = grouped.forceOne = forceOne;
      scalar.cancelAfterFirst = grouped.cancelAfterFirst = cancelAfterFirst;
      const a = scalar.execute(row.budget ?? source.steps.length), b = grouped.execute(row.budget ?? source.steps.length);
      assert.deepEqual(defined(b.arena), defined(a.arena), `${label}: full architectural/helper/exit arena`);
      assert.deepEqual(b.receipt, a.receipt, `${label}: exact aggregate and child receipt`);
      assert.equal(b.cr2, a.cr2); verify(b);
      memory(scalar, writes); memory(grouped, writes);
      observations.push({label, reason: b.arena.readUInt32LE(72), retired: b.arena.readUInt32LE(76),
        arena_sha256: hash(defined(b.arena)), wideCalls: calls(b)});
    } finally {scalar.close(); grouped.close();}
  }

  for (const length of [9, 16, 31, 32]) {
    const source = mixed(length);
    for (const control of [0x003f, 0x023f, 0x037f]) {
      const budgets = [...new Set([0, 1, 2, 8, 9, length - 1, length, length + 1])].filter(value => value <= length + 1);
      for (const budget of budgets) compare(`${source.name}/CW${control}/budget${budget}`, source, {control, budget}, result => {
        const count = Math.min(budget, length);
        stop(result, 1, Math.min(budget, length + 1), budget > length ? 0x1100 : PC + count * 2);
        const wanted = numeric(source, count, control);
        assert.deepEqual(result.arena.subarray(FP + (count ? 110 : 40), FP + (count ? 120 : 50)), wanted.raw,
          'independent pc24/53/64 rounded full80 value');
        assert.equal(result.arena.readUInt16LE(FP + 18), wanted.status, 'literal sticky PE/C1/TOP');
        assert.equal(result.arena.readUInt16LE(FP + 20), wanted.tag);
        const wide = calls(result);
        assert.equal(wide.length, budget ? 1 : 0);
        if (wide.length) {
          assert.equal(wide[0].args.length, 39);
          assert.equal(wide[0].args[4], source.wideOrdinal, 'wide ordinal follows the complete legacy catalog');
          assert.equal(wide[0].args[38], count);
          assert.deepEqual(wide[0].args.slice(5 + length, 37), Array(32 - length).fill(0), 'unused EA arguments are zero');
          assert.equal(BigInt(wide[0].result), wideEnvelope(0, count, count - 1));
        }
      });
    }
  }

  const source = mixed(32);
  for (let index = 1; index < 32; index++) compare(`wide side-entry ${index}`, source,
    {pc: PC + index * 2, budget: 1}, result => {
      stop(result, 1, 1, PC + (index + 1) * 2);
      assert.equal(calls(result).length, 0, 'every original interior instruction retains its original body');
    });
  for (const row of [{flags: 0x302}, {flags: 0x10202}, {shadow: 1}]) compare('wide original debug state captures max1',
    source, {...row, budget: 32}, result => {
      const trap = row.flags === 0x302;
      stop(result, trap ? 16 : 1, trap ? 1 : 32, PC + (trap ? 2 : 64), trap ? 0x302 : 0x202);
      assert.equal(calls(result)[0].args[38], 1);
    });
  compare('wide initial cancel precedes pending mf', source, {cancel: 1, status: 1, control: 0x37e}, result => {
    stop(result, 2, 0, PC); assert.equal(result.calls.length, 0);
  });
  compare('wide first waiting mf precedes rf clearing and operand access', source,
    {status: 1, control: 0x37e, flags: 0x10202, nullDs: true}, result => {
      stop(result, 12, 0, PC, 0x10202); assert.equal(result.cr2, null);
      assert.equal(result.arena.readUInt16LE(FP + 18), 0x8081);
      assert.deepEqual(result.calls.map(call => call.name), ['x87_resident_op']);
    });
  compare('wide cancellation after first completion reaches original boundary', source, {}, result => {
    stop(result, 2, 1, PC + 2);
    assert.equal(BigInt(calls(result)[0].result), wideEnvelope(0, 1, 0));
  }, {forceOne: true, cancelAfterFirst: true});
  compare('wide cs limit preserves prefix beyond legacy group width', source, {csLimit: PC + 17}, result => {
    stop(result, 11, 9, PC + 18);
    assert.equal(BigInt(calls(result)[0].result), wideEnvelope(0, 9, 8));
  });

  for (const length of [9, 32]) {
    const late = catalog(program(`wide-latePF${length}`, ['load', ...Array(length - 2).fill('twice'), 'mul'], data));
    compare(`wide later operand PF at ${length - 1}`, late, {secondAddress: 0x6000}, result => {
      stop(result, 23, length - 1, PC + (length - 1) * 2);
      assert.deepEqual([80, 84, 88, 92].map(at => result.arena.readUInt32LE(at)), [4, 0x6000, 1, 4]);
      assert.equal(result.cr2, 0x6000);
      assert.deepEqual(result.arena.subarray(FP + 110, FP + 120), numeric(late, length - 1, 0x37f).raw);
      assert.equal(BigInt(calls(result)[0].result), wideEnvelope(23, length - 1, length - 1));
    });
  }
  const deferred = catalog(program('wide-deferred-mf', ['load', ...Array(31).fill('neg')], data));
  deferred.code.splice(0, 4, 0xd8, 0xf1, 0xd9, 0x07);
  deferred.steps[0] = {pc: PC, opcode: 0xf1, word: 8 | 4 << 8 | 0x10 << 16, control: 1};
  deferred.steps[1] = {pc: PC + 2, opcode: 0x107, word: 3, control: 0x401};
  compare('wide new pending mf defers before next unmapped operand', deferred,
    {control: 0x37b, zeroSecondStack: true, secondAddress: 0x6000}, result => {
      stop(result, 12, 1, PC + 2); assert.equal(result.cr2, null);
      assert.equal(result.arena.readUInt16LE(FP + 18), 0x8084);
      assert.equal(BigInt(calls(result)[0].result), wideEnvelope(0, 1, 0));
    });

  const store = mixed(32, true);
  for (const control of [0x003f, 0x023f, 0x037f]) {
    const wanted = numeric(store, 32, control);
    compare(`wide terminal store/CW${control}`, store, {control, budget: 32}, result => {
      stop(result, 1, 32, PC + 64);
      assert.equal(result.arena.readUInt16LE(FP + 18), wanted.status);
      assert.equal(result.arena.readUInt16LE(FP + 20), wanted.tag);
      assert.equal(BigInt(calls(result)[0].result), wideEnvelope(0, 32, 31));
    }, {writes: [[RAM + 4, wanted.storedBits]]});
  }
  const sourceWrite = catalog(program('wide-terminal-source-write',
    ['load', ...Array.from({length: 30}, (_, index) => index % 2 ? 'add' : 'twice'), 'store'], data));
  const smc = numeric(sourceWrite, 32, 0x023f);
  compare('wide terminal source write retires all 32 before invalidation', sourceWrite,
    {control: 0x023f, secondAddress: PC, budget: 32}, result => {
      stop(result, 6, 32, PC + 64);
      assert.equal(result.arena.readUInt16LE(FP + 18), smc.status);
      assert.equal(result.arena.readUInt16LE(FP + 20), smc.tag);
      assert.equal(BigInt(calls(result)[0].result), wideEnvelope(11, 32, 31));
    }, {writes: [[PC, smc.storedBits]]});
  const writeFault = catalog(program('wide-terminal-writePF', ['load', ...Array(30).fill('twice'), 'store'], data));
  compare('wide terminal write PF retains 31 completed operations and no pop', writeFault, {secondAddress: 0x6000}, result => {
    stop(result, 23, 31, PC + 62); assert.equal(result.cr2, 0x6000);
    assert.deepEqual([80, 84, 88, 92].map(at => result.arena.readUInt32LE(at)), [6, 0x6000, 2, 4]);
    assert.equal(result.arena.readUInt16LE(FP + 20), 0x3ffc);
    assert.equal(BigInt(calls(result)[0].result), wideEnvelope(23, 31, 31));
  });
  const terminal = {...mixed(9), name: 'wide-terminal9', captureInstructions: 9}; terminal.code = terminal.code.slice(0, 18);
  for (const budget of [9, 10]) compare(`wide terminal budget${budget}`, terminal, {budget}, result => {
    stop(result, budget === 9 ? 1 : 3, 9, PC + 18);
    assert.equal(BigInt(calls(result)[0].result), wideEnvelope(0, 9, 8));
  });

  for (const [label, progress, alter, row = {}, retired = 0] of [
    ['reserved high bit', wideEnvelope(0, 32, 31) | 1n << 43n],
    ['count exceeds static maximum', wideEnvelope(0, 33, 31)],
    ['success skips reported step', wideEnvelope(0, 9, 7)],
    ['failed packet claims completion', wideEnvelope(0, 9, 8), bytes => bytes.writeUInt32LE(1, 16)],
    ['original debug max exceeded', wideEnvelope(0, 32, 31), undefined, {flags: 0x302}],
    ['nonstore SMC', wideEnvelope(11, 32, 31)],
    ['malformed last packet', wideEnvelope(0, 32, 31), bytes => bytes.writeUInt32LE(39, 8), {}, 31],
    ['malformed middle packet', wideEnvelope(0, 9, 8), bytes => bytes.writeUInt32LE(39, 8), {}, 8],
    ['nonpacket stale success', wideEnvelope(7, 9, 9), undefined, {}, 9],
  ]) {
    const c = make(source, {wide: true});
    try {
      c.seed(row); const before = c.arena();
      c.wideSynthetic = () => {
        const reported = Number(progress >> 38n & 31n), step = source.steps[reported];
        const bytes = packet(step.control >>> 8, row.flags ?? 0x202);
        alter?.(bytes); return {envelope: progress, packet: bytes};
      };
      const result = c.execute(32, {consume: false});
      stop(result, 7, retired, PC + retired * 2, row.flags ?? 0x202);
      assert.deepEqual(result.arena.subarray(FP, FP + 128), before.subarray(FP, FP + 128));
      assert.equal(result.receipt, undefined); memory(c);
      observations.push({label: `wide malformed ${label}`, reason: 7, retired});
    } finally {c.close();}
  }
  for (const ordinal of [0, 0xffffffff]) {
    const c = make(source, {wide: true});
    try {
      c.seed(); const before = c.arena(); c.wideOrdinal = ordinal;
      const result = c.execute(32, {consume: false}); stop(result, 7, 0, PC);
      assert.deepEqual(result.arena.subarray(FP, FP + 128), before.subarray(FP, FP + 128));
      memory(c); observations.push({label: `wide wrong-kind/out-of-range ordinal ${ordinal}`, reason: 7, retired: 0});
    } finally {c.close();}
  }
}
