import assert from 'node:assert/strict';
import {mkdirSync, writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

export const SIZE = 4364, CODE = 0x1000, BANK_BYTES = 8192;
export const FLAGS = [2, 0xcd7], BOUNDARIES = [0, 1, 16, 17, 31];

function record(magic, size, fields = [], version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic);
  bytes.writeUInt32LE(0x10000 + version, 4); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, i) => bytes.writeUInt32LE(value >>> 0, 16 + i * 4)); return bytes;
}
export function seedHex(pc, flags = 2, pattern = 0, destination = 0, source = 2, cl = null) {
  const lows = pattern === 0 ? [0x8001, 0xa500, 0, 0x7fff, 0xaaaa, 0x5555, 0x4000, 0xffff]
    : [0xffff, 0x5a80, 0x8001, 0x4000, 0x5555, 0xaaaa, 0x7fff, 0xffff];
  const registers = lows.map((low, i) => (((0xa101 + pattern * 0x111 + i * 0x101) << 16) | low) >>> 0);
  const pair = pattern === 0 ? [0x8001, 0] : [0xffff, 0x8001];
  registers[destination] = ((registers[destination] & 0xffff0000) | pair[0]) >>> 0;
  registers[source] = ((registers[source] & 0xffff0000) | pair[1]) >>> 0;
  if (cl !== null) registers[1] = ((registers[1] & 0xffffff00) | cl) >>> 0;
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
  const bytes = Buffer.alloc(BANK_BYTES, 0xcc), forms = [], groups = []; let at = 0;
  function append(family, direction, destination, source, raw, group) {
    const opcode = direction === 'left' ? family === 'immediate' ? 0xa4 : 0xa5 : family === 'immediate' ? 0xac : 0xad;
    const instruction = [0x66, 0x0f, opcode, 0xc0 | (source << 3) | destination, ...(family === 'immediate' ? [raw] : [])];
    const form = {id: forms.length, family, direction, destination, source, ...(family === 'immediate' ? {raw} : {}),
      group, pc: CODE + at, length: instruction.length, hex: Buffer.from(instruction).toString('hex')};
    bytes.set(instruction, at); at += instruction.length; forms.push(form); return form.id;
  }
  function cohorts(role, descriptors) {
    for (let first = 0; first < descriptors.length; first += 60) {
      const id = `${role}-${first / 60}`, start = at, ids = [];
      for (const [family, direction, destination, source, raw] of descriptors.slice(first, first + 60)) ids.push(append(family, direction, destination, source, raw, id));
      bytes.set([0xeb, 0xfe], at); at += 2;
      groups.push({id, entries: false, blocks: [[CODE + start, at - start]], forms: ids, instructions: ids.length + 1});
    }
  }
  const raw = [];
  for (const direction of ['left', 'right']) for (let count = 0; count < 256; count++) raw.push(['immediate', direction, 0, 2, count]);
  for (const direction of ['left', 'right']) raw.push(['cl', direction, 0, 2]);
  cohorts('raw', raw);
  const aliases = [];
  for (const direction of ['left', 'right']) for (let destination = 0; destination < 8; destination++) for (let source = 0; source < 8; source++) {
    for (const count of [1, 16]) aliases.push(['immediate', direction, destination, source, count]);
    aliases.push(['cl', direction, destination, source]);
  }
  cohorts('alias', aliases);
  assert.equal(at, 4392);
  at = 0x1400; const profileStart = at, profileForms = [];
  for (const direction of ['left', 'right']) {
    for (const count of BOUNDARIES) profileForms.push(append('immediate', direction, 0, 2, count, 'profile'));
    profileForms.push(append('cl', direction, 0, 2, undefined, 'profile'));
  }
  bytes.set([0xeb, 0xfe], at); at += 2;
  const blocks = [[CODE + profileStart, at - profileStart]], paths = [];
  for (const [index, direction] of ['left', 'right'].entries()) {
    const start = 0x1800 + index * 0x80, opcode = direction === 'left' ? 'a4' : 'ac';
    const pieces = [`660f${opcode}d0110f94c40f9ac17502`, '7b02', '6683d7000f92c7eb00'].map(hex => Buffer.from(hex, 'hex'));
    let cursor = start; const pathBlocks = [];
    for (let piece = 0; piece < pieces.length; piece++) {
      bytes.set(pieces[piece], cursor); pathBlocks.push([CODE + cursor, pieces[piece].length]); cursor += pieces[piece].length;
      if (piece !== pieces.length - 1) {bytes.set([0x0f, 0x0b], cursor); cursor += 2;}
    }
    assert.equal(cursor - start, 28); blocks.push(...pathBlocks);
    paths.push({id: index, direction, pc: CODE + start, end_pc: CODE + cursor, blocks: pathBlocks, instructions: 8});
  }
  groups.push({id: 'profile', entries: false, blocks, forms: profileForms, instructions: 29});
  groups.push({id: 'entry-profile', entries: true, blocks, forms: profileForms, instructions: 29});
  bytes.set([0x0f, 0x0b], 0x1f00);
  assert.equal(forms.length, 910); assert.equal(groups.length, 18);
  return {hex: bytes.toString('hex'), forms, groups, consumer: {paths}, invalid_pc: CODE + 0x1f00};
}
export function makePlan() {
  const bank = bankPlan(), contexts = [], actions = []; let next = 0;
  const form = (role, family, direction, raw) => bank.forms.find(f => f.group.startsWith(`${role}-`) && f.family === family && f.direction === direction && (family === 'cl' || f.raw === raw));
  const observations = [];
  for (const direction of ['left', 'right']) for (const family of ['immediate', 'cl']) for (let raw = 0; raw < 32; raw++) for (const flags of FLAGS) for (let pattern = 0; pattern < 2; pattern++) observations.push({form: form('raw', family, direction, raw).id, flags, pattern, cl: family === 'cl' ? raw : null, label: 'masked counts flags operands'});
  for (const direction of ['left', 'right']) for (const family of ['immediate', 'cl']) for (let raw = 32; raw < 256; raw++) observations.push({form: form('raw', family, direction, raw).id, flags: FLAGS[raw % 2], pattern: raw % 2, cl: family === 'cl' ? raw : null, label: 'remaining raw count union'});
  const aliasRows = bank.forms.filter(f => f.group.startsWith('alias-')).flatMap(f => (f.family === 'cl' ? [1, 16] : [f.raw]).map(raw => ({form: f.id, flags: raw === 1 ? 0xcd7 : 2, pattern: raw === 1 ? 1 : 0, cl: f.family === 'cl' ? raw : null, label: 'old-state alias boundary'})));
  assert.equal(observations.length, 1408); assert.equal(aliasRows.length, 512);
  function context(owner, entries, role = 'normal') {
    const spec = {id: contexts.length + 1, owner, entries, role, key: [0x57440000 + contexts.length + 1, 0x53484946], pages: 2}; contexts.push(spec);
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
    const selected = role === 'stale' || entries || owner === 'standalone' ? [profile]
      : [...bank.groups.filter(g => g.id.startsWith(owner === 'replacement' ? 'raw-' : 'alias-')), profile];
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
      const rows = group.id.startsWith('raw-') ? observations.filter(row => bank.forms[row.form].group === group.id)
        : group.id.startsWith('alias-') ? aliasRows.filter(row => bank.forms[row.form].group === group.id)
          : profile.forms.flatMap(id => {const f = bank.forms[id]; return (f.family === 'cl' ? BOUNDARIES : [f.raw]).map(raw => ({form: id, flags: raw === 0 || raw === 16 ? 0xcd7 : 2, pattern: raw === 16 ? 0 : 1, cl: f.family === 'cl' ? raw : null, label: 'six-profile count boundary'}));});
      for (const row of rows) {const f = bank.forms[row.form]; input(0, seedHex(f.pc, row.flags, row.pattern, f.destination, f.source, row.cl), row.label); call(group.id, 1, row.label, 1, {form: f.id});}
      if (group.id === profile.id) for (const path of bank.consumer.paths) {
        input(0, seedHex(path.pc, 0xcd7, 1), 'continuous consumer seed'); call(group.id, 9, 'continuous consumer', 8, {path: path.id});
        input(0, seedHex(path.pc, 0xcd7, 1), 'split consumer seed');
        for (const budget of [1, 3, 4]) call(group.id, budget, 'split consumer', budget, {path: path.id});
        call(group.id, 1, 'consumer no replay', 0, {path: path.id});
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
        const bad = Buffer.alloc(entries ? 8 : 16); bad.writeUInt32LE(profile.blocks[0][0]); if (!entries) bad.writeUInt32LE(5, 4);
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
  // keep one resident slot free until the late compiler-refusal control has run.
  const finalAliasCompile = actions.findIndex(a => a.context === 2 && a.kind === 'host' && a.group === 'alias-6');
  const profileCompile = actions.findIndex(a => a.context === 2 && a.kind === 'host' && a.group === 'profile');
  assert.equal(actions[finalAliasCompile - 1].label, 'exact compiler descriptors');
  assert.equal(actions[profileCompile - 1].label, 'exact compiler descriptors');
  const deferredAlias = actions.splice(finalAliasCompile - 1, profileCompile - finalAliasCompile);
  assert.ok(deferredAlias.every(a => a.context === 2));
  const residentClose = actions.findIndex(a => a.context === 2 && a.kind === 'close');
  actions.splice(residentClose, 0, ...deferredAlias);
  actions.forEach((action, id) => {action.id = id;});
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
  assert.equal(counts.generated_calls, 2127); assert.equal(counts.planned_retired, 2238); assert.equal(counts.positive_calls, 2094); assert.equal(counts.zero_retirement_calls, 33);
  assert.equal(counts.child_modules + counts.dispatchers, 26); assert.ok(counts.frames <= 12000);
  return {schema_version: 1, arena_bytes: SIZE, code_address: CODE, bank, contexts, actions, counts,
    capture_files: ['bank.x86', 'initial-arena.bin', 'capture.json', 'profile.wasm', 'entry-profile.wasm']};
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  assert.equal(process.argv[2], '--plan'); const output = resolve(process.argv[3]); mkdirSync(output, {recursive: true});
  const plan = makePlan(); writeFileSync(resolve(output, 'plan.json'), JSON.stringify(plan, null, 2) + '\n', {flag: 'wx'});
  writeFileSync(resolve(output, 'bank.x86'), Buffer.from(plan.bank.hex, 'hex'), {flag: 'wx'}); console.log(JSON.stringify(plan.counts));
}
