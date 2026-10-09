import assert from 'node:assert/strict';
import {mkdirSync, writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

export const SIZE = 4364, CODE = 0x1000, DATA = 0x4000, TOP = 0xfffff000;
const LOW = [[0, 1, 0x7fff, 0xffff, 0x8000, 0x1234, 0xaaaa, 0x5555],
  [0x7fff, 0xffff, 0, 0x8000, 1, 0xaaaa, 0x5555, 0x1234],
  [0x8000, 0x7fff, 0xffff, 0x7fff, 0x1234, 1, 0x5555, 0xaaaa]];
const TRUE = [0x802, 2, 3, 2, 0x42, 2, 3, 2, 0x82, 2, 6, 2, 0x82, 2, 0x42, 2];
const FALSE = [2, 0x802, 2, 3, 2, 0x42, 2, 3, 2, 0x82, 2, 6, 2, 0x82, 2, 0x42];
function record(magic, length, fields = [], version = 1) {
  const bytes = Buffer.alloc(length); bytes.write(magic); bytes.writeUInt32LE(0x10000 + version, 4); bytes.writeUInt32LE(length, 8);
  fields.forEach((n, i) => bytes.writeUInt32LE(n >>> 0, 16 + 4 * i)); return bytes;
}
function word(n) {const b = Buffer.alloc(4); b.writeUInt32LE(n >>> 0); return [...b];}
export function seedHex(pc, flags = 2, pattern = 0, overrides = {}) {
  const registers = LOW[pattern].map((low, i) => (((0xa101 + pattern * 0x111 + i * 0x101) << 16) | low) >>> 0);
  for (const [index, n] of Object.entries(overrides)) registers[Number(index)] = n >>> 0;
  const b = Buffer.alloc(140); record('R3ST', 56, [...registers, pc, flags]).copy(b);
  record('R3EX', 40, [3, 0, 0, 0, 0, 0], 3).copy(b, 56); record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]).copy(b, 100);
  return b.toString('hex');
}
export function initialHex() {
  const b = Buffer.from(Array.from({length: SIZE}, (_, i) => (37 * i + 19) & 255)); Buffer.from(seedHex(CODE), 'hex').copy(b);
  const fp = record('R3FP', 128);
  for (const [at, n] of [[16, 0x27f], [18, 0x2840], [20, 0xaaaa], [22, 0x345], [32, 0x33], [34, 0x2b]]) fp.writeUInt16LE(n, at);
  fp.writeUInt32LE(0x12345678, 24); fp.writeUInt32LE(0x9abcdef0, 28);
  for (let i = 40; i < 120; i++) fp[i] = (29 * i + 7) & 255; fp.copy(b, 4236); return b.toString('hex');
}
export function dataHex() {
  const b = Buffer.from(Array.from({length: 4096}, (_, i) => (73 * i + 11) & 255)); b.set([1, 0x80, 0x34, 0x12]); return b.toString('hex');
}
export function bankPlan() {
  const b = Buffer.alloc(4096, 0xcc), forms = [], blocks = []; let at = 0;
  function form(axis, cc, destination, tail, overrides = {}) {
    const raw = [0x66, 0x0f, 0x40 + cc, tail[0] | destination << 3, ...tail.slice(1)];
    const f = {id: forms.length, axis, cc, destination, pc: CODE + at, length: raw.length, hex: Buffer.from(raw).toString('hex'), overrides};
    forms.push(f); b.set(raw, at); at += raw.length; return f;
  }
  function end(start) {b.set([0xeb, 0xfe], at); at += 2; blocks.push([CODE + start, at - start]);}
  for (let cc = 0; cc < 16; cc++) form('truth', cc, cc % 8, [5, ...word(DATA + cc * 2)]); end(0);
  at = 0x100;
  for (let destination = 0; destination < 8; destination++) {
    const tail = destination === 4 ? [4, 0x24] : destination === 5 ? [0x45, 0] : [destination];
    form('base', 4, destination, tail, {[destination]: DATA});
    if (destination !== 4) {
      form('index', 4, destination, [4, destination << 3 | 5, ...word(DATA)], {[destination]: 0});
      form('both', 4, destination, [destination === 5 ? 0x44 : 4, destination << 3 | destination, ...(destination === 5 ? [0] : [])], {[destination]: DATA / 2});
    }
  }
  end(0x100); assert.equal(forms.filter(f => ['base', 'index', 'both'].includes(f.axis)).length, 22);
  at = 0x200;
  const wraps = [
    [3, [0x83, ...word(0x5000)], {3: 0xfffff000}],
    [6, [4, 0xf5, ...word(DATA)], {6: 0x20000000}],
    [1, [0x84, 0x0b, ...word(DATA)], {3: 0x80000000, 1: 0x80000000}],
    [0, [0x84, 0x30, ...word(DATA)], {0: 0xffffffff, 6: 1}],
    [5, [0x84, 0x3d, ...word(DATA)], {5: 1, 7: 0xffffffff}],
    [2, [4, 0x95, ...word(DATA)], {2: 0x40000000}],
    [3, [0x43, 0xff], {3: DATA + 1}],
    [4, [0x44, 0x24, 0xff], {4: DATA + 1}],
  ];
  for (const [destination, tail, overrides] of wraps) form('wrap', 4, destination, tail, overrides); end(0x200);
  const mainBlocks = [...blocks]; at = 0x300; const faultForms = [];
  for (let cc = 0; cc < 16; cc++) {
    const prefix = CODE + at; b[at++] = 0x90; const f = form('fault', cc, cc % 8, [3], {3: DATA}); faultForms.push({...f, prefix});
  }
  end(0x300); const faultBlock = blocks.at(-1), consumerBlocks = [], paths = [];
  const pieces = ['6639c8660f4c13660f4d6b020f4cf50f90c40f92c17c02', '6639c8660f4213660f436b020f42f50f92c40f90c17202'];
  for (let path = 0; path < 2; path++) {
    const offset = 0x800 + path * 64, piece = Buffer.from(pieces[path], 'hex'); assert.equal(piece.length, 23);
    piece.copy(b, offset); b.set([0x0f, 0x0b, 0xeb, 0], offset + 23);
    consumerBlocks.push([CODE + offset, 23], [CODE + offset + 25, 2]);
    paths.push({id: path, pc: CODE + offset, end_pc: CODE + offset + 27, instructions: 8});
  }
  b.set([0x0f, 0x0b], 0xf00);
  return {hex: b.toString('hex'), forms, fault_forms: faultForms, consumer: {paths, blocks: consumerBlocks}, invalid_pc: CODE + 0xf00,
    groups: [{id: 'main', blocks: mainBlocks, instructions: 49}, {id: 'control', blocks: [faultBlock, ...consumerBlocks], instructions: 49},
      {id: 'helper', blocks: [[faultForms[4].prefix, 5]], instructions: 2}, {id: 'keeper', blocks: [[paths[0].pc + 3, 4]], instructions: 1}]};
}
export function helperCases() {
  const good = record('R3MH', 40, [0, 0x8001, 0, 0, 0, 2], 2), rows = [];
  const add = (id, status, mutate) => {const packet = Buffer.from(good); mutate(packet); rows.push({id, status, packet_hex: packet.toString('hex')});};
  add('status', 9, () => {}); add('version', 0, p => p.writeUInt32LE(0x10001, 4));
  add('width', 0, p => p.writeUInt32LE(4, 36)); add('value', 0, p => p.writeUInt32LE(0x10000, 20));
  add('span', 0, p => {p.writeUInt32LE(1, 16); p.writeUInt32LE(0, 20); p.writeUInt32LE(1, 24); p.writeUInt32LE(DATA + 2, 28); p.writeUInt32LE(1, 32);});
  add('overflow', 0, p => {p.writeUInt32LE(1, 16); p.writeUInt32LE(0, 20); p.writeUInt32LE(3, 24); p.writeUInt32LE(DATA, 28); p.writeUInt32LE(1, 32);});
  return rows;
}
export function makePlan() {
  const bank = bankPlan(), data = dataHex(), actions = [], contexts = []; let next = 0;
  const groups = Object.fromEntries(bank.groups.map(g => [g.id, g]));
  function context(owner, entries, role = 'normal') {
    const spec = {id: contexts.length + 1, owner, entries, role, key: [0x57430000 + contexts.length + 1, 0x4d434d56], pages: 6}; contexts.push(spec);
    const add = (kind, fields = {}) => actions.push({id: next++, context: spec.id, kind, ...fields});
    const input = (offset, hex, label) => add('input', {offset, hex, label});
    const host = (name, args, fields = {}) => add('host', {name, args, ...fields});
    const call = (group, budget, label, retired, fields = {}) => add('call', {group, channel: owner === 'resident' && !entries ? 'dispatch' : 'direct', budget, label, planned_retired: retired, ...fields});
    const seed = (pc, flags, overrides = {}, pattern = 0, label = 'CPU seed') => input(0, seedHex(pc, flags, pattern, overrides), label);
    const upload = (address, hex, label) => {input(140, hex, label); host('upload', [address, hex.length / 2]);};
    const page = (address, phase) => add('data', {address, count: 1024, file: `${spec.id}-${phase}-${address.toString(16)}.bin`, label: 'complete readable RAM page'});
    function compile(group, slot) {
      const request = Buffer.alloc(groups[group].blocks.length * (entries ? 4 : 8));
      groups[group].blocks.forEach(([pc, n], i) => {request.writeUInt32LE(pc, i * (entries ? 4 : 8)); if (!entries) request.writeUInt32LE(n, i * 8 + 4);});
      input(140, request.toString('hex'), 'compiler descriptors');
      host(owner === 'resident' ? entries ? 'compile_resident_entries' : 'compile_resident' : entries ? 'compile_entries' : 'compile', entries ? [groups[group].blocks.length, 0] : [groups[group].blocks.length], {group});
      add('module', {target: 'child', group, slot, file: `${spec.id}-${group}.wasm`, helper_case: null});
      if (owner === 'resident') {
        add('table', {group, slot}); host('acknowledge_resident_installation', ['key_low', 'key_high', 'unit_low', 'unit_high', slot], {group});
        if (slot === 0) {host('dispatcher_module', ['key_low', 'key_high']); add('module', {target: 'dispatcher', group: 'dispatcher', slot: null, file: `${spec.id}-dispatcher.wasm`, helper_case: null});}
      }
    }
    add('open'); input(0, initialHex(), 'full authored nondefault arena'); host('map', [CODE, 1, 7]); upload(CODE, bank.hex, 'complete literal bank');
    if (role !== 'stale') host('protect', [CODE, 1, 5]);
    host('map', [DATA, 1, 3]); upload(DATA, data, 'literal data page'); host('protect', [DATA, 1, 1]); page(DATA, 'initial');
    if (role === 'normal') {host('map', [TOP, 1, 3]); upload(TOP, data, 'literal top page'); host('protect', [TOP, 1, 1]); page(TOP, 'initial');}
    if (role === 'normal') {
      compile('main', 0);
      if (!entries) {
        for (let pattern = 0; pattern < 3; pattern++) for (let combination = 0; combination < 32; combination++) for (const f of bank.forms.filter(f => f.axis === 'truth')) {
          const flags = 2 | [1, 4, 0x40, 0x80, 0x800].reduce((n, bit, i) => n | ((combination >> i & 1) * bit), 0) | [0, 0x10, 0x400][pattern];
          seed(f.pc, flags, {}, pattern, 'predicate and sentinel FLAGS'); call('main', 1, 'predicate truth', 1, {form: f.id});
        }
        for (const f of bank.forms.filter(f => ['base', 'index', 'both', 'wrap'].includes(f.axis))) for (const flags of [2, 0x42]) {
          seed(f.pc, flags | 0x410, f.overrides); call('main', 1, f.axis, 1, {form: f.id});
        }
      } else for (const f of bank.forms.filter(f => f.axis === 'truth')) for (const flags of [FALSE[f.cc], TRUE[f.cc]]) {
        seed(f.pc, flags | [0x10, 0x400][f.cc % 2], {}, f.cc % 3); call('main', 1, 'entry predicate representative', 1, {form: f.id});
      }
      compile('control', 1);
      if (!entries) {
        const shapes = [{id: 'unmapped', address: 0x8000, map: 0x8000}, {id: 'none', address: DATA, permission: DATA, bits: 0},
          {id: 'write', address: DATA, permission: DATA, bits: 2}, {id: 'execute', address: DATA, permission: DATA, bits: 4},
          {id: 'cross-unmapped', address: DATA + 4095, map: DATA + 4096}, {id: 'cross-write', address: DATA + 4095, permission: DATA + 4096, bits: 2},
          {id: 'overflow', address: 0xffffffff}];
        for (const shape of shapes) {
          if (shape.id === 'cross-write') {host('map', [DATA + 4096, 1, 3]); upload(DATA + 4096, data, 'cross-page data');}
          const rows = [...bank.fault_forms.map(f => ({f, flags: FALSE[f.cc], label: 'false'})), {f: bank.fault_forms[4], flags: TRUE[4], label: 'true representative'}];
          for (const {f, flags, label} of rows) {
            if (shape.permission !== undefined) host('protect', [shape.permission, 1, shape.bits]);
            seed(f.prefix, flags | 0x410, {3: shape.address}); call('control', 20, `${shape.id} ${label} prefix fault`, 1, {form: f.id});
            call('control', 1, `${shape.id} unchanged retry`, 0, {form: f.id});
            if (shape.map !== undefined) {host('map', [shape.map, 1, 3]); upload(shape.map, data, 'data-only fault repair'); host('protect', [shape.map, 1, 1]);}
            else if (shape.permission !== undefined) host('protect', [shape.permission, 1, 1]);
            else input(28, Buffer.from(word(DATA)).toString('hex'), 'explicit overflow address-register repair');
            call('control', 1, `${shape.id} repair completion`, 1, {form: f.id});
            if (shape.map !== undefined) host('unmap', [shape.map, 1]);
          }
          if (shape.id === 'cross-write') host('unmap', [DATA + 4096, 1]);
        }
        for (const address of [DATA + 4094, 0xfffffffe]) for (const f of bank.fault_forms) for (const flags of [FALSE[f.cc], TRUE[f.cc]]) {
          seed(f.pc, flags | 0x410, {3: address}); call('control', 1, 'last-two-byte read', 1, {form: f.id});
        }
        host('map', [DATA + 4096, 1, 3]); upload(DATA + 4096, data, 'cross-page success data'); host('protect', [DATA + 4096, 1, 1]);
        for (const f of bank.fault_forms) for (const flags of [FALSE[f.cc], TRUE[f.cc]]) {seed(f.pc, flags | 0x410, {3: DATA + 4095}); call('control', 1, 'successful cross-page read', 1, {form: f.id});}
        page(DATA + 4096, 'cross-final'); host('unmap', [DATA + 4096, 1]);
      }
      for (const path of bank.consumer.paths) {
        const overrides = {0: (0xa1010000 | (path.id ? 0 : 0x8000)) >>> 0, 1: 0xa2020001, 3: DATA};
        seed(path.pc, 0xcd7, overrides); call('control', 9, 'continuous consumer', 8, {path: path.id});
        seed(path.pc, 0xcd7, overrides); for (const budget of [1, 3, 4]) call('control', budget, 'split consumer', budget, {path: path.id});
        call('control', 1, 'consumer no replay', 0, {path: path.id});
      }
      seed(bank.fault_forms[0].prefix, 2, {3: DATA}); call('control', 0, 'zero budget', 0);
      input(96, '01000000', 'set cancel'); call('control', 20, 'cancel positive budget', 0); call('control', 0, 'cancel zero budget', 0); input(96, '00000000', 'clear cancel');
      const first = bank.consumer.paths[0].pc + 3, request = Buffer.alloc(entries ? 8 : 16); request.writeUInt32LE(first);
      if (!entries) request.writeUInt32LE(4, 4); request.writeUInt32LE(bank.invalid_pc, entries ? 4 : 8); if (!entries) request.writeUInt32LE(2, 12);
      input(140, request.toString('hex'), 'late admitted memory then UD2');
      host(owner === 'resident' ? entries ? 'compile_resident_entries' : 'compile_resident' : entries ? 'compile_entries' : 'compile', entries ? [2, 0] : [2], {group: 'late-failure', allowed_failure: true});
      seed(first, 0xc16, {3: DATA}); call('control', 1, 'prior owner survival', 1); add('guard', {group: 'control', wrong_key: true});
    } else if (role === 'helper') {
      const request = Buffer.alloc(8); request.writeUInt32LE(bank.fault_forms[4].prefix); request.writeUInt32LE(5, 4);
      input(140, request.toString('hex'), 'inert helper module descriptors'); host('compile', [1], {group: 'helper'});
      for (const h of helperCases()) {
        add('module', {target: 'child', group: 'helper', slot: null, file: `${spec.id}-helper-${h.id}.wasm`, helper_case: h.id});
        seed(bank.fault_forms[4].prefix, 2, {3: DATA}); call('helper', 20, 'malformed inert helper ' + h.id, 1, {channel: 'direct', helper_case: h.id});
      }
    } else {
      compile('keeper', 0); const pc = bank.consumer.paths[0].pc + 3;
      seed(pc, 0xc16, {3: DATA}); call('keeper', 1, 'current before same-byte write', 1);
      seed(pc, 0xc16, {3: DATA}); host('write8', [CODE, 0x66]); call('keeper', 1, 'same-byte stale direct', 0, {channel: 'direct'});
      if (owner === 'resident') call('keeper', 1, 'same-byte stale dispatcher', 0, {channel: 'dispatch'});
    }
    page(DATA, 'final'); if (role === 'normal') page(TOP, 'final');
    add('close'); add('closed_call', {group: role === 'normal' ? 'control' : role === 'helper' ? 'helper' : 'keeper', channel: 'direct', budget: 1, label: 'cached closed function', planned_retired: 0});
  }
  for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) context(owner, entries);
  context('replacement', false, 'helper'); context('replacement', false, 'stale'); context('resident', false, 'stale');
  const count = kind => actions.filter(a => a.kind === kind).length;
  const calls = actions.filter(a => ['call', 'closed_call'].includes(a.kind));
  const frames = actions.reduce((n, a) => n + (['open', 'close'].includes(a.kind) ? 1 : a.kind === 'closed_call' ? 0 : 2), 0);
  const counts = {contexts: contexts.length, actions: actions.length, generated_calls: calls.length, planned_retired: calls.reduce((n, a) => n + a.planned_retired, 0),
    modules: count('module'), frames, raw_bytes: frames * SIZE, data_files: count('data'), diagnostic_reads: count('data') * 1024,
    inputs: count('input'), guards: count('guard'), tables: count('table'), files: 10 + count('module') + count('data'),
    checkpoints: calls.length + count('guard') + count('close') + count('data') + 1};
  assert.ok(counts.generated_calls <= 6000 && counts.modules <= 48 && counts.frames <= 25000);
  return {schema_version: 1, size: SIZE, code: CODE, data_address: DATA, top: TOP, data_hex: data, bank, helper_cases: helperCases(), contexts, actions, counts};
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const output = resolve(process.argv[2]); mkdirSync(output);
  const plan = makePlan(); writeFileSync(resolve(output, 'plan.json'), JSON.stringify(plan, null, 2) + '\n', {flag: 'wx'});
  console.log(JSON.stringify(plan.counts));
}
