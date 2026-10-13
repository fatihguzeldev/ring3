import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {harness, PC, SEG, BASES, words, record, state, registers, exit, descriptor, hash, writeRAM} from '../../../../base/fixtures/p2-cs-base/runtime/support.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'explicit engine, output and root');
const paths = ['engine/src/abi/x86/segments/modes.rs', 'engine/src/cpu/x86/decode/modes.rs',
  'engine/src/cpu/x86/decode/strings/defaults.rs', 'engine/src/cpu/x86/segments/transfers/tests.rs',
  'engine/src/process/calls/call/modes.rs', 'engine/src/cpu/x86/decode/lower.rs',
  'engine/src/cpu/dbt/wasm/strings.rs', 'engine/src/cpu/dbt/wasm/integer/dispatch.rs',
  'engine/src/cpu/dbt/wasm/segments.rs', 'engine/src/process/cpu/segments.rs',
  'engine/src/cpu/x86/decode/strings.rs', 'engine/src/cpu/x86/decode/operands.rs',
  'engine/src/cpu/x86/decode/flow.rs', 'engine/src/cpu/x86/segments/cache.rs', 'engine/src/cpu/x86/segments/transfers.rs',
  'engine/src/process/calls/call.rs', 'engine/tests/cpu/segments/code/defaults/wasm.rs',
  'engine/tests/cpu/segments/code/defaults/fixtures/p2-code-defaults/runtime/run.mjs'];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), h = await harness(enginePath, output, root);
const call = (name, args, status = 0) => ({name, args: args.map(value => value >>> 0), status});
const helper = (c, name) => c.resident ? `${name}_resident` : name;
const bound = (c, material, args) => [...(c.resident ? material.binding : []), ...args];
const defaultBases = [0, 0x4000, 0, 0x6000, 0, 0];
function mode(bytes, {default32 = false, bases = defaultBases, selector = 0x1b, limit} = {}) {
  const sg = record('R3SG', 64, [], 3);
  [0x23, selector, 0x23, 0x23, 0x3b, 0].forEach((value, index) => sg.writeUInt16LE(value, 16 + 2 * index));
  const cs = limit === undefined ? (default32 ? 0xf3bfffff : 0xf1bfffff) : ((default32 ? 0xf2b00000 : 0xf0b00000) | limit);
  [0xf33fffff, cs, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0].forEach((value, index) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][index]));
  bytes.set(sg, SEG); bytes.set(record('R3SB', 40, bases), BASES); return bytes;
}
function seed(options = {}) {return mode(state(h.initial, options), options);}
function complete(input, pc, changes = {}, version = 1) {
  const want = Buffer.from(input); want.writeUInt32LE(pc, 48); want.writeUInt32LE(2, 52);
  for (const [index, value] of Object.entries(changes)) want.writeUInt32LE(value >>> 0, 16 + Number(index) * 4);
  return exit(want, 1, 1, version);
}
function scalarPacket(want, width, value, store = false) {
  want.set(record('R3MH', 40, [0, store ? 0 : value, 0, 0, 0, width], 10), 100);
}
function compile(c, name, code, input, helpers = [], pc = PC, linear = 0x5000) {
  h.upload(c, linear, code); h.put(c, 0, input); return h.compile(c, name, [[pc, code.length]], helpers);
}

for (const resident of [false, true]) for (const entries of [false, true]) {
  {
    const c = h.open(resident, entries, 'D16 operand default and per-instruction66', [[0x5000]]), regs = [...registers]; regs[0] = 0xabcd0000;
    const input = seed({regs}), code = Buffer.from('b8341266b878563412b8efbeebfe', 'hex');
    if (compile(c, 'D16 word dword word body', code, input)) {
      let next = input;
      for (const [pc, eax] of [[0x1003, 0xabcd1234], [0x1009, 0x12345678], [0x100c, 0x1234beef]]) {
        const want = complete(next, pc, {0: eax}); h.observe(c, '66 changes this operand only', {input: next, want}); next = want;
      }
    } else h.skip(c, 'D16 operand sequence', 'D16 word dword word body'); h.host(c, 'close');
  }
  {
    const c = h.open(resident, entries, 'independent66 operand and67 address defaults', [[0x5000], [0x6000, 3], [0xb000, 3]]), regs = [...registers];
    [regs[0], regs[2], regs[3], regs[6]] = [0x5000, 0x33330000, 0xabcd0020, 0x12340004];
    h.upload(c, 0x6024, words([0x11223344])); h.upload(c, 0xb000, words([0x55667788]));
    const input = seed({regs}), code = Buffer.from('8b10668b10678b1066678b10ebfe', 'hex');
    if (compile(c, 'D16 all four operand-address combinations', code, input, ['cpu_read16', 'cpu_read32'])) {
      let next = input;
      for (const [pc, width, address, value, edx] of [[0x1002, 2, 0x6024, 0x3344, 0x33333344], [0x1005, 4, 0x6024, 0x11223344, 0x11223344],
        [0x1008, 2, 0xb000, 0x7788, 0x11227788], [0x100c, 4, 0xb000, 0x55667788, 0x55667788]]) {
        const want = complete(next, pc, {2: edx}, 6); scalarPacket(want, width, value);
        h.observe(c, 'operand width and effective-address width are independent', {input: next, want, helpers: [call(`cpu_read${width * 8}`, [address])]}); next = want;
      }
    } else h.skip(c, 'D16 independent size execution', 'D16 all four operand-address combinations'); h.host(c, 'close');
  }
  {
    const c = h.open(resident, entries, 'D16 moffs and XLAT implicit bases', [[0x5000], [0x6000, 3], [0xaaaa6000, 3]]), regs = [...registers];
    regs[0] = 0xdead0000; regs[3] = 0xaaaa0040;
    h.upload(c, 0x6020, Buffer.from('0312', 'hex')); h.upload(c, 0x6030, Buffer.from('0356', 'hex'));
    h.upload(c, 0x6043, Buffer.from([5])); h.upload(c, 0xaaaa6045, Buffer.from([7]));
    const input = seed({regs}), code = Buffer.from('a1200067a130000000d767d7ebfe', 'hex');
    if (compile(c, 'D16 moffs16 moffs32 XLAT16 XLAT32', code, input, ['cpu_read16', 'cpu_read8'])) {
      let next = input;
      for (const [pc, width, address, value, eax] of [[0x1003, 2, 0x6020, 0x1203, 0xdead1203], [0x1009, 2, 0x6030, 0x5603, 0xdead5603],
        [0x100a, 1, 0x6043, 5, 0xdead5605], [0x100c, 1, 0xaaaa6045, 7, 0xdead5607]]) {
        const want = complete(next, pc, {0: eax}, 6); scalarPacket(want, width, value);
        h.observe(c, 'moffs and XLAT honor decoded address size', {input: next, want, helpers: [call(`cpu_read${width * 8}`, [address])]}); next = want;
      }
    } else h.skip(c, 'D16 moffs XLAT execution', 'D16 moffs16 moffs32 XLAT16 XLAT32'); h.host(c, 'close');
  }
  {
    const c = h.open(resident, entries, 'D16 zero-base mode survives loaded DS publication', [[PC], [0x6000, 3]]), regs = [...registers];
    regs[0] = 0x43; regs[3] = 0xabcd0020;
    h.upload(c, 0x6020, Buffer.from('3412', 'hex'));
    h.host(c, 'configure_segment_descriptor_raw', [0x43, 0x600000ff, 0x0040f200]);
    const input = seed({regs, bases: [0, 0, 0, 0, 0, 0]}), load = helper(c, 'segment_load');
    const material = compile(c, 'D16 MOV DS AX followed by word data load', Buffer.from('8ed88b17ebfe', 'hex'), input, [load, 'cpu_read16'], PC, PC);
    if (material) {
      const first = complete(input, 0x1002, {}, 6);
      first.writeUInt16LE(0x43, SEG + 22); first.writeUInt32LE(0xf23000ff, SEG + 40); first.writeUInt32LE(0x6000, BASES + 28);
      first.set(record('R3MH', 40, [0, 0x6000, 0, 0, 0, 0], 7), 100);
      h.observe(c, 'segment helper keeps current-v3 D16 with CS base zero', {input, want: first,
        helpers: [call(load, bound(c, material, [3, 0x43]))]});
      const second = complete(first, 0x1004, {2: 0x34561234}, 6); scalarPacket(second, 2, 0x1234);
      h.observe(c, 'retained next word instruction uses newly loaded DS authority', {input: first, want: second, helpers: [call('cpu_read16', [0x6020])]});
    } else h.skip(c, 'D16 loaded DS continuation', 'D16 MOV DS AX followed by word data load'); h.host(c, 'close');
  }
  for (const address32 of [false, true]) {
    const c = h.open(resident, entries, 'D16 REP word pointer and count width', [[0x5000], [0x6000, 3], [0x7000, 3], [0xb000, 3]]), regs = [...registers];
    [regs[1], regs[6], regs[7]] = address32 ? [1, 0x1020, 0x2040] : [0xabcd0001, 0x12340020, 0x56780040];
    const bases = [address32 ? 0x9000 : 0x7000, 0x4000, 0, 0x6000, 0, 0], input = seed({regs, bases});
    const source = address32 ? 0x7020 : 0x6020, destination = address32 ? 0xb040 : 0x7040;
    h.upload(c, source, Buffer.from('3412', 'hex')); h.upload(c, destination, Buffer.from('aaaa', 'hex'));
    const code = Buffer.from(address32 ? '67f3a5ebfe' : 'f3a5ebfe', 'hex'), store = resident ? 'cpu_store_resident16' : 'cpu_store16';
    const material = compile(c, 'D16 REP MOVSW decoded pointer width', code, input, ['cpu_read16', store]);
    if (material) {
      const want = complete(input, address32 ? 0x1003 : 0x1002,
        {1: address32 ? 0 : 0xabcd0000, 6: address32 ? 0x1022 : 0x12340022, 7: address32 ? 0x2042 : 0x56780042}, 6);
      scalarPacket(want, 2, 0, true); want.writeUInt32LE(1, 4604);
      const wantRAM = h.ram(c); writeRAM(wantRAM, destination, Buffer.from('3412', 'hex'));
      h.observe(c, 'REP advances active count and indices with inactive halves preserved', {input, want, wantRAM,
        helpers: [call('cpu_read16', [source]), call(store, bound(c, material, [destination, 0x1234]))]});
      if (!address32) {
        const zeroRegs = [...regs]; zeroRegs[1] = 0xabcd0000; zeroRegs[6] = 0xffff0000; zeroRegs[7] = 0xffff0000;
        const zero = seed({regs: zeroRegs, bases}), zeroWant = complete(zero, 0x1002, {}, 6); zeroWant.writeUInt32LE(1, 4604);
        h.observe(c, 'zero CX ignores high ECX and touches no guest address', {input: zero, want: zeroWant, kind: 'control'});
      }
    } else h.skip(c, 'D16 REP execution', 'D16 REP MOVSW decoded pointer width'); h.host(c, 'close');
  }
  {
    const c = h.open(resident, entries, 'D16 stack operand width with independent B32 pointer', [[0x5000], [0x7000, 3]]), input = seed();
    h.upload(c, 0x7000, Buffer.alloc(4096, 0x6b));
    const store16 = resident ? 'cpu_store_resident16' : 'cpu_store16', store32 = resident ? 'cpu_store_resident32' : 'cpu_store32';
    const material = compile(c, 'D16 PUSH POP word dword body', Buffer.from('505866506658ebfe', 'hex'), input, [store16, store32, 'cpu_read16', 'cpu_read32']);
    if (material) {
      let next = input, ram = h.ram(c);
      for (const [pc, width, store, address, esp, value] of [[0x1001, 2, true, 0x77fe, 0x77fe, 0x5678], [0x1002, 2, false, 0x77fe, 0x7800, 0x5678],
        [0x1004, 4, true, 0x77fc, 0x77fc, 0x12345678], [0x1006, 4, false, 0x77fc, 0x7800, 0x12345678]]) {
        const want = complete(next, pc, {4: esp}, 6); scalarPacket(want, width, value, store);
        if (store) writeRAM(ram, address, words([value]).subarray(0, width));
        const name = store ? width === 2 ? store16 : store32 : `cpu_read${width * 8}`;
        h.observe(c, 'stack value width changes while SS.B32 remains independent', {input: next, want, wantRAM: ram,
          helpers: [call(name, store ? bound(c, material, [address, value]) : [address])]}); next = want;
      }
    } else h.skip(c, 'D16 stack execution', 'D16 PUSH POP word dword body'); h.host(c, 'close');
  }
  for (const word of [true, false]) {
    const c = h.open(resident, entries, 'D16 near CALL RET operand-selected frame', [[0x5000], [0x6000], [0x7000, 3]]);
    const code = Buffer.from(word ? 'e8fd0f' : '66e8fa0f0000', 'hex'), ret = Buffer.from(word ? 'c3' : '66c3', 'hex');
    h.upload(c, 0x5000, code); h.upload(c, 0x6000, ret); h.upload(c, 0x7000, Buffer.alloc(4096, 0x6b));
    const input = seed(), store = resident ? `cpu_store_resident${word ? 16 : 32}` : `cpu_store${word ? 16 : 32}`, read = `cpu_read${word ? 16 : 32}`;
    h.put(c, 0, input); const material = h.compile(c, 'D16 near call and return two-block capture', [[PC, code.length], [0x2000, ret.length]], [store, read]);
    if (material) {
      const returned = PC + code.length, width = word ? 2 : 4, address = 0x7800 - width;
      const want = exit(Buffer.from(input), 3, 2, 6); want.writeUInt32LE(returned, 48); want.writeUInt32LE(2, 52); scalarPacket(want, width, returned);
      const wantRAM = h.ram(c); writeRAM(wantRAM, address, words([returned]).subarray(0, width));
      h.observe(c, 'near CALL RET uses operand width and commits each transaction once', {input, want, wantRAM, budget: 3,
        helpers: [call(store, bound(c, material, [address, returned])), call(read, [address])]});
    } else h.skip(c, 'D16 near roundtrip', 'D16 near call and return two-block capture'); h.host(c, 'close');
  }
  for (const [codeHex, ecx, expectedEcx, pc, name] of [
    ['e2fe', 0x10001, 0x10000, 0x1002, 'LOOP default CX stops'], ['67e2fd', 0x10001, 0x10000, PC, 'LOOP67 full ECX takes'],
    ['e3fe', 0xabcd0000, 0xabcd0000, PC, 'JCXZ default ignores high ECX'], ['67e3fd', 0xabcd0000, 0xabcd0000, 0x1003, 'JECXZ67 sees high ECX'],
  ]) {
    const c = h.open(resident, entries, name, [[0x5000]]), regs = [...registers]; regs[1] = ecx;
    const input = seed({regs}), code = Buffer.from(codeHex, 'hex');
    if (compile(c, name + ' compilation', code, input)) h.observe(c, name, {input, want: complete(input, pc, {1: expectedEcx})});
    else h.skip(c, name, name + ' compilation'); h.host(c, 'close');
  }
  {
    const c = h.open(resident, entries, 'same logical PC and base with distinct D key', [[PC]]);
    h.upload(c, PC, Buffer.from('b834129090ebfe', 'hex')); const flat = state(h.initial); h.put(c, 0, flat);
    const old = h.compile(c, 'same-byte retained D32 unit', [[PC, 7]]);
    if (old) {
      const legacy1 = state(h.initial); legacy1.fill(0xa5, BASES, BASES + 40);
      h.observe(c, 'legacy v1 keeps D32 and ignores companion junk', {input: legacy1, want: complete(legacy1, 0x1005, {0: 0x90901234}), kind: 'control'});
      const legacy2 = seed({default32: true, bases: [0, 0, 0, 0, 0, 0]}); legacy2.writeUInt16LE(2, SEG + 4); legacy2.fill(0xa5, BASES, BASES + 40);
      h.observe(c, 'legacy v2 keeps D32 and ignores companion junk', {input: legacy2, want: complete(legacy2, 0x1005, {0: 0x90901234}), kind: 'control'});
      const forbidden = seed({bases: [0, 0, 0, 0, 0, 0]}); forbidden.writeUInt16LE(2, SEG + 4); forbidden.fill(0xa5, BASES, BASES + 40);
      const refusal = h.observe(c, 'legacy v2 refuses D16 neutrally', {input: forbidden, want: forbidden, status: 2, kind: 'control'});
      if (refusal.passed) refusal.classification = 'expected-legacy-refusal';
      const input = seed({bases: [0, 0, 0, 0, 0, 0], limit: 0x1002}), want = exit(Buffer.from(input), 3, 0);
      const observed = h.observe(c, 'wrong D key checks only first byte and preserves RF', {input, want});
      if (observed.passed) {
        h.put(c, 0, want); const replacement = h.compile(c, 'D16 same-PC stopped repair', [], [], {stopped: true});
        if (replacement) {
          h.observe(c, 'D16 repaired material uses word length and preserves high EAX', {input, want: complete(input, 0x1003, {0: 0x12341234})});
          if (resident) {
            h.lookup(c, 'D16 same-PC lookup selects D16 unit', input, PC, replacement);
            h.lookup(c, 'D32 same-PC lookup selects original unit', flat, PC, old);
          }
        } else h.skip(c, 'D16 key repaired execution', 'D16 same-PC stopped repair');
      } else h.skip(c, 'D16 stopped repair and lookup', 'wrong D key NeedCode');
    } else h.skip(c, 'D16 live key switch', 'same-byte retained D32 unit'); h.host(c, 'close');
  }
  for (const kind of ['jump', 'call', 'return', 'iret']) {
    const c = h.open(resident, entries, `D32 ${kind} loads D16 at same base`, [[PC], [0x2000], [0x7000, 3]]);
    const code = kind === 'jump' ? Buffer.from('ea002000004300', 'hex') : kind === 'call' ? Buffer.from('9a002000004300', 'hex') : Buffer.from([kind === 'return' ? 0xcb : 0xcf]);
    h.upload(c, PC, code); h.upload(c, 0x2000, Buffer.from('b834129090ebfe', 'hex')); h.upload(c, 0x7000, Buffer.alloc(4096, 0x6b));
    if (kind === 'return' || kind === 'iret') h.upload(c, 0x7800, words([0x2000, 0xa5a50043, ...(kind === 'iret' ? [0x10002] : [])]));
    const raw = descriptor(0); h.host(c, 'configure_segment_descriptor_raw', [0x43, raw.low, raw.high - 0x400000]);
    const name = helper(c, kind === 'jump' ? 'far_jump' : kind === 'call' ? 'far_call' : 'far_return'), input = state(h.initial);
    h.put(c, 0, input); const material = h.compile(c, `D32 ${kind} retains same-base old-D target`, [[PC, code.length], [0x2000, 7]], [...(kind === 'return' || kind === 'iret' ? ['cpu_read'] : []), name]);
    if (material) {
      const want = exit(mode(Buffer.from(input), {bases: [0, 0, 0, 0, 0, 0], selector: 0x43}), 3, 1, 6);
      want.writeUInt32LE(0x2000, 48); want.writeUInt32LE(kind === 'iret' ? 0x10002 : 2, 52); want.set(record('R3MH', 40, [0, 0x2000, 0, 0, 0, 0], 7), 100);
      const helpers = [], wantRAM = h.ram(c);
      if (kind === 'call') {want.writeUInt32LE(0x77f8, 32); writeRAM(wantRAM, 0x77f8, words([0x1007, 0x1b]));}
      if (kind === 'return' || kind === 'iret') {
        for (const offset of kind === 'iret' ? [0, 4, 8] : [0, 4]) helpers.push(call('cpu_read', [0x7800 + offset, 4, 1]));
        want.writeUInt32LE(kind === 'iret' ? 0x780c : 0x7808, 32); want.set(words([kind === 'iret' ? 0x10002 : 0xa5a50043]), 140);
      }
      helpers.push(call(name, bound(c, material, [0x2000, 0x43, 0, ...(kind === 'call' ? [0x1007, 0x7800] : [])])));
      const observed = h.observe(c, `D32 ${kind} retires once before D-key NeedCode`, {input, want, wantRAM, budget: 2, helpers});
      if (observed.passed) {
        h.put(c, 0, want); const repaired = h.compile(c, `D16 ${kind} destination stopped capture`, [], [], {stopped: true});
        if (repaired) h.observe(c, `D16 ${kind} destination uses AX and three-byte length`, {input: want, want: complete(want, 0x2003, {0: 0x12341234})});
        else h.skip(c, `D16 ${kind} destination execution`, 'D16 destination capture');
      } else h.skip(c, `D16 ${kind} stopped repair`, 'source transfer and D-key NeedCode');
    } else h.skip(c, `D32 ${kind} execution`, 'D32 source compilation'); h.host(c, 'close');
  }
  {
    const c = h.open(resident, entries, 'D16 word far JMP loads D32 with unchanged nonflat base', [[0x5000], [0x7000]]);
    h.upload(c, 0x5000, Buffer.from('ea00304300', 'hex')); h.upload(c, 0x7000, Buffer.from('b878569090ebfe', 'hex'));
    const raw = descriptor(0x4000); h.host(c, 'configure_segment_descriptor_raw', [0x43, raw.low, raw.high]);
    const input = seed(), name = helper(c, 'far_jump'); h.put(c, 0, input);
    const material = h.compile(c, 'D16 source and retained D16 target', [[PC, 5], [0x3000, 7]], [name]);
    if (material) {
      const want = exit(mode(Buffer.from(input), {default32: true, selector: 0x43}), 3, 1, 6);
      want.writeUInt32LE(0x3000, 48); want.writeUInt32LE(2, 52); want.set(record('R3MH', 40, [0, 0x3000, 0, 0, 0, 0], 7), 100);
      const observed = h.observe(c, 'D16 far word offset is independent of D32 destination default', {input, want, budget: 2,
        helpers: [call(name, bound(c, material, [0x3000, 0x43, 1]))]});
      if (observed.passed) {
        h.put(c, 0, want); const repaired = h.compile(c, 'D32 destination with unchanged base stopped capture', [], [], {stopped: true});
        if (repaired) h.observe(c, 'D32 new key interprets full immediate at logical target', {input: want, want: complete(want, 0x3005, {0: 0x90905678})});
        else h.skip(c, 'D32 reverse destination execution', 'D32 destination capture');
      } else h.skip(c, 'D32 reverse stopped repair', 'D16 word transfer');
    } else h.skip(c, 'D16 reverse execution', 'D16 source compilation'); h.host(c, 'close');
  }
}
assert.deepEqual(pins(), sourcePins, 'mode owners and literal mode oracle stayed fixed');
writeFileSync(join(output, 'default-source-pins.json'), JSON.stringify(sourcePins, null, 2) + '\n');
h.finish([
  'Finite current-v3 CS.D16 defaults plus existing operation families; SS.B32 held independent in these vectors.',
  'Literal v3/R3SB mode images are independent of product encoders; full4796 arena/all mapped RAM and real helper packets retained by shared CS-base receipt harness.',
  'Same physical bytes and same logical PC have different D interpretations; mismatched keys begin no instruction and preserve RF.',
  'Forward JMP/CALL/RETF/IRET and reverse word JMP prove offset width is independent of destination default, with one retirement before NeedCode.',
  'Source/native high-EIP boundary probes remain decoder evidence only; this actual suite makes no independent hardware high-IP wrap claim.',
  'INT imm8/IDT delivery, NT1/task return, privilege transfer and B16 mixed ENTER boundaries remain explicit and separate.',
  'Old-engine compile/preflight refusal is recorded as admission failure; no browser, game or performance claim.',
]);
