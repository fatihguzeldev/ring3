import assert from 'node:assert/strict';
import {mkdirSync, writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

export const SIZE = 4364, CODE = 0x1000, BANK_BYTES = 8192;
export const FLAGS = [2, 0xcd7];
export const VECTORS = [
  [0xffff, 0x0001, 0x8000, 0x7fff, 0x00ff, 0xff00, 0x5555, 0xaaaa],
  [0x0001, 0xffff, 0x7fff, 0x8000, 0xff00, 0x00ff, 0xaaaa, 0x5555],
];
export const EDGES = [[0, 0], [0, 1], [0xffff, 1], [0x7fff, 1], [0x8000, 0x8000],
  [0xffff, 0xffff], [0xf, 1], [0xff, 1], [0x8000, 0], [0xffff, 0], [1, 0xffff], [0x5555, 0xaaaa]];

function record(magic, size, fields = [], version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic);
  bytes.writeUInt32LE(0x10000 + version, 4); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, 16 + i * 4)); return bytes;
}
export function seedHex(pc, flags = 2, vector = 0, overrides = []) {
  const registers = VECTORS[vector].map((low, i) => (((0xa101 + vector * 0x111 + i * 0x101) << 16) | low) >>> 0);
  for (const [index, low] of overrides) registers[index] = ((registers[index] & 0xffff0000) | low) >>> 0;
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
  for (const [at, value] of [[16, 0x27f], [18, 0x2840], [20, 0xaaaa], [22, 0x345], [32, 0x33], [34, 0x2b]]) fp.writeUInt16LE(value, at);
  fp.writeUInt32LE(0x12345678, 24); fp.writeUInt32LE(0x9abcdef0, 28);
  for (let i = 40; i < 120; i++) fp[i] = (i * 29 + 7) & 255;
  bytes.set(fp, 4236); return bytes.toString('hex');
}
export function bankPlan() {
  const bytes = Buffer.alloc(BANK_BYTES, 0xcc), forms = [], groups = [];
  function append(at, destination, source, group) {
    const instruction = [0x66, 0x0f, 0xc1, 0xc0 | (source << 3) | destination];
    const form = {id: forms.length, destination, source, group, pc: CODE + at, length: 4, hex: Buffer.from(instruction).toString('hex')};
    bytes.set(instruction, at); forms.push(form); return form.id;
  }
  for (let cohort = 0; cohort < 2; cohort++) {
    const id = `pairs-${cohort}`, start = cohort === 0 ? 0 : 0xf82, ids = [];
    for (let ordinal = cohort * 32; ordinal < (cohort + 1) * 32; ordinal++) ids.push(append(start + ids.length * 4, ordinal >> 3, ordinal & 7, id));
    bytes.set([0xeb, 0xfe], start + 128);
    groups.push({id, entries: false, blocks: [[CODE + start, 130]], forms: ids, instructions: 33});
  }
  const profileForms = [];
  const profilePairs = [...Array.from({length: 8}, (_, i) => [i, i]), [0, 1], [1, 0], [4, 0], [0, 4]];
  for (const [destination, source] of profilePairs) profileForms.push(append(0x400 + profileForms.length * 4, destination, source, 'profile'));
  bytes.set([0xeb, 0xfe], 0x430);
  const blocks = [[0x1400, 50]], paths = [];
  for (let index = 0; index < 2; index++) {
    const start = 0x800 + index * 0x80;
    const pieces = [Buffer.from(`660fc1d00f92c40f90c1660fc1c1${index === 0 ? '73' : '72'}02`, 'hex'), Buffer.from('6683d7000f92c7eb00', 'hex')];
    const pathBlocks = [[CODE + start, pieces[0].length], [CODE + start + pieces[0].length + 2, pieces[1].length]];
    bytes.set(pieces[0], start); bytes.set([0x0f, 0x0b], start + pieces[0].length); bytes.set(pieces[1], start + pieces[0].length + 2);
    blocks.push(...pathBlocks);
    paths.push({id: index, pc: CODE + start, end_pc: CODE + start + 27, blocks: pathBlocks, instructions: 8});
  }
  groups.push({id: 'profile', entries: false, blocks, forms: profileForms, instructions: 29});
  groups.push({id: 'entry-profile', entries: true, blocks, forms: profileForms, instructions: 29});
  bytes.set([0x0f, 0x0b], 0x1f00);
  assert.equal(forms.length, 76); assert.equal(groups.length, 4);
  return {hex: bytes.toString('hex'), forms, groups, consumer: {paths}, invalid_pc: 0x2f00};
}
export function makePlan() {
  const bank = bankPlan(), contexts = [], actions = []; let next = 0;
  function context(owner, entries, role = 'normal') {
    const spec = {id: contexts.length + 1, owner, entries, role, key: [0x58410000 + contexts.length + 1, 0x574f5244], pages: 2}; contexts.push(spec);
    const add = (kind, fields = {}) => actions.push({id: next++, context: spec.id, kind, ...fields});
    const input = (offset, hex, label) => add('input', {offset, hex, label});
    const host = (name, args, fields = {}) => add('host', {name, args, ...fields});
    const channel = owner === 'resident' && !entries ? 'dispatch' : 'direct';
    const call = (group, budget, label, retired, fields = {}) => add('call', {group, channel, budget, label, planned_retired: retired, ...fields});
    add('open'); input(0, initialHex(), 'authored full nondefault arena');
    if (owner !== 'standalone') {
      host('map', [CODE, 2, 7]);
      for (let page = 0; page < 2; page++) {input(140, bank.hex.slice(page * 8192, (page + 1) * 8192), 'literal bank page'); host('upload', [CODE + page * 4096, 4096]);}
      if (role !== 'stale') host('protect', [CODE, 2, 5]);
    }
    const profile = bank.groups.find(g => g.id === (entries ? 'entry-profile' : 'profile'));
    const selected = role === 'stale' || entries || owner === 'standalone' ? [profile] : [bank.groups[0], bank.groups[1], profile];
    for (const [slot, group] of selected.entries()) {
      if (owner !== 'standalone') {
        const request = Buffer.alloc(group.blocks.length * (entries ? 4 : 8));
        group.blocks.forEach(([pc, length], i) => {request.writeUInt32LE(pc, i * (entries ? 4 : 8)); if (!entries) request.writeUInt32LE(length, i * 8 + 4);});
        input(140, request.toString('hex'), 'exact compiler descriptors');
        host(owner === 'resident' ? entries ? 'compile_resident_entries' : 'compile_resident' : entries ? 'compile_entries' : 'compile', entries ? [group.blocks.length, 0] : [group.blocks.length], {group: group.id});
      }
      add('module', {target: 'child', group: group.id, slot: owner === 'resident' ? slot : null, file: `${spec.id}-${group.id}.wasm`, capture: owner === 'standalone' ? `${group.id}.wasm` : null});
      if (owner === 'resident') {
        add('table', {group: group.id, slot}); host('acknowledge_resident_installation', ['key_low', 'key_high', 'unit_low', 'unit_high', slot], {group: group.id});
        if (slot === 0 && !entries) {host('dispatcher_module', ['key_low', 'key_high']); add('module', {target: 'dispatcher', group: 'dispatcher', slot: null, file: `${spec.id}-dispatcher.wasm`, capture: null});}
      }
      if (role === 'stale') continue;
      if (group.id.startsWith('pairs-')) {
        for (const id of group.forms) for (let vector = 0; vector < 2; vector++) for (const flags of FLAGS) {
          const f = bank.forms[id]; input(0, seedHex(f.pc, flags, vector), 'complete ordered alias axis');
          call(group.id, 1, 'complete ordered alias axis', 1, {form: id, vector, flags});
        }
      } else {
        for (const id of profile.forms) {
          const f = bank.forms[id], vector = id & 1, flags = FLAGS[id & 1]; input(0, seedHex(f.pc, flags, vector), 'six-profile representative');
          call(group.id, 1, 'six-profile representative', 1, {form: id, vector, flags});
        }
        if (!entries && owner !== 'standalone') {
          for (const [edge, [destinationLow, sourceLow]] of EDGES.entries()) {
            const f = bank.forms[72], flags = FLAGS[edge & 1];
            input(0, seedHex(f.pc, flags, edge & 1, [[0, destinationLow], [1, sourceLow]]), 'separate arithmetic edge');
            call(group.id, 1, 'separate arithmetic edge', 1, {form: f.id, edge});
          }
          for (const [edge, low] of [0, 0x8000, 0x7fff, 0xffff].entries()) {
            const f = bank.forms[64]; input(0, seedHex(f.pc, FLAGS[edge & 1], edge & 1, [[0, low]]), 'separate self arithmetic edge');
            call(group.id, 1, 'separate self arithmetic edge', 1, {form: f.id, edge});
          }
        }
        for (const path of bank.consumer.paths) {
          const overrides = path.id === 0 ? [[0, 0x7fff], [1, 0], [2, 1], [7, 0xffff]] : [[0, 0xffff], [1, 0xff00], [2, 0x100], [7, 0xffff]];
          input(0, seedHex(path.pc, 0xcd7, 1, overrides), 'continuous consumer seed'); call(group.id, 9, 'continuous consumer', 8, {path: path.id});
          input(0, seedHex(path.pc, 0xcd7, 1, overrides), 'split consumer seed');
          for (const budget of [1, 3, 4]) call(group.id, budget, 'split consumer', budget, {path: path.id});
          call(group.id, 1, 'consumer no replay', 0, {path: path.id});
        }
      }
    }
    if (owner !== 'standalone') {
      if (role === 'stale') {
        input(0, seedHex(profile.blocks[0][0], 0xcd7, 1), 'pre-write current owner'); call(profile.id, 1, 'pre-write current owner', 1);
        host('write8', [profile.blocks[0][0], 0x66]); call(profile.id, 1, 'same-byte stale direct', 0, {channel: 'direct'});
        if (owner === 'resident') call(profile.id, 1, 'same-byte stale dispatcher', 0);
      } else {
        input(0, seedHex(bank.consumer.paths[0].pc, 0xcd7, 1), 'zero cancel boundary seed');
        call(profile.id, 0, 'zero budget', 0); input(96, '01000000', 'set cancel'); call(profile.id, 9, 'cancel positive budget', 0); call(profile.id, 0, 'cancel zero budget', 0); input(96, '00000000', 'clear cancel');
        const bad = Buffer.alloc(entries ? 8 : 16); bad.writeUInt32LE(profile.blocks[0][0]); if (!entries) bad.writeUInt32LE(4, 4);
        bad.writeUInt32LE(bank.invalid_pc, entries ? 4 : 8); if (!entries) bad.writeUInt32LE(2, 12);
        input(140, bad.toString('hex'), 'late valid WORD then UD2 descriptors');
        host(owner === 'resident' ? entries ? 'compile_resident_entries' : 'compile_resident' : entries ? 'compile_entries' : 'compile', entries ? [2, 0] : [2], {group: 'late-failure', allowed_failure: true});
        input(0, seedHex(profile.blocks[0][0], 0xcd7, 1), 'prior owner survival'); call(profile.id, 1, 'prior owner survival', 1);
        add('guard', {group: profile.id, wrong_key: true});
      }
    }
    add('close'); if (owner !== 'standalone') add('closed_call', {group: profile.id, channel: 'direct', budget: 1, planned_retired: 0});
  }
  context('replacement', false); context('resident', false);
  context('standalone', false); context('standalone', true); context('replacement', true); context('resident', true);
  context('replacement', false, 'stale'); context('resident', false, 'stale');
  const generated = actions.filter(a => a.kind === 'call' || a.kind === 'closed_call');
  const count = (kind, test = () => true) => actions.filter(a => a.kind === kind && test(a)).length;
  const counts = {contexts: contexts.length, actions: actions.length, generated_calls: generated.length,
    planned_retired: generated.reduce((sum, a) => sum + a.planned_retired, 0), positive_calls: generated.filter(a => a.planned_retired > 0).length,
    zero_retirement_calls: generated.filter(a => a.planned_retired === 0).length, inputs: count('input'), host_calls: count('host'), guard_calls: count('guard'), tables: count('table'),
    child_modules: count('module', a => a.target === 'child'), dispatchers: count('module', a => a.target === 'dispatcher'),
    frames: actions.reduce((sum, a) => sum + (a.kind === 'closed_call' ? 0 : a.kind === 'open' || a.kind === 'close' ? 1 : 2), 0),
    checkpoints: generated.length + count('guard') + count('close') + 1, capture_files: 5};
  counts.physical_files = 9 + counts.child_modules + counts.dispatchers;
  counts.journal_rows = counts.frames + counts.actions + counts.child_modules + counts.dispatchers;
  counts.host_api_calls = contexts.length * 3 + counts.host_calls + counts.guard_calls + count('host', a => a.name.startsWith('compile') && contexts[a.context - 1].owner === 'replacement') * 3;
  assert.equal(counts.generated_calls, 703); assert.equal(counts.planned_retired, 814); assert.equal(counts.positive_calls, 670); assert.equal(counts.zero_retirement_calls, 33);
  assert.equal(counts.child_modules + counts.dispatchers, 14); assert.ok(counts.frames <= 6000);
  return {schema_version: 1, arena_bytes: SIZE, code_address: CODE, bank, contexts, actions, counts,
    capture_files: ['bank.x86', 'initial-arena.bin', 'capture.json', 'profile.wasm', 'entry-profile.wasm']};
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  assert.equal(process.argv[2], '--plan'); const output = resolve(process.argv[3]); mkdirSync(output, {recursive: true});
  const plan = makePlan(); writeFileSync(resolve(output, 'plan.json'), JSON.stringify(plan, null, 2) + '\n', {flag: 'wx'});
  writeFileSync(resolve(output, 'bank.x86'), Buffer.from(plan.bank.hex, 'hex'), {flag: 'wx'}); console.log(JSON.stringify(plan.counts));
}
