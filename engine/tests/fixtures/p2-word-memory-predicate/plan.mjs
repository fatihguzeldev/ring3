import assert from 'node:assert/strict';
import {mkdirSync, writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

export const SIZE = 4364, CODE = 0x1000, DATA = 0x4000, TOP = 0xfffff000;
const LOW = [[0, 1, 0x7fff, 0xffff, 0x8000, 0x1234, 0xaaaa, 0x5555],
  [0x7fff, 0xffff, 0, 0x8000, 1, 0xaaaa, 0x5555, 0x1234],
  [0x8000, 0x7fff, 0xffff, 0x7fff, 0x1234, 1, 0x5555, 0xaaaa]];
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
const CMP_PAIRS = [[0, 0], [0, 1], [0x8000, 1], [0x7fff, 0xffff], [0x10, 1], [0x101, 1], [0xffff, 0xffff], [0x8000, 0x7fff]];
const TEST_PAIRS = [[0, 0xffff], [0x8000, 0x8000], [0x7fff, 0xffff], [0xff, 0x0f], [0x101, 0x101], [0xffff, 0xaaaa], [0x5555, 0xaaaa], [0x1234, 0xff]];
const IMM8 = [0, 1, 0x7f, 0x80, 0xff, 0x10, 0x0f, 0x55];
const ALIAS_ADDRESS = DATA + 0x180;
function edgeInputs(form, index) {
  let [left, right] = (form >= 4 ? TEST_PAIRS : CMP_PAIRS)[index];
  if (form === 3) right = IMM8[index] < 128 ? IMM8[index] : 0xff00 | IMM8[index];
  return {left, right, memory: form === 1 ? right : left, register_value: form === 1 ? left : right};
}
function encoding(form, register, tail, immediate = 1) {
  const opcode = [0x39, 0x3b, 0x81, 0x83, 0x85, 0xf7][form];
  const field = [register, register, 7, 7, register, 0][form];
  const raw = [0x66, opcode, tail[0] | field << 3, ...tail.slice(1)];
  if (form === 2 || form === 5) raw.push(immediate & 255, immediate >>> 8);
  if (form === 3) raw.push(immediate & 255);
  return raw;
}
export function dataHex() {
  const b = Buffer.from(Array.from({length: 4096}, (_, i) => (73 * i + 11) & 255));
  for (let form = 0; form < 6; form++) for (let index = 0; index < 8; index++) b.writeUInt16LE(edgeInputs(form, index).memory, 2 * (form * 8 + index));
  b.writeUInt16LE(0x8000, 0x100); b.writeUInt16LE(1, 0x102); b.writeUInt16LE(0x8001, 0x180); return b.toString('hex');
}
export function bankPlan() {
  const b = Buffer.alloc(4096, 0xcc), forms = [], groups = []; let at = 0;
  function form(axis, type, register, tail, immediate = 1, fields = {}) {
    const raw = encoding(type, register, tail, immediate), row = {id: forms.length, axis, form: type, register: [0, 1, 4].includes(type) ? register : null,
      pc: CODE + at, length: raw.length, hex: Buffer.from(raw).toString('hex'), overrides: {}, ...fields};
    forms.push(row); b.set(raw, at); at += raw.length; return row;
  }
  function finish(id, start, instructions) {b.set([0xeb, 0xfe], at); at += 2; groups.push({id, blocks: [[CODE + start, at - start]], instructions: instructions + 1});}
  for (let type = 0; type < 6; type++) for (let index = 0; index < 8; index++) {
    const pair = edgeInputs(type, index);
    form('edge', type, index, [5, ...word(DATA + 2 * (type * 8 + index))], pair.right, {case: index, register_value: pair.register_value});
  }
  finish('main', 0, 48);
  const aliases = [];
  for (let register = 0; register < 8; register++) {
    aliases.push({alias: 'base', register, tail: register === 4 ? [4, 0x24] : register === 5 ? [0x45, 0] : [register], overrides: {[register]: ALIAS_ADDRESS}});
    if (register !== 4) {
      aliases.push({alias: 'index', register, tail: [4, register << 3 | 5, ...word(ALIAS_ADDRESS)], overrides: {[register]: 0}});
      aliases.push({alias: 'both', register, tail: [register === 5 ? 0x44 : 4, register << 3 | register, ...(register === 5 ? [0] : [])], overrides: {[register]: ALIAS_ADDRESS / 2}});
    }
  }
  assert.equal(aliases.length, 22);
  const aliasRows = [0, 1, 4].flatMap(type => aliases.map(a => ({type, ...a})));
  for (let half = 0; half < 2; half++) {
    at = 0x200 + half * 0x200; const start = at;
    for (const a of aliasRows.slice(half * 33, half * 33 + 33)) form('alias', a.type, a.register, a.tail, 1, {alias: a.alias, overrides: a.overrides});
    finish(half ? 'alias-b' : 'alias-a', start, 33);
  }
  at = 0x600;
  const wraps = [
    [3, [0x83, ...word(0x5180)], {3: 0xfffff000}],
    [6, [4, 0xf5, ...word(ALIAS_ADDRESS)], {6: 0x20000000}],
    [1, [0x84, 0x0b, ...word(ALIAS_ADDRESS)], {3: 0x80000000, 1: 0x80000000}],
    [0, [0x84, 0x30, ...word(ALIAS_ADDRESS)], {0: 0xffffffff, 6: 1}],
    [5, [0x84, 0x3d, ...word(ALIAS_ADDRESS)], {5: 1, 7: 0xffffffff}],
    [2, [4, 0x95, ...word(ALIAS_ADDRESS)], {2: 0x40000000}],
    [3, [0x43, 0xff], {3: ALIAS_ADDRESS + 1}],
    [4, [0x44, 0x24, 0xff], {4: ALIAS_ADDRESS + 1}],
  ];
  for (const type of [0, 1, 4]) for (const [register, tail, overrides] of wraps) form('wrap', type, register, tail, 1, {overrides});
  finish('wrap', 0x600, 24);
  at = 0x800; const faultForms = [];
  for (let type = 0; type < 6; type++) {
    const prefix = CODE + at; b[at++] = 0x90;
    faultForms.push({...form('fault', type, 0, [3], type === 3 ? 0xff : 1), prefix});
  }
  b.set([0xeb, 0xfe], at); at += 2; const faultBlock = [CODE + 0x800, at - 0x800];
  const consumerBlocks = [], paths = [];
  for (let path = 0; path < 3; path++) {
    const cc = [0, 2, 8][path], opcode = [0x39, 0x3b, 0x85][path], offset = 0xa00 + path * 64;
    const piece = [0x66, opcode, 0x03, 0x66, 0x0f, 0x40 + cc, 0xd1, 0x66, 0x0f, 0x41 + cc, 0xef,
      0x0f, 0x41 + cc, 0xf5, 0x0f, 0x90 + cc, 0xc4, 0x0f, 0x91 + cc, 0xc1, 0x70 + cc, 2];
    assert.equal(piece.length, 22); b.set(piece, offset); b.set([0x0f, 0x0b, 0xeb, 0], offset + 22);
    consumerBlocks.push([CODE + offset, 22], [CODE + offset + 24, 2]); paths.push({id: path, pc: CODE + offset, end_pc: CODE + offset + 26, instructions: 8});
  }
  b.set([0x0f, 0x0b], 0xf00);
  groups.push({id: 'control', blocks: [faultBlock, ...consumerBlocks], instructions: 37},
    {id: 'helper', blocks: [[faultForms[0].prefix, faultForms[0].length + 1]], instructions: 2},
    {id: 'keeper', blocks: [[faultForms[0].pc, faultForms[0].length]], instructions: 1});
  for (const group of groups) {assert.ok(group.blocks.length <= 8 && group.instructions <= 64); for (const [pc, size] of group.blocks) assert.ok(pc >= CODE && pc + size <= CODE + 4096);}
  assert.equal(forms.length, 144);
  return {hex: b.toString('hex'), forms, fault_forms: faultForms, consumer: {paths, blocks: consumerBlocks}, invalid_pc: CODE + 0xf00, groups};
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
    const spec = {id: contexts.length + 1, owner, entries, role, key: [0x574d0000 + contexts.length + 1, 0x50524544], pages: 6}; contexts.push(spec);
    const add = (kind, fields = {}) => actions.push({id: next++, context: spec.id, kind, ...fields});
    const input = (offset, hex, label) => add('input', {offset, hex, label});
    const host = (name, args, fields = {}) => add('host', {name, args, ...fields});
    const call = (group, budget, label, retired, fields = {}) => add('call', {group, channel: owner === 'resident' && !entries ? 'dispatch' : 'direct', budget, label, planned_retired: retired, ...fields});
    const seed = (pc, flags, overrides = {}, pattern = 0, label = 'CPU seed') => input(0, seedHex(pc, flags, pattern, overrides), label);
    const upload = (address, hex, label) => {input(140, hex, label); host('upload', [address, hex.length / 2]);};
    const page = (address, phase) => add('data', {address, count: 1024, file: `${spec.id}-${phase}-${address.toString(16)}.bin`, label: 'complete readable RAM page'});
    function seedForm(f, flags, pattern = 0) {
      const overrides = {...f.overrides};
      if (f.axis === 'edge' && f.register !== null) overrides[f.register] = (((0xb101 + pattern * 0x111 + f.register * 0x101) << 16) | f.register_value) >>> 0;
      seed(f.pc, flags, overrides, pattern, 'form and operand seed');
    }
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
        for (const f of bank.forms.filter(f => f.axis === 'edge')) for (const flags of [2, 0xcd7]) {
          seedForm(f, flags, flags === 2 ? 0 : 2); call('main', 1, 'edge form and FLAGS', 1, {form: f.id});
        }
        for (const [group, slot] of [['alias-a', 1], ['alias-b', 2]]) {
          compile(group, slot); const [start, length] = groups[group].blocks[0];
          for (const f of bank.forms.filter(f => f.axis === 'alias' && f.pc >= start && f.pc < start + length)) for (const flags of [2, 0xcd7]) {
            seedForm(f, flags); call(group, 1, 'legal address/source alias', 1, {form: f.id});
          }
        }
        compile('wrap', 3);
        for (const f of bank.forms.filter(f => f.axis === 'wrap')) {seedForm(f, 0xcd7); call('wrap', 1, 'wrapping effective address', 1, {form: f.id});}
      } else for (const f of bank.forms.filter(f => f.axis === 'edge' && [0, 2].includes(f.case))) {
        seedForm(f, 0xcd7, f.case ? 1 : 0); call('main', 1, 'entry form representative', 1, {form: f.id});
      }
      compile('control', entries ? 1 : 4);
      if (!entries) {
        const shapes = [{id: 'unmapped', address: 0x8000, map: 0x8000}, {id: 'none', address: DATA, permission: DATA, bits: 0},
          {id: 'write', address: DATA, permission: DATA, bits: 2}, {id: 'execute', address: DATA, permission: DATA, bits: 4},
          {id: 'cross-unmapped', address: DATA + 4095, map: DATA + 4096}, {id: 'cross-write', address: DATA + 4095, permission: DATA + 4096, bits: 2},
          {id: 'overflow', address: 0xffffffff}];
        for (const shape of shapes) {
          if (shape.id === 'cross-write') {host('map', [DATA + 4096, 1, 3]); upload(DATA + 4096, data, 'cross-page data');}
          for (const f of bank.fault_forms) {
            if (shape.permission !== undefined) host('protect', [shape.permission, 1, shape.bits]);
            seed(f.prefix, 0xcd7, {0: 0xb101ffff, 3: shape.address}); call('control', 20, `${shape.id} prefix fault`, 1, {form: f.id});
            call('control', 1, `${shape.id} unchanged retry`, 0, {form: f.id});
            if (shape.map !== undefined) {host('map', [shape.map, 1, 3]); upload(shape.map, data, 'data-only fault repair'); host('protect', [shape.map, 1, 1]);}
            else if (shape.permission !== undefined) host('protect', [shape.permission, 1, 1]);
            else input(28, Buffer.from(word(DATA)).toString('hex'), 'explicit overflow address-register repair');
            call('control', 1, `${shape.id} repair completion`, 1, {form: f.id});
            if (shape.map !== undefined) host('unmap', [shape.map, 1]);
          }
          if (shape.id === 'cross-write') host('unmap', [DATA + 4096, 1]);
        }
        for (const address of [DATA + 4094, 0xfffffffe]) for (const f of bank.fault_forms) for (const pattern of [0, 2]) {
          seed(f.pc, pattern ? 0xcd7 : 2, {3: address}, pattern); call('control', 1, 'last-two-byte read', 1, {form: f.id});
        }
        host('map', [DATA + 4096, 1, 3]); upload(DATA + 4096, data, 'cross-page success data'); host('protect', [DATA + 4096, 1, 1]);
        for (const f of bank.fault_forms) for (const pattern of [0, 2]) {seed(f.pc, pattern ? 0xcd7 : 2, {3: DATA + 4095}, pattern); call('control', 1, 'successful cross-page read', 1, {form: f.id});}
        page(DATA + 4096, 'cross-final'); host('unmap', [DATA + 4096, 1]);
      }
      for (const path of bank.consumer.paths) {
        const overrides = {0: (0xa1010000 | [1, 0, 0xffff][path.id]) >>> 0, 1: 0xa2021234, 3: DATA + 0x100 + (path.id === 1 ? 2 : 0)};
        seed(path.pc, 0xcd7, overrides); call('control', 9, 'continuous consumer', 8, {path: path.id});
        seed(path.pc, 0xcd7, overrides); for (const budget of [1, 3, 4]) call('control', budget, 'split consumer', budget, {path: path.id});
        call('control', 1, 'consumer no replay', 0, {path: path.id});
      }
      seed(bank.fault_forms[0].prefix, 0xcd7, {3: DATA}); call('control', 0, 'zero budget', 0);
      input(96, '01000000', 'set cancel'); call('control', 20, 'cancel positive budget', 0); call('control', 0, 'cancel zero budget', 0); input(96, '00000000', 'clear cancel');
      const first = bank.consumer.paths[0].pc, request = Buffer.alloc(entries ? 8 : 16); request.writeUInt32LE(first);
      if (!entries) request.writeUInt32LE(22, 4); request.writeUInt32LE(bank.invalid_pc, entries ? 4 : 8); if (!entries) request.writeUInt32LE(2, 12);
      input(140, request.toString('hex'), 'late admitted predicate then UD2');
      host(owner === 'resident' ? entries ? 'compile_resident_entries' : 'compile_resident' : entries ? 'compile_entries' : 'compile', entries ? [2, 0] : [2], {group: 'late-failure', allowed_failure: true});
      seed(first, 0xcd7, {0: 0xa1010001, 3: DATA + 0x100}); call('control', 1, 'prior owner survival', 1); add('guard', {group: 'control', wrong_key: true});
    } else if (role === 'helper') {
      const request = Buffer.alloc(8); request.writeUInt32LE(bank.fault_forms[0].prefix); request.writeUInt32LE(bank.fault_forms[0].length + 1, 4);
      input(140, request.toString('hex'), 'inert helper module descriptors'); host('compile', [1], {group: 'helper'});
      for (const h of helperCases()) {
        add('module', {target: 'child', group: 'helper', slot: null, file: `${spec.id}-helper-${h.id}.wasm`, helper_case: h.id});
        seed(bank.fault_forms[0].prefix, 0xcd7, {0: 0xb1010001, 3: DATA}); call('helper', 20, 'malformed inert helper ' + h.id, 1, {channel: 'direct', helper_case: h.id});
      }
    } else {
      compile('keeper', 0); const pc = bank.fault_forms[0].pc;
      seed(pc, 0xcd7, {0: 0xb1010001, 3: DATA}); call('keeper', 1, 'current before same-byte write', 1);
      seed(pc, 0xcd7, {0: 0xb1010001, 3: DATA}); host('write8', [CODE, 0x66]); call('keeper', 1, 'same-byte stale direct', 0, {channel: 'direct'});
      if (owner === 'resident') call('keeper', 1, 'same-byte stale dispatcher', 0, {channel: 'dispatch'});
    }
    page(DATA, 'final'); if (role === 'normal') page(TOP, 'final');
    add('close'); add('closed_call', {group: role === 'normal' ? 'control' : role === 'helper' ? 'helper' : 'keeper', channel: 'direct', budget: 1, label: 'cached closed function', planned_retired: 0});
  }
  for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) context(owner, entries);
  context('replacement', false, 'helper'); context('replacement', false, 'stale'); context('resident', false, 'stale');
  const count = kind => actions.filter(a => a.kind === kind).length, calls = actions.filter(a => ['call', 'closed_call'].includes(a.kind));
  const frames = actions.reduce((n, a) => n + (['open', 'close'].includes(a.kind) ? 1 : a.kind === 'closed_call' ? 0 : 2), 0);
  const counts = {contexts: contexts.length, actions: actions.length, generated_calls: calls.length, planned_retired: calls.reduce((n, a) => n + a.planned_retired, 0),
    modules: count('module'), frames, raw_bytes: frames * SIZE, data_files: count('data'), diagnostic_reads: count('data') * 1024,
    inputs: count('input'), guards: count('guard'), tables: count('table'), files: 10 + count('module') + count('data'),
    checkpoints: calls.length + count('guard') + count('close') + count('data') + 1};
  assert.ok(counts.generated_calls <= 1200 && counts.modules <= 32 && counts.frames <= 12000);
  return {schema_version: 1, size: SIZE, code: CODE, data_address: DATA, top: TOP, data_hex: data, bank, helper_cases: helperCases(), contexts, actions, counts};
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const output = resolve(process.argv[2]); mkdirSync(output);
  const plan = makePlan(); writeFileSync(resolve(output, 'plan.json'), JSON.stringify(plan, null, 2) + '\n', {flag: 'wx'});
  console.log(JSON.stringify(plan.counts));
}
