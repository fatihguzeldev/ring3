import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {harness, PC, SEG, BASES, words, record, state, registers, exit, descriptor, hash, writeRAM} from '../../../../../../segments/code/base/fixtures/p2-cs-base/runtime/support.mjs';

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root, 'explicit engine, output and root');
const paths = ['engine/src/abi/far.rs', 'engine/src/abi/cpu_helper.rs',
  'engine/src/cpu/x86/segments/transfers.rs', 'engine/src/cpu/x86/segments/transfers/gates.rs',
  'engine/src/cpu/dbt/wasm/cpu_result.rs', 'engine/src/cpu/dbt/wasm/far/gate.rs',
  'engine/src/process/cpu/far/call.rs', 'engine/src/cpu/dbt/wasm/stack/pointer.rs',
  'engine/tests/cpu/control/far/gates/wasm.rs',
  'engine/tests/cpu/control/far/gates/fixtures/p2-call-gates/runtime/run.mjs'];
const pins = () => Object.fromEntries(paths.map(path => [path, hash(readFileSync(join(root, path)))]));
const sourcePins = pins(), h = await harness(enginePath, output, root);
const call = (name, args, status = 0) => ({name, args: args.map(value => value >>> 0), status});
const helper = (c, operation) => `${operation}${c.resident ? '_resident' : ''}`;
const bound = (c, material, args) => [...(c.resident ? material.binding : []), ...args];
function gate(kind, selector, target, {dpl = 3, present = true, count = 31, reserved = 0} = {}) {
  return [(selector * 65536 + (target & 0xffff)) >>> 0,
    ((kind === 12 ? target & 0xffff0000 : 0) | kind << 8 | dpl << 13 | Number(present) << 15 | count | reserved) >>> 0];
}
function configure(c, selector, raw) {h.host(c, 'configure_segment_descriptor_raw', [selector, ...raw]);}
function rawCode({base = 0, default32 = true, present = true, conforming = false, dpl = 3, limit} = {}) {
  if (limit === undefined) {const raw = descriptor(base); return [raw.low, (raw.high - (default32 ? 0 : 0x400000)
    - (present ? 0 : 0x8000) - (3 - dpl) * 0x2000 + (conforming ? 0x400 : 0)) >>> 0];}
  return [((base & 0xffff) * 65536 + (limit & 0xffff)) >>> 0,
    ((base & 0xff000000) | base >>> 16 & 255 | (limit & 0xf0000) | Number(default32) << 22
      | Number(present) << 15 | dpl << 13 | (conforming ? 0x1e00 : 0x1a00)) >>> 0];
}
function loaded(bytes, {csBase = 0, csDefault32 = true, ssBase = 0, ssDefault32 = true} = {}) {
  const current = csBase !== 0 || ssBase !== 0 || !csDefault32 || !ssDefault32;
  const sg = record('R3SG', 64, [], current ? 3 : 1);
  [0x23, 0x4b, 0x23, 0x23, 0x3b, 0].forEach((value, index) => sg.writeUInt16LE(value, 16 + index * 2));
  if (current) [0xf33fffff, csDefault32 ? 0xf3bfffff : 0xf1bfffff, ssDefault32 ? 0xf33fffff : 0xf13fffff,
    0xf33fffff, 0xf33fffff, 0].forEach((value, index) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][index]));
  bytes.set(sg, SEG); bytes.set(record('R3SB', 40, [0, csBase, ssBase, 0, 0, 0]), BASES); return bytes;
}
function initial(options = {}) {
  const bytes = state(h.initial, options);
  if (options.ssDefault32 === false) {
    const sg = record('R3SG', 64, [], 3);
    [0x23, 0x1b, 0x23, 0x23, 0x3b, 0].forEach((value, index) => sg.writeUInt16LE(value, 16 + index * 2));
    [0xf33fffff, 0xf3bfffff, 0xf13fffff, 0xf33fffff, 0xf33fffff, 0].forEach((value, index) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][index]));
    bytes.set(sg, SEG); bytes.set(record('R3SB', 40, options.bases), BASES);
  }
  return bytes;
}
function instruction(operation, word, memory = false) {
  const prefix = word ? Buffer.from([0x66]) : Buffer.alloc(0);
  const offset = word ? 0xbeef : 0xdeadbeef;
  const operand = Buffer.concat([words([offset]).subarray(0, word ? 2 : 4), Buffer.from([0x43, 0])]);
  const body = memory ? Buffer.from(operation === 'call' ? 'ff1d20300000' : 'ff2d20300000', 'hex')
    : Buffer.concat([Buffer.from([operation === 'call' ? 0x9a : 0xea]), operand]);
  return {code: Buffer.concat([prefix, body]), operand, offset};
}
function done(input, target, operation, slot, returnIP, {retired = 1, reason = 1, context = {}, esp = input.readUInt32LE(32)} = {}) {
  const want = loaded(Buffer.from(input), context); want.writeUInt32LE(target, 48); want.writeUInt32LE(2, 52);
  if (operation === 'call') {
    const active = context.ssDefault32 === false ? ((esp - 2 * slot) & 0xffff) | (esp & 0xffff0000) : esp - 2 * slot;
    want.writeUInt32LE(active >>> 0, 32);
  }
  want.set(record('R3MH', 40, [0, target, 0, 0, 0, operation === 'call' ? slot : 0], 9), 100);
  return exit(want, reason, retired, reason >= 16 ? 7 : 6);
}
function frame(ram, slot, returned, esp = 0x7800, {ssBase = 0, ssDefault32 = true} = {}) {
  const offset = delta => ssDefault32 ? (esp + delta) >>> 0 : (esp + delta) & 0xffff;
  writeRAM(ram, (ssBase + offset(-slot)) >>> 0, words([0x1b]).subarray(0, slot));
  writeRAM(ram, (ssBase + offset(-2 * slot)) >>> 0, words([returned]).subarray(0, slot));
}
function expectedCall(c, material, name, op, word, offset, returned, esp) {
  return call(name, bound(c, material, [offset, 0x43, Number(word), ...(op === 'call' ? [returned, esp] : [])]));
}
function protocolMaterial(c, original, selected, mutate, returnedStatus) {
  // declared protocol-negative control: each import still invokes the real engine helper,
  // then only the selected returned packet is deliberately corrupted before consumption.
  const module = new WebAssembly.Module(new Uint8Array(c.memory.buffer, original.pointer, original.length));
  const imports = WebAssembly.Module.imports(module);
  const ring3 = Object.fromEntries(imports.filter(imp => imp.kind === 'function').map(imp => [imp.name, (...args) => {
    const status = c.api[imp.name](...args);
    c.calls.push({name: imp.name, args: args.map(value => value >>> 0), status});
    if (imp.name === selected) {const bytes = h.arena(c); mutate(bytes); h.put(c, 0, bytes);}
    return imp.name === selected ? returnedStatus ?? status : status;
  }]));
  return {...original, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run};
}

for (const resident of [false, true]) for (const entries of [false, true]) {
  for (const kind of [4, 12]) for (const op of ['jump', 'call']) for (const word of [false, true]) for (const memory of [false, true]) {
    const target = kind === 4 ? 0x2345 : 0x12345, slot = kind === 4 ? 2 : 4;
    const title = `gate${slot * 8} ${op} op${word ? 16 : 32} ${memory ? 'memory' : 'immediate'}`;
    const c = h.open(resident, entries, title, [[PC], [target & 0xfffff000], [0x3000, 3], [0x7000, 3]]);
    const input = initial(), {code, operand, offset} = instruction(op, word, memory), returned = PC + code.length;
    h.upload(c, PC, code); h.upload(c, target, Buffer.from('ebfe', 'hex'));
    h.upload(c, 0x3020, operand); h.upload(c, 0x7000, Buffer.alloc(4096, 0x6b));
    configure(c, 0x4b, rawCode()); configure(c, 0x43, gate(kind, 0x48, target));
    const name = helper(c, `far_${op}`); h.put(c, 0, input);
    const material = h.compile(c, title + ' capture', [[PC, code.length], [target, 2]], [...(memory ? ['cpu_read'] : []), name]);
    if (material) {
      for (const budget of [1, 2]) {
        const want = done(input, target, op, slot, returned, {retired: budget}), wantRAM = h.ram(c);
        if (memory) want.set(operand, 140);
        if (op === 'call') frame(wantRAM, slot, returned);
        const helpers = [...(memory ? [call('cpu_read', [0x3020, operand.length, 1])] : []),
          expectedCall(c, material, name, op, word, offset, returned, 0x7800)];
        h.observe(c, title + (budget === 1 ? ' selected gate target and frame' : ' same-key target loop after one gate retirement'),
          {input, want, wantRAM, budget, helpers});
      }
      if (kind === 12 && op === 'call' && word && !memory) {
        for (const cancel of [0, 1]) {
          const stopped = Buffer.from(input); stopped.writeUInt32LE(cancel, 96);
          const want = exit(Buffer.from(stopped), cancel ? 2 : 1, 0, 6);
          h.observe(c, cancel ? 'cancel preserves RF and never begins gate CALL' : 'zero budget preserves RF and never begins gate CALL',
            {input: stopped, want, budget: 0, kind: 'control'});
        }
        const traced = Buffer.from(input); traced.writeUInt32LE(0x10102, 52);
        const want = done(traced, target, op, slot, returned, {reason: 16}); want.writeUInt32LE(0x102, 52);
        const wantRAM = h.ram(c); frame(wantRAM, slot, returned);
        h.observe(c, 'TF reports exactly one retired gate with committed selected frame and consumed RF',
          {input: traced, want, wantRAM, budget: 2, helpers: [expectedCall(c, material, name, op, word, offset, returned, 0x7800)]});
      }
    } else h.skip(c, title + ' execution', title + ' capture');
    h.host(c, 'close');
  }

  for (const op of ['jump', 'call']) {
    const c = h.open(resident, entries, `gate ${op} fault authority`, [[PC], [0x7000, 3]]);
    const {code, offset} = instruction(op, true), returned = PC + code.length, name = helper(c, `far_${op}`);
    h.upload(c, PC, code); h.upload(c, 0x7000, Buffer.alloc(4096, 0x6b));
    const seed = initial(); h.put(c, 0, seed);
    const material = h.compile(c, `gate ${op} fault capture`, [[PC, code.length]], [name]);
    if (material) {
      const cases = [
        {label: 'outer DPL before presence and null target', outer: {dpl: 2, present: false}, targetSelector: 0, version: 7, status: 3, detail: 0x40},
        {label: 'outer presence before null target', outer: {present: false}, targetSelector: 0, version: 7, status: 6, detail: 0x40},
        {label: 'inner null selector', targetSelector: 0, status: 3, detail: 0},
        {label: 'inner missing selector preserves TI and selector attribution', targetSelector: 0x57, status: 3, detail: 0x54},
        {label: 'inner data type before absence', targetRaw: [0xffff, 0x00cf7200], status: 3, detail: 0x48},
        {label: 'inner code presence before stack', targetRaw: rawCode({present: false}), status: 6, detail: 0x48, boundedSS: op === 'call'},
        {label: 'canonical reserved payload refusal is neutral', outer: {reserved: 0x80}, hostStatus: 7},
        {label: 'task gate remains a separate neutral refusal', kind: 5, hostStatus: 7},
        ...(op === 'jump' ? [{label: 'JMP lower-DPL nonconforming type privilege before absence', targetRaw: rawCode({dpl: 0, present: false}), status: 3, detail: 0x48}]
          : [
            {label: 'CALL valid lower-DPL nonconforming needs missing TSS authority', targetRaw: rawCode({dpl: 0}), hostStatus: 7},
            {label: 'CALL stack capacity before selected target limit', targetRaw: rawCode({limit: 0xffff}), target: 0x10000, boundedSS: true, esp: 0x7808, status: 7, detail: 0},
            {label: 'CALL selected target limit before page probing', targetRaw: rawCode({limit: 0xffff}), target: 0x10000, esp: 0x7004, status: 3, detail: 0},
            {label: 'CALL second dword slot fault leaves first mapped slot unchanged', esp: 0x7004, status: 1, detail: 1, address: 0x6ffc, access: 2},
            {label: 'CALL second word slot fault uses gate width despite instruction66', kind: 4, esp: 0x7002, status: 1, detail: 1, address: 0x6ffe, access: 2},
          ]),
      ];
      for (const row of cases) {
        const kind = row.kind ?? 12, slot = kind === 4 ? 2 : 4, target = row.target ?? 0x2000, targetSelector = row.targetSelector ?? 0x48;
        configure(c, 0x48, row.targetRaw ?? rawCode()); configure(c, 0x43, gate(kind, targetSelector, target, row.outer));
        const regs = [...registers]; regs[4] = row.esp ?? 0x7800; const input = initial({regs});
        if (row.boundedSS) {
          const sg = record('R3SG', 64, [], 2);
          [0x23, 0x1b, 0x23, 0x23, 0x3b, 0].forEach((value, index) => sg.writeUInt16LE(value, 16 + index * 2));
          [0xf33fffff, 0xf3bfffff, 0xf23077ff, 0xf33fffff, 0xf33fffff, 0].forEach((value, index) => sg.writeUInt32LE(value >>> 0, [28, 32, 36, 40, 44, 56][index]));
          input.set(sg, SEG);
        }
        const want = Buffer.from(input); want.writeUInt32LE(2, 52);
        if (row.hostStatus) {exit(want, 7, 0, 6); want.writeUInt32LE(3, 80);}
        else {
          const packet = record('R3MH', 40, [row.status, 0, row.detail, row.address ?? 0, row.access ?? 0,
            row.version === 7 ? 0 : op === 'call' ? slot : 0], row.version ?? 9);
          want.set(packet, 100); exit(want, row.status === 1 ? 5 : row.status + 8, 0, 6);
          want.writeUInt32LE(row.detail, 80);
          if (row.status === 1) {want.writeUInt32LE(row.address, 84); want.writeUInt32LE(row.access, 88); want.writeUInt32LE(slot, 92);}
        }
        h.observe(c, `${op}: ${row.label}`, {input, want, helpers: [expectedCall(c, material, name, op, true, offset, returned, regs[4])
          ].map(helperCall => ({...helperCall, status: row.hostStatus ?? 0}))});
      }
    } else h.skip(c, `gate ${op} fault execution`, `gate ${op} fault capture`);
    h.host(c, 'close');
  }

  for (const op of ['jump', 'call']) for (const csDefault32 of [false, true]) {
    const context = {csBase: csDefault32 ? 0x4000 : 0, csDefault32}, target = 0x2000, kind = csDefault32 ? 4 : 12, slot = kind === 4 ? 2 : 4;
    const title = `${op} gate switches ${csDefault32 ? 'CS base' : 'CS D'} inside invocation`;
    const pages = csDefault32 ? [[PC], [target], [0x6000], [0x7000, 3]] : [[PC], [target], [0x7000, 3]];
    const c = h.open(resident, entries, title, pages), input = initial(), {code, offset} = instruction(op, kind === 12), returned = PC + code.length;
    const destination = csDefault32 ? Buffer.from('b8df9b5713ebfe', 'hex') : Buffer.from('b834129090ebfe', 'hex');
    const decoy = csDefault32 ? Buffer.from('b80df0ad0bebfe', 'hex') : destination;
    h.upload(c, PC, code); h.upload(c, target, decoy); if (csDefault32) h.upload(c, 0x6000, destination);
    h.upload(c, 0x7000, Buffer.alloc(4096, 0x6b)); configure(c, 0x43, gate(kind, 0x48, target)); configure(c, 0x48, rawCode({base: context.csBase, default32: csDefault32}));
    const name = helper(c, `far_${op}`); h.put(c, 0, input);
    const material = h.compile(c, title + ' original key capture', [[PC, code.length], [target, destination.length]], [name]);
    if (material) {
      const want = done(input, target, op, slot, returned, {reason: 3, context}), wantRAM = h.ram(c);
      if (op === 'call') frame(wantRAM, slot, returned);
      const row = h.observe(c, title + ' retires once then requests new key', {input, want, wantRAM, budget: 2,
        helpers: [expectedCall(c, material, name, op, kind === 12, offset, returned, 0x7800)]});
      if (row.passed) {
        h.put(c, 0, want); const repaired = h.compile(c, title + ' stopped capture under selected key', [], [], {stopped: true});
        if (repaired) {
          const completed = Buffer.from(want); completed.writeUInt32LE(target + (csDefault32 ? 5 : 3), 48);
          completed.writeUInt32LE(csDefault32 ? 0x13579bdf : 0x12341234, 16); exit(completed, 1, 1, 1);
          h.observe(c, title + ' repaired body uses loaded mode and physical base', {input: want, want: completed});
        } else h.skip(c, title + ' new body execution', 'new-key stopped capture');
      } else h.skip(c, title + ' stopped repair', 'gate retirement and key reroute');
    } else h.skip(c, title + ' execution', 'original-key gate compilation');
    h.host(c, 'close');
  }

  for (const kind of [4, 12]) {
    const slot = kind === 4 ? 2 : 4, op = 'call', word = kind === 12, target = 0x2345, context = {ssBase: 0x8000, ssDefault32: false};
    const title = `gate${slot * 8} independent nonflat B16 pointer wrap`;
    const c = h.open(resident, entries, title, [[PC], [0x2000], [0x8000, 3], [0x17000, 3], [0x18000, 3]]), regs = [...registers];
    regs[4] = 0xabcd0002;
    const input = initial({regs, bases: [0, 0, context.ssBase, 0, 0, 0], ssDefault32: false}), {code, offset} = instruction(op, word), returned = PC + code.length;
    h.upload(c, PC, code); h.upload(c, target, Buffer.from('ebfe', 'hex'));
    for (const page of [0x8000, 0x17000, 0x18000]) h.upload(c, page, Buffer.alloc(4096, 0x6b));
    configure(c, 0x43, gate(kind, 0x48, target)); configure(c, 0x48, rawCode());
    const name = helper(c, 'far_call'); h.put(c, 0, input);
    const material = h.compile(c, title + ' capture', [[PC, code.length], [target, 2]], [name]);
    if (material) {
      for (const budget of [1, 2]) {
        const want = done(input, target, op, slot, returned, {retired: budget, context, esp: regs[4]}), wantRAM = h.ram(c);
        frame(wantRAM, slot, returned, regs[4], context);
        h.observe(c, title + ' exact selected slots and preserved high ESP', {input, want, wantRAM, budget,
          helpers: [expectedCall(c, material, name, op, word, offset, returned, regs[4])]});
      }
    } else h.skip(c, title + ' execution', title + ' capture');
    h.host(c, 'close');
  }

  {
    const title = 'declared gate packet controls', c = h.open(resident, entries, title, [[PC], [0x5000], [0x7000, 3]]), input = initial();
    const {code, offset} = instruction('call', true), returned = PC + code.length, name = helper(c, 'far_call');
    h.upload(c, PC, code); h.upload(c, 0x5000, Buffer.from([0x90])); h.upload(c, 0x7000, Buffer.alloc(4096, 0x6b)); configure(c, 0x43, gate(12, 0, 0x2000));
    h.put(c, 0, input); const material = h.compile(c, title + ' capture', [[PC, code.length]], [name]);
    if (material) {
      const mutations = [
        ['unknown packet version', packet => packet.writeUInt16LE(10, 104)],
        ['missing effective CALL slot', packet => packet.writeUInt32LE(0, 136)],
        ['invalid one-byte CALL slot', packet => packet.writeUInt32LE(1, 136)],
        ['transport failure precedes malformed slot', packet => packet.writeUInt32LE(0, 136), 7, 3],
        ['SMC transport still requires a valid slot', packet => packet.writeUInt32LE(0, 136), 11, 2],
        ['invalid eight-byte CALL slot', packet => packet.writeUInt32LE(8, 136)],
        ['reserved header bits', packet => packet.writeUInt32LE(1, 112)],
        ['noncanonical inner GP selector RPL', packet => packet.writeUInt32LE(0x49, 124)],
        ['oversized inner GP selector', packet => packet.writeUInt32LE(0x10000, 124)],
        ['null NP selector', packet => packet.writeUInt32LE(6, 116)],
        ['floating point status is not a gate result', packet => packet.writeUInt32LE(4, 116)],
        ['SIMD status is not a gate result', packet => packet.writeUInt32LE(5, 116)],
        ['error reply may not publish target value', packet => packet.writeUInt32LE(0x2000, 120)],
        ['gate stack page fault cannot be a read', packet => {packet.set(words([1, 0, 1, 0x77fc, 1, 4]), 116);}],
        ['slot2 cannot publish a high target', packet => {packet.set(words([0, 0x10000, 0, 0, 0, 2]), 116);}],
      ];
      for (const [label, mutate, returnedStatus, detail = 2] of mutations) {
        const want = Buffer.from(input); want.writeUInt32LE(2, 52); want.set(record('R3MH', 40, [3, 0, 0, 0, 0, 4], 9), 100); mutate(want);
        exit(want, 7, 0, 6); want.writeUInt32LE(detail, 80);
        const modified = protocolMaterial(c, material, name, mutate, returnedStatus);
        h.observe(c, title + ': ' + label, {input, want, material: modified, kind: 'declared-protocol-control',
          helpers: [expectedCall(c, material, name, 'call', true, offset, returned, 0x7800)]});
      }
      const mutateKey = bytes => {
        loaded(bytes, {csBase: 0x4000}); bytes.writeUInt16LE(0x1b, SEG + 18); bytes.writeUInt32LE(0, 136);
      };
      const want = Buffer.from(input); want.writeUInt32LE(2, 52);
      want.set(record('R3MH', 40, [3, 0, 0, 0, 0, 4], 9), 100); mutateKey(want); exit(want, 3, 0, 6);
      h.observe(c, 'declared key mutation outranks malformed slot and rejected helper transport',
        {input, want, material: protocolMaterial(c, material, name, mutateKey, 7), kind: 'declared-context-transport-control',
          helpers: [expectedCall(c, material, name, 'call', true, offset, returned, 0x7800)]});
    } else h.skip(c, title + ' execution', title + ' capture');
    h.host(c, 'close');
  }
  {
    const title = 'v9 cannot replace a source pointer read packet', c = h.open(resident, entries, title, [[PC], [0x3000, 3]]), input = initial();
    const {code, operand} = instruction('jump', false, true), name = helper(c, 'far_jump');
    h.upload(c, PC, code); h.upload(c, 0x3020, operand); configure(c, 0x43, gate(12, 0x48, 0x2000)); configure(c, 0x48, rawCode());
    h.put(c, 0, input); const material = h.compile(c, title + ' capture', [[PC, code.length]], ['cpu_read', name]);
    if (material) {
      const mutate = bytes => bytes.writeUInt16LE(9, 104), want = Buffer.from(input); want.writeUInt32LE(2, 52); want.set(operand, 140);
      want.set(record('R3MH', 40, [0, 0, 0, 0, 0, 6], 9), 100); exit(want, 7, 0, 6); want.writeUInt32LE(2, 80);
      h.observe(c, title, {input, want, material: protocolMaterial(c, material, 'cpu_read', mutate), kind: 'declared-protocol-control',
        helpers: [call('cpu_read', [0x3020, 6, 1])]});
    } else h.skip(c, title + ' execution', title + ' capture');
    h.host(c, 'close');
  }
}
assert.deepEqual(pins(), sourcePins, 'gate owners and literal gate oracles stayed fixed');
writeFileSync(join(output, 'gate-source-pins.json'), JSON.stringify(sourcePins, null, 2) + '\n');
h.finish([
  'Configured canonical type4/type12 call gates at CPL3; same-CPL JMP/CALL only, direct v7 retained separately.',
  'Four profiles: replacement/resident crossed with explicit extents/entry decoding; every observation compares literal full4796 arena, all mapped guest RAM and exact real helper arguments/status.',
  'Gate target and frame width are independent of instruction66, target CS.D, and loaded SS.B; same-CPL count31 causes no parameter copies.',
  'Outer DPL/P, inner selector/type/DPL/P, cached-stack capacity, selected target limit, then batch page probes have distinct oracles. Privilege-changing CALL and task gates remain neutral host7 refusals.',
  'Based/D16 target key reroute retires once before NeedCode; stopped repair captures physical bytes under selected key.',
  'Malformed packet cases are declared controls: real guard/helpers execute, then the returned packet and optionally import transport are corrupted. One separately declared control also changes live CS key. The inner-null source guarantees no pre-corruption context/RAM effects.',
  'Gate-v9 remains rejected by the ordinary source-read validator; slot2/high-target is an independent falsifier for the reviewed supplementary validation.',
  'No TSS/IDT/privilege stack switch, browser/game, performance, shared-memory or hardware compatibility claim.',
]);
