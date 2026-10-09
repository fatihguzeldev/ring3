import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdirSync, writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

export const SIZE = 4364;
export const PAGES = 16;
export const BUDGET = 8192;
export const SHAPES = [1, 4, 8].flatMap(units => [1, 8].map(blocks => ({id: 0, units, blocks}))).map((s, id) => ({...s, id}));
export const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

function record(magic, length, fields, version = 1) {
  const b = Buffer.alloc(length); b.write(magic); b.writeUInt16LE(version, 4);
  b.writeUInt16LE(1, 6); b.writeUInt32LE(length, 8);
  fields.forEach((v, i) => b.writeUInt32LE(v >>> 0, 16 + i * 4));
  return b;
}

// Authored state, including a wrap of EAX during the first full-budget call.
export function seedHex(eax = 0xfffff001) {
  const b = Buffer.alloc(140);
  b.set(record('R3ST', 56, [eax, 0x12342222, 0x23453333, 0x34564444, 0x45675555, 0x56786666, 0x67897777, 0x789a8888, 0x1000, 0xcd7]));
  b.set(record('R3EX', 40, [3, 0, 0, 0, 0, 0], 3), 56);
  b.set(record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]), 100);
  return b.toString('hex');
}

export function initialHex() {
  const b = Buffer.from(Array.from({length: SIZE}, (_, i) => (i * 37 + 19) & 255));
  const fp = record('R3FP', 128, []);
  for (const [at, value] of [[16, 0x27f], [18, 0x2840], [20, 0xaaaa], [22, 0x345], [32, 0x33], [34, 0x2b]]) fp.writeUInt16LE(value, at);
  fp.writeUInt32LE(0x12345678, 24); fp.writeUInt32LE(0x9abcdef0, 28);
  for (let i = 40; i < 120; i++) fp[i] = (i * 29 + 7) & 255;
  b.set(fp, 4236); b.set(Buffer.from(seedHex(), 'hex'));
  return b.toString('hex');
}

export function banksFor(shape) {
  return Array.from({length: shape.units}, (_, unit) => {
    const pc = 0x1000 + unit * 4096, next = 0x1000 + ((unit + 1) % shape.units) * 4096;
    const b = Buffer.alloc(194);
    for (let i = 0; i < 63; i++) b.set([0x8d, 0x40, 1], i * 3);
    b[189] = 0xe9; b.writeInt32LE(next - (pc + b.length), 190);
    const blocks = shape.blocks === 1 ? [{pc, length: 194}] : Array.from({length: 8}, (_, i) => ({pc: pc + i * 24, length: i === 7 ? 26 : 24}));
    return {unit, pc, hex: b.toString('hex'), blocks, file: `bank-${shape.id}-${unit}.x86`, sha256: sha256(b)};
  });
}

export function makePlan() {
  let actionId = 0, contextId = 0;
  const processes = [];
  for (let index = 0; index < 3; index++) {
    const order = Array.from({length: 6}, (_, i) => (i + 2 * index) % 6), contexts = [];
    function context(shape, role, ordinal) {
      const c = {id: ++contextId, shape_id: shape.id, role, ...(ordinal ? {ordinal} : {}), pages: PAGES, key: [contextId, 0x50484153], banks: banksFor(shape), actions: []};
      const add = (kind, fields = {}) => c.actions.push({id: actionId++, kind, ...fields});
      const input = (offset, hex) => add('input', {offset, hex});
      const host = (name, args, fields = {}) => add('host', {name, args, ...fields});
      const calls = (channel, count, phase, fields = {}) => add('calls', {channel, count, phase, timed: false, ...fields});
      input(0, initialHex());
      for (const bank of c.banks) {
        host('map', [bank.pc, 1, 7]); input(140, bank.hex); host('upload', [bank.pc, 194]);
        if (role !== 'stale') host('protect', [bank.pc, 1, 5]);
      }
      for (const bank of c.banks) {
        const request = Buffer.alloc(bank.blocks.length * 8);
        bank.blocks.forEach((block, i) => {request.writeUInt32LE(block.pc, i * 8); request.writeUInt32LE(block.length, i * 8 + 4);});
        input(140, request.toString('hex'));
        host('compile_resident', [bank.blocks.length], {unit: bank.unit});
        add('module', {target: 'child', unit: bank.unit, file: `${c.id}-child-${bank.unit}.wasm`});
        add('table', {unit: bank.unit});
        host('acknowledge_resident_installation', ['key_low', 'key_high', `unit:${bank.unit}:low`, `unit:${bank.unit}:high`, bank.unit], {unit: bank.unit});
      }
      host('dispatcher_module', ['key_low', 'key_high']);
      add('module', {target: 'dispatcher', file: `${c.id}-dispatcher.wasm`});
      input(0, seedHex());
      if (role === 'cold') calls('dispatch', 1, 'cold_witness', {budget: BUDGET});
      if (role === 'warm') {
        calls('dispatch', 1, 'first', {budget: BUDGET, timed: true});
        calls('dispatch', 100, 'dispatch_warm', {budget: BUDGET});
        for (let sample = 0; sample < 30; sample++) calls('dispatch', 32, 'dispatch_sample', {budget: BUDGET, timed: true, sample});
        if (shape.units === 1) {
          calls('direct', 1, 'direct_first', {budget: BUDGET, unit: 0});
          calls('direct', 99, 'direct_warm', {budget: BUDGET, unit: 0});
          for (let sample = 0; sample < 30; sample++) calls('direct', 32, 'direct_sample', {budget: BUDGET, unit: 0, timed: true, sample});
        }
        for (const channel of ['guard', 'finder']) {
          calls(channel, 1024, `${channel}_warm`);
          for (let sample = 0; sample < 30; sample++) calls(channel, 1024, `${channel}_sample`, {timed: true, sample});
        }
        for (let sample = 0; sample < 30; sample++) calls('empty', 1024, 'empty_sample', {timed: true, sample});
        input(0, seedHex(0xfffffff0));
        for (const budget of [1, 7, 55, 2]) calls('dispatch', 1, 'matrix', {budget});
      }
      if (role === 'zero') calls('dispatch', 1, 'control_zero', {budget: 0});
      if (role === 'cancel') {
        input(96, '01000000');
        calls('dispatch', 1, 'control_cancel', {budget: BUDGET});
        calls('dispatch', 1, 'control_cancel_zero', {budget: 0});
      }
      if (role === 'stale') {
        host('write8', [0x1000, 0x8d]);
        calls('dispatch', 1, 'control_stale', {budget: BUDGET, expected_status: 4});
      }
      add('close'); contexts.push(c);
    }
    for (const shapeId of order) {
      const shape = SHAPES[shapeId];
      for (let ordinal = 1; ordinal <= 30; ordinal++) context(shape, 'cold', ordinal);
      context(shape, 'warm');
    }
    for (const role of ['zero', 'cancel', 'stale']) context(SHAPES[0], role);
    processes.push({index, order, contexts});
  }
  const contexts = processes.flatMap(p => p.contexts), actions = contexts.flatMap(c => c.actions);
  const generated = actions.filter(a => a.kind === 'calls' && ['dispatch', 'direct'].includes(a.channel));
  const positive = generated.filter(a => a.phase !== 'control_stale' && !a.phase.startsWith('control_cancel') && a.budget > 0);
  const sum = rows => rows.reduce((n, a) => n + a.count, 0);
  const units = contexts.reduce((n, c) => n + c.banks.length, 0);
  const counts = {
    processes: 3, engine_contexts: contexts.length, child_modules: units, dispatcher_modules: contexts.length,
    generated_calls: sum(generated), positive_calls: sum(positive), zero_retirement_controls: sum(generated) - sum(positive),
    expected_instructions: positive.reduce((n, a) => n + a.count * a.budget, 0),
    guard_calls: sum(actions.filter(a => a.channel === 'guard')), finder_calls: sum(actions.filter(a => a.channel === 'finder')),
    actions: actions.length, journal_events: actions.length + contexts.length,
    raw_frames: actions.reduce((n, a) => n + (a.kind === 'close' ? 1 : 2), 0) + contexts.length,
    // open + arena_ptr are implicit; module calls do not invoke another engine API.
    non_loop_host_api_calls: actions.filter(a => a.kind === 'host' || a.kind === 'close').length + contexts.length * 2,
    warm_timed_expected_instructions: positive.filter(a => a.phase.endsWith('_sample')).reduce((n, a) => n + a.count * a.budget, 0),
    individual_generated_witnesses: sum(generated.filter(a => a.count === 1)),
    module_files: units + contexts.length, bank_files: 26 * 3,
    // Per process: manifest, plan, arenas, journal, checkpoints, result.
    process_physical_files: units + contexts.length + 26 * 3 + 6 * 3,
    journal_rows: actions.length + contexts.length + (actions.reduce((n, a) => n + (a.kind === 'close' ? 1 : 2), 0) + contexts.length) + units + contexts.length,
    checkpoint_rows: actions.filter(a => a.kind === 'calls' || a.kind === 'close').length + processes.length,
    explicit_gc_calls: contexts.length + actions.filter(a => a.kind === 'calls' && a.timed).length,
    individual_expected_instructions: positive.filter(a => a.count === 1).reduce((n, a) => n + a.budget, 0),
  };
  assert.equal(counts.generated_calls, 26082); assert.equal(counts.expected_instructions, 212976786);
  assert.equal(counts.engine_contexts, 567); assert.equal(counts.child_modules, 2427);
  assert.equal(counts.guard_calls, 571392); assert.equal(counts.finder_calls, 571392);
  assert.equal(counts.warm_timed_expected_instructions, 188743680);
  assert.equal(counts.individual_generated_witnesses, 648);
  return {schema_version: 1, layout: {arena_bytes: SIZE, budget: BUDGET, pages: PAGES}, shapes: SHAPES, counts, processes};
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  assert.equal(process.argv[2], '--plan'); const output = resolve(process.argv[3]);
  mkdirSync(output, {recursive: true}); const plan = makePlan();
  writeFileSync(resolve(output, 'plan.json'), JSON.stringify(plan, null, 2) + '\n', {flag: 'wx'});
  for (const shape of SHAPES) for (const bank of banksFor(shape)) writeFileSync(resolve(output, bank.file), Buffer.from(bank.hex, 'hex'), {flag: 'wx'});
  console.log(JSON.stringify(plan.counts));
}
