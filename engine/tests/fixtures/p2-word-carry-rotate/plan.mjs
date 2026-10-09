import assert from 'node:assert/strict';
import {mkdirSync, writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

export const SIZE = 4364, CODE = 0x1000;
export const FLAGS = [2, 3, 0xcd6, 0xcd7];
export const HIGH_RAW = [32, 33, 49, 50, 255, 0x66, 0x67, 0xf0, 0xf2, 0xf3];
export const CX_RAW = [...Array.from({length: 32}, (_, i) => i), 32, 33, 49, 50, 255];
const EDGES = [0, 1, 0x7fff, 0x8000, 0xffff, 0x8001, 0x4000, 0xaaaa];

function record(magic, length, fields = [], version = 1) {
  const b = Buffer.alloc(length); b.write(magic); b.writeUInt16LE(version, 4);
  b.writeUInt16LE(1, 6); b.writeUInt32LE(length, 8);
  fields.forEach((n, i) => b.writeUInt32LE(n >>> 0, 16 + i * 4)); return b;
}

export function initialHex() {
  const b = Buffer.from(Array.from({length: SIZE}, (_, i) => (i * 37 + 19) & 255));
  const fp = record('R3FP', 128);
  for (const [at, n] of [[16, 0x27f], [18, 0x2840], [20, 0xaaaa], [22, 0x345], [32, 0x33], [34, 0x2b]]) fp.writeUInt16LE(n, at);
  fp.writeUInt32LE(0x12345678, 24); fp.writeUInt32LE(0x9abcdef0, 28);
  for (let i = 40; i < 120; i++) fp[i] = (i * 29 + 7) & 255;
  b.set(fp, 4236); b.set(Buffer.from(seedHex(CODE, 0, 2, 0, 0, 0), 'hex'), 0); return b.toString('hex');
}

export function seedHex(pc, salt, flags, alias, value, cl = 33) {
  const registers = Array.from({length: 8}, (_, i) => ((((0xa100 + i + salt % 31) & 0xffff) * 65536) + EDGES[(salt + i) % 8]) >>> 0);
  registers[1] = ((registers[1] & 0xffff0000) | 0xa500 | cl) >>> 0;
  registers[alias] = ((registers[alias] & 0xffff0000) | value) >>> 0;
  const b = Buffer.alloc(140);
  b.set(record('R3ST', 56, [...registers, pc, flags]));
  b.set(record('R3EX', 40, [3, 0, 0, 0, 0, 0], 3), 56);
  b.set(record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]), 100);
  return b.toString('hex');
}

export function bankPlan() {
  const bank = Buffer.alloc(4096, 0xcc), forms = [], groups = [];
  let at = 0, groupStart = 0, groupFirst = 0;
  function append(family, kind, alias, raw) {
    const bytes = [0x66, {one: 0xd1, immediate: 0xc1, cl: 0xd3}[family], 0xd0 + kind * 8 + alias];
    if (family === 'immediate') bytes.push(raw);
    const form = {id: forms.length, family, kind, alias, ...(raw === undefined ? {} : {raw}), pc: CODE + at, length: bytes.length, hex: Buffer.from(bytes).toString('hex'), group: `main-${groups.length}`};
    bank.set(bytes, at); at += bytes.length; forms.push(form);
    if (forms.length === 48 || forms.length === 96 || forms.length === 130) {
      groups.push({id: `main-${groups.length}`, entries: false, blocks: [[CODE + groupStart, at - groupStart]], forms: forms.slice(groupFirst).map(f => f.id), instructions: forms.length - groupFirst});
      bank.set([0xeb, 0xfe], at); at += 2; groupStart = at; groupFirst = forms.length;
    }
  }
  for (const family of ['one', 'immediate', 'cl']) {
    if (family === 'immediate') {
      for (let kind = 0; kind < 2; kind++) for (let raw = 0; raw < 32; raw++) append(family, kind, 0, raw);
      for (let kind = 0; kind < 2; kind++) for (const raw of HIGH_RAW) append(family, kind, 0, raw);
      for (let kind = 0; kind < 2; kind++) for (let alias = 1; alias < 8; alias++) append(family, kind, alias, 18);
    } else for (let kind = 0; kind < 2; kind++) for (let alias = 0; alias < 8; alias++) append(family, kind, alias);
  }
  assert.equal(forms.length, 130); assert.deepEqual(groups.map(g => g.instructions), [48, 48, 34]);
  const pieces = ['66b9218066d3d166d3d966d3d10f92c20f90c366b8008066d1d00f92c60f90c76683d50066b8008066c1d8120f90c07102',
    '66b8008066d1d07002', '0f90c2d0d0d1deebfe'].map(h => Buffer.from(h, 'hex'));
  let chainAt = 2048; const blocks = [];
  for (let i = 0; i < pieces.length; i++) {
    blocks.push([CODE + chainAt, pieces[i].length]); bank.set(pieces[i], chainAt); chainAt += pieces[i].length;
    if (i < 2) {bank.set([0x0f, 0x0b], chainAt); chainAt += 2;}
  }
  groups.push({id: 'consumer', entries: false, blocks, forms: [], instructions: 22});
  at = 2560; const entryForms = [];
  for (const family of ['one', 'immediate', 'cl']) for (let kind = 0; kind < 2; kind++) for (let alias = 0; alias < 8; alias++) {
    const bytes = [0x66, {one: 0xd1, immediate: 0xc1, cl: 0xd3}[family], 0xd0 + kind * 8 + alias, ...(family === 'immediate' ? [18] : [])];
    entryForms.push({id: forms.length + entryForms.length, family, kind, alias, ...(family === 'immediate' ? {raw: 18} : {}), pc: CODE + at, length: bytes.length, hex: Buffer.from(bytes).toString('hex'), group: 'entry-forms'});
    bank.set(bytes, at); at += bytes.length;
  }
  bank.set([0xeb, 0xfe], at);
  groups.push({id: 'entry-forms', entries: true, blocks: [[CODE + 2560, at + 2 - 2560]], forms: entryForms.map(f => f.id), instructions: 49});
  groups.push({id: 'entry-consumer', entries: true, blocks, forms: [], instructions: 22});
  bank.set([0x0f, 0x0b], 3840);
  return {hex: bank.toString('hex'), forms: [...forms, ...entryForms], groups, consumer: {blocks, instructions: 22, pc: blocks[0][0], first_carry: blocks[0][0] + 4}, invalid_pc: CODE + 3840};
}

export function makePlan() {
  const bank = bankPlan(), actions = [], contexts = []; let actionId = 0;
  const form = (family, kind, alias, raw) => bank.forms.find(f => f.group.startsWith('main-') && f.family === family && f.kind === kind && f.alias === alias && (family !== 'immediate' || f.raw === raw));
  const observations = [];
  function observe(f, flags, value, cl, category) {
    observations.push({form: f.id, flags, value, cl, category});
  }
  for (let kind = 0; kind < 2; kind++) for (let raw = 0; raw < 32; raw++) for (let bit = 0; bit < 17; bit++) observe(form('immediate', kind, 0, raw), 0xcd6 | Number(bit === 16), bit === 16 ? 0 : 2 ** bit, 33, 'basis');
  for (const family of ['one', 'immediate', 'cl']) for (let kind = 0; kind < 2; kind++) for (let alias = 0; alias < 8; alias++) for (const flags of FLAGS) observe(form(family, kind, alias, 18), flags, EDGES[alias], 33, 'structural');
  for (let kind = 0; kind < 2; kind++) for (const raw of HIGH_RAW) for (const flags of FLAGS) observe(form('immediate', kind, 0, raw), flags, kind === 0 ? 0x4000 : 0, 33, 'high-raw');
  for (let kind = 0; kind < 2; kind++) for (const raw of CX_RAW) for (const flags of FLAGS) observe(form('cl', kind, 1), flags, 0xa500 | raw, raw, 'cx-cl');
  assert.equal(observations.length, 1656);
  for (let kind = 0; kind < 2; kind++) {
    for (const raw of [1, 18]) observe(form('immediate', kind, 0, raw), kind === 0 ? 2 : 3, kind === 0 ? 0x4000 : 0, 33, 'anchor-defined-undefined');
    for (const raw of [17, 49, 32]) observe(form('immediate', kind, 0, raw), 0xcd7, 0x8001, 33, 'anchor-zero-distance');
  }
  function context(owner, entries) {
    const c = {id: contexts.length + 1, owner, entries, key: [0x57430000 + contexts.length + 1, 0x574f5244], pages: 1}; contexts.push(c);
    const add = (kind, fields = {}) => actions.push({id: actionId++, context: c.id, kind, ...fields});
    const input = (offset, hex, label) => add('input', {offset, hex, label});
    const host = (name, args, fields) => add('host', {name, args, ...fields});
    const call = (group, budget, label, fields = {}) => add('call', {group, channel: owner === 'resident' && !entries ? 'dispatch' : 'direct', budget, label, ...fields});
    add('open'); input(0, initialHex(), 'full authored nondefault arena');
    if (owner !== 'standalone') {host('map', [CODE, 1, 7]); input(140, bank.hex, 'literal bank upload'); host('upload', [CODE, 4096]);}
    const selected = bank.groups.filter(g => entries ? g.entries : !g.entries);
    for (const [slot, g] of selected.entries()) {
      if (owner !== 'standalone') {
        const request = Buffer.alloc(g.blocks.length * (entries ? 4 : 8));
        g.blocks.forEach(([pc, length], i) => {request.writeUInt32LE(pc, i * (entries ? 4 : 8)); if (!entries) request.writeUInt32LE(length, i * 8 + 4);});
        input(140, request.toString('hex'), 'exact compiler descriptors');
        host(owner === 'resident' ? entries ? 'compile_resident_entries' : 'compile_resident' : entries ? 'compile_entries' : 'compile', entries ? [g.blocks.length, 0] : [g.blocks.length], {group: g.id});
      }
      add('module', {target: 'child', group: g.id, file: `${c.id}-${g.id}.wasm`, capture: owner === 'standalone' ? `${g.id}.wasm` : null, slot});
      if (owner === 'resident') {
        add('table', {group: g.id, slot}); host('acknowledge_resident_installation', ['key_low', 'key_high', 'unit_low', 'unit_high', slot], {group: g.id});
        if (slot === 0) {host('dispatcher_module', ['key_low', 'key_high']); add('module', {target: 'dispatcher', group: 'dispatcher', file: `${c.id}-dispatcher.wasm`, slot: null, capture: null});}
      }
      const cases = entries ? bank.forms.filter(f => f.group === g.id).map((f, i) => ({form: f.id, flags: FLAGS[i % 4], value: EDGES[f.alias], cl: 33, category: 'entry-structural'})) : observations.filter(o => bank.forms[o.form].group === g.id);
      for (const [i, row] of cases.entries()) {
        const f = bank.forms[row.form]; input(0, seedHex(f.pc, i, row.flags, f.alias, row.value, row.cl), row.category);
        call(g.id, 1, row.category, {form: f.id});
      }
      if (g.forms.length === 0) {
        for (const flags of [2, 0xcd7]) {
          input(0, seedHex(bank.consumer.pc, flags, flags, 5, 0xffff), 'continuous consumer seed');
          for (let i = 0; i < 22; i++) call(g.id, 1, 'continuous consumer', {step: i});
        }
        input(0, seedHex(bank.consumer.pc, 17, 0xcd7, 5, 0xffff), 'budget and cancel seed');
        call(g.id, 0, 'zero budget'); call(g.id, 3, 'prefix budget'); input(96, '01000000', 'set cancel');
        call(g.id, 5, 'cancel boundary'); input(96, '00000000', 'clear cancel'); call(g.id, 19, 'unreset continuation');
      }
    }
    const last = selected.at(-1);
    if (owner !== 'standalone') {
      const bad = Buffer.alloc(entries ? 8 : 16); bad.writeUInt32LE(bank.consumer.first_carry, 0);
      if (!entries) bad.writeUInt32LE(3, 4);
      bad.writeUInt32LE(bank.invalid_pc, entries ? 4 : 8); if (!entries) bad.writeUInt32LE(2, 12);
      input(140, bad.toString('hex'), 'late complete unsupported candidate');
      host(owner === 'resident' ? entries ? 'compile_resident_entries' : 'compile_resident' : entries ? 'compile_entries' : 'compile', entries ? [2, 0] : [2], {group: 'late-failure', allowed_failure: true});
      input(0, seedHex(bank.consumer.first_carry, 3, 0xcd7, 1, 0x8021), 'prior owner survival'); call(last.id, 1, 'prior owner survival');
      add('guard', {group: last.id, wrong_key: true});
      host('write8', [CODE, 0x66]); call(last.id, 1, 'same-byte stale direct', {channel: 'direct'});
      if (owner === 'resident') call(last.id, 1, 'same-byte stale dispatcher', {channel: 'dispatch'});
    }
    add('close'); if (owner !== 'standalone') add('closed_call', {group: last.id, budget: 1});
  }
  for (const entries of [false, true]) for (const owner of ['standalone', 'replacement', 'resident']) context(owner, entries);
  const calls = actions.filter(a => a.kind === 'call'), generated = actions.filter(a => ['call', 'closed_call'].includes(a.kind));
  const positive = calls.filter(a => a.budget > 0 && a.label !== 'cancel boundary' && !a.label.startsWith('same-byte stale'));
  const counts = {contexts: contexts.length, actions: actions.length, generated_calls: generated.length,
    positive_calls: positive.length, zero_retirement_calls: generated.length - positive.length,
    expected_instructions: positive.reduce((n, a) => n + a.budget, 0),
    single_op_base: 4968, literal_anchors: 30, entry_structural: 144, continuous_calls: calls.filter(a => a.label === 'continuous consumer').length,
    guard_calls: actions.filter(a => a.kind === 'guard').length, host_calls: actions.filter(a => a.kind === 'host').length,
    child_modules: actions.filter(a => a.kind === 'module' && a.target === 'child').length,
    dispatchers: actions.filter(a => a.kind === 'module' && a.target === 'dispatcher').length,
    native_standalone_modules: 6, frames: actions.reduce((n, a) => n + (a.kind === 'closed_call' ? 0 : ['open', 'close'].includes(a.kind) ? 1 : 2), 0),
    checkpoints: generated.length + actions.filter(a => ['guard', 'close'].includes(a.kind)).length + 1,
    capture_files: 9, physical_files: 35};
  assert.equal(counts.generated_calls, 5444); assert.equal(counts.continuous_calls, 264);
  assert.equal(counts.positive_calls, 5422); assert.equal(counts.zero_retirement_calls, 22); assert.equal(counts.expected_instructions, 5542);
  return {schema_version: 1, arena_bytes: SIZE, code_address: CODE, bank, contexts, actions, counts};
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  assert.equal(process.argv[2], '--plan'); const out = resolve(process.argv[3]); mkdirSync(out, {recursive: true});
  const plan = makePlan(); writeFileSync(resolve(out, 'plan.json'), JSON.stringify(plan, null, 2) + '\n', {flag: 'wx'});
  writeFileSync(resolve(out, 'bank.x86'), Buffer.from(plan.bank.hex, 'hex'), {flag: 'wx'}); console.log(JSON.stringify(plan.counts));
}
