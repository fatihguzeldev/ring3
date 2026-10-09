import assert from 'node:assert/strict';
import {mkdirSync, writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

export const SIZE = 4364, CODE = 0x1000;
const LOW = [[0, 1, 0x7fff, 0xffff, 0x8000, 0x1234, 0xaaaa, 0x5555],
  [0x7fff, 0xffff, 0, 0x8000, 1, 0xaaaa, 0x5555, 0x1234],
  [0x8000, 0x7fff, 0xffff, 0x7fff, 0x1234, 1, 0x5555, 0xaaaa]];
const TRUE_FLAGS = [0x802, 2, 3, 2, 0x42, 2, 3, 2, 0x82, 2, 6, 2, 0x82, 2, 0x42, 2];
const FALSE_FLAGS = [2, 0x802, 2, 3, 2, 0x42, 2, 3, 2, 0x82, 2, 6, 2, 0x82, 2, 0x42];
function record(magic, length, fields = [], version = 1) {
  const bytes = Buffer.alloc(length); bytes.write(magic); bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(length, 8);
  fields.forEach((n, i) => bytes.writeUInt32LE(n >>> 0, 16 + i * 4)); return bytes;
}
export function seedHex(pc, flags = 2, pattern = 0, consumer = null) {
  const registers = LOW[pattern].map((low, i) => (((0xa101 + pattern * 0x111 + i * 0x101) << 16) | low) >>> 0);
  if (consumer !== null) {
    registers[0] = ((registers[0] & 0xffff0000) | (consumer === 0 ? 0x8000 : 0)) >>> 0;
    registers[1] = ((registers[1] & 0xffff0000) | 1) >>> 0;
  }
  const bytes = Buffer.alloc(140);
  bytes.set(record('R3ST', 56, [...registers, pc, flags]));
  bytes.set(record('R3EX', 40, [3, 0, 0, 0, 0, 0], 3), 56);
  bytes.set(record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]), 100);
  return bytes.toString('hex');
}
export function initialHex() {
  const bytes = Buffer.from(Array.from({length: SIZE}, (_, i) => (i * 37 + 19) & 255));
  bytes.set(Buffer.from(seedHex(CODE), 'hex'));
  const fp = record('R3FP', 128);
  for (const [at, n] of [[16, 0x27f], [18, 0x2840], [20, 0xaaaa], [22, 0x345], [32, 0x33], [34, 0x2b]]) fp.writeUInt16LE(n, at);
  fp.writeUInt32LE(0x12345678, 24); fp.writeUInt32LE(0x9abcdef0, 28);
  for (let i = 40; i < 120; i++) fp[i] = (i * 29 + 7) & 255;
  bytes.set(fp, 4236); return bytes.toString('hex');
}
export function bankPlan() {
  const bytes = Buffer.alloc(4096, 0xcc), forms = [], groups = []; let at = 0, start = 0, first = 0;
  function append(cc, destination, source) {
    const raw = [0x66, 0x0f, 0x40 + cc, 0xc0 | destination << 3 | source];
    forms.push({id: forms.length, cc, destination, source, pc: CODE + at, length: 4, hex: Buffer.from(raw).toString('hex'), group: `main-${groups.length}`});
    bytes.set(raw, at); at += 4;
    if (forms.length === 48 || forms.length === 79) {
      groups.push({id: `main-${groups.length}`, entries: false, blocks: [[CODE + start, at - start]], forms: forms.slice(first).map(f => f.id)});
      bytes.set([0xeb, 0xfe], at); at += 2; start = at; first = forms.length;
    }
  }
  for (let cc = 0; cc < 16; cc++) append(cc, 0, 3);
  for (let destination = 0; destination < 8; destination++) for (let source = 0; source < 8; source++) if (destination !== 0 || source !== 3) append(4, destination, source);
  assert.equal(forms.length, 79);
  const blocks = [], paths = [];
  const pieces = ['6639c8660f4cd3660f4dee0f4cfe0f90c40f92c17c02', '6639c8660f42d3660f43ee0f42fe0f92c40f90c17202'];
  for (let path = 0; path < 2; path++) {
    const offset = 2048 + path * 64, piece = Buffer.from(pieces[path], 'hex');
    assert.equal(piece.length, 22); bytes.set(piece, offset); bytes.set([0x0f, 0x0b, 0xeb, 0], offset + piece.length);
    blocks.push([CODE + offset, piece.length], [CODE + offset + piece.length + 2, 2]);
    paths.push({id: path, pc: CODE + offset, end_pc: CODE + offset + piece.length + 4, instructions: 8});
  }
  groups.push({id: 'consumer', entries: false, blocks, forms: []});
  at = 2560; const entryForms = [];
  for (let cc = 0; cc < 16; cc++) {
    const destination = cc % 8, source = (cc + 3) % 8, raw = [0x66, 0x0f, 0x40 + cc, 0xc0 | destination << 3 | source];
    entryForms.push({id: forms.length + entryForms.length, cc, destination, source, pc: CODE + at, length: 4, hex: Buffer.from(raw).toString('hex'), group: 'entry-forms'});
    bytes.set(raw, at); at += 4;
  }
  bytes.set([0xeb, 0xfe], at);
  groups.push({id: 'entry-forms', entries: true, blocks: [[CODE + 2560, 66]], forms: entryForms.map(f => f.id)});
  groups.push({id: 'entry-consumer', entries: true, blocks, forms: []});
  bytes.set([0x0f, 0x0b], 3840);
  return {hex: bytes.toString('hex'), forms: [...forms, ...entryForms], groups, consumer: {blocks, paths}, invalid_pc: CODE + 3840};
}
export function makePlan() {
  const bank = bankPlan(), actions = [], contexts = []; let nextId = 0;
  const main = bank.forms.filter(f => !f.group.startsWith('entry'));
  const truth = [];
  for (let pattern = 0; pattern < 3; pattern++) for (let combination = 0; combination < 32; combination++) for (let cc = 0; cc < 16; cc++) {
    const flags = 2 | [1, 4, 0x40, 0x80, 0x800].reduce((n, bit, i) => n | ((combination >> i & 1) * bit), 0) | [0, 0x10, 0x400][pattern];
    truth.push({form: cc, flags, pattern, label: 'predicate truth'});
  }
  for (const f of main.slice(16)) for (const flags of [2, 0x42]) truth.push({form: f.id, flags, pattern: 0, label: 'all ordered aliases'});
  assert.equal(truth.length, 1662);
  function context(owner, entries) {
    const spec = {id: contexts.length + 1, owner, entries, key: [0x574d0000 + contexts.length + 1, 0x434d4f56], pages: 1}; contexts.push(spec);
    const add = (kind, fields = {}) => actions.push({id: nextId++, context: spec.id, kind, ...fields});
    const input = (offset, hex, label) => add('input', {offset, hex, label});
    const host = (name, args, fields = {}) => add('host', {name, args, ...fields});
    const call = (group, budget, label, plannedRetired, fields = {}) => add('call', {group, channel: owner === 'resident' && !entries ? 'dispatch' : 'direct', budget, label, planned_retired: plannedRetired, ...fields});
    add('open'); input(0, initialHex(), 'full authored nondefault arena');
    if (owner !== 'standalone') {host('map', [CODE, 1, 7]); input(140, bank.hex, 'complete literal bank'); host('upload', [CODE, 4096]);}
    const selected = bank.groups.filter(g => g.entries === entries);
    for (const [slot, group] of selected.entries()) {
      if (owner !== 'standalone') {
        const request = Buffer.alloc(group.blocks.length * (entries ? 4 : 8));
        group.blocks.forEach(([pc, length], i) => {request.writeUInt32LE(pc, i * (entries ? 4 : 8)); if (!entries) request.writeUInt32LE(length, i * 8 + 4);});
        input(140, request.toString('hex'), 'compiler descriptors');
        host(owner === 'resident' ? entries ? 'compile_resident_entries' : 'compile_resident' : entries ? 'compile_entries' : 'compile', entries ? [group.blocks.length, 0] : [group.blocks.length], {group: group.id});
      }
      add('module', {target: 'child', group: group.id, slot, file: `${spec.id}-${group.id}.wasm`, capture: owner === 'standalone' ? `${group.id}.wasm` : null});
      if (owner === 'resident') {
        add('table', {group: group.id, slot}); host('acknowledge_resident_installation', ['key_low', 'key_high', 'unit_low', 'unit_high', slot], {group: group.id});
        if (slot === 0) {host('dispatcher_module', ['key_low', 'key_high']); add('module', {target: 'dispatcher', group: 'dispatcher', file: `${spec.id}-dispatcher.wasm`, slot: null, capture: null});}
      }
      const rows = entries ? bank.forms.filter(f => f.group === group.id).flatMap(f => [false, true].map(outcome => ({form: f.id, flags: (outcome ? TRUE_FLAGS : FALSE_FLAGS)[f.cc] | (f.cc % 2 ? 0x10 : 0x400), pattern: f.cc % 3, label: 'entry predicate representative'}))) : truth.filter(row => bank.forms[row.form].group === group.id);
      for (const row of rows) {const f = bank.forms[row.form]; input(0, seedHex(f.pc, row.flags, row.pattern), row.label); call(group.id, 1, row.label, 1, {form: f.id});}
      if (group.forms.length === 0) {
        for (const path of bank.consumer.paths) {
          input(0, seedHex(path.pc, 0xcd7, 0, path.id), 'continuous consumer seed'); call(group.id, 9, 'continuous consumer', 8, {path: path.id});
          input(0, seedHex(path.pc, 0xcd7, 0, path.id), 'split consumer seed');
          for (const budget of [1, 3, 4]) call(group.id, budget, 'split consumer', budget, {path: path.id});
          call(group.id, 1, 'consumer no replay', 0, {path: path.id});
        }
        input(0, seedHex(bank.consumer.paths[0].pc, 0xcd7, 0, 0), 'zero and cancel seed');
        call(group.id, 0, 'zero budget', 0); input(96, '01000000', 'set cancel');
        call(group.id, 9, 'cancel positive budget', 0); call(group.id, 0, 'cancel zero budget', 0); input(96, '00000000', 'clear cancel');
      }
    }
    const keeper = selected.at(-1);
    if (owner !== 'standalone') {
      const firstCmov = bank.consumer.paths[0].pc + 3, bad = Buffer.alloc(entries ? 8 : 16);
      bad.writeUInt32LE(firstCmov); if (!entries) bad.writeUInt32LE(4, 4);
      bad.writeUInt32LE(bank.invalid_pc, entries ? 4 : 8); if (!entries) bad.writeUInt32LE(2, 12);
      input(140, bad.toString('hex'), 'late complete UD2 candidate');
      host(owner === 'resident' ? entries ? 'compile_resident_entries' : 'compile_resident' : entries ? 'compile_entries' : 'compile', entries ? [2, 0] : [2], {group: 'late-failure', allowed_failure: true});
      input(0, seedHex(firstCmov, 0xcd7), 'prior owner survival'); call(keeper.id, 1, 'prior owner survival', 1);
      add('guard', {group: keeper.id, wrong_key: true});
      host('write8', [CODE, 0x66]); call(keeper.id, 1, 'same-byte stale direct', 0, {channel: 'direct'});
      if (owner === 'resident') call(keeper.id, 1, 'same-byte stale dispatcher', 0, {channel: 'dispatch'});
    }
    add('close'); if (owner !== 'standalone') add('closed_call', {group: keeper.id, budget: 1, channel: 'direct', planned_retired: 0});
  }
  for (const entries of [false, true]) for (const owner of ['standalone', 'replacement', 'resident']) context(owner, entries);
  const generated = actions.filter(a => a.kind === 'call' || a.kind === 'closed_call');
  const counts = {contexts: contexts.length, actions: actions.length, generated_calls: generated.length,
    planned_retired: generated.reduce((n, a) => n + a.planned_retired, 0),
    status0_calls: generated.filter(a => a.kind === 'call' && !a.label.startsWith('same-byte stale')).length,
    positive_calls: generated.filter(a => a.planned_retired > 0).length,
    zero_retirement_calls: generated.filter(a => a.planned_retired === 0).length,
    host_calls: actions.filter(a => a.kind === 'host').length, guard_calls: actions.filter(a => a.kind === 'guard').length,
    inputs: actions.filter(a => a.kind === 'input').length, tables: actions.filter(a => a.kind === 'table').length,
    child_modules: actions.filter(a => a.kind === 'module' && a.target === 'child').length,
    dispatchers: actions.filter(a => a.kind === 'module' && a.target === 'dispatcher').length,
    frames: actions.reduce((n, a) => n + (a.kind === 'closed_call' ? 0 : a.kind === 'open' || a.kind === 'close' ? 1 : 2), 0),
    checkpoints: generated.length + actions.filter(a => a.kind === 'guard' || a.kind === 'close').length + 1,
    capture_files: 8, physical_files: 26};
  counts.journal_rows = counts.frames + counts.actions + counts.child_modules + counts.dispatchers;
  counts.host_api_calls = 8 + counts.host_calls + counts.guard_calls + actions.filter(a => a.kind === 'host' && a.name.startsWith('compile') && contexts[a.context - 1].owner === 'replacement').length * 3 + 4;
  assert.equal(counts.generated_calls, 5174); assert.equal(counts.planned_retired, 5278); assert.equal(counts.child_modules + counts.dispatchers, 17); assert.ok(counts.frames <= 21300);
  return {schema_version: 1, arena_bytes: SIZE, code_address: CODE, bank, contexts, actions, counts};
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  assert.equal(process.argv[2], '--plan'); const output = resolve(process.argv[3]); mkdirSync(output, {recursive: true});
  const plan = makePlan(); writeFileSync(resolve(output, 'plan.json'), JSON.stringify(plan, null, 2) + '\n', {flag: 'wx'});
  writeFileSync(resolve(output, 'bank.x86'), Buffer.from(plan.bank.hex, 'hex'), {flag: 'wx'}); console.log(JSON.stringify(plan.counts));
}
