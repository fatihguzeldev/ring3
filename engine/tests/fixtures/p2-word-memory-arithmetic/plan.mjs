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
const LEFT = [[0, 0x7fff, 0xffff, 0x8000, 0xf, 0xff, 0x1234, 0xaaaa], [0, 0x8000, 0, 0x7fff, 0x10, 0x101, 0xffff, 0x8000]];
const RIGHT = [[0, 1, 1, 0x8000, 1, 1, 0xeeee, 0x5555], [0, 1, 1, 0xffff, 1, 1, 0xffff, 0x7fff]];
const ALIAS = DATA + 0x180, CONSUMER = DATA + 0x300;
export function dataHex() {
  const b = Buffer.from(Array.from({length: 4096}, (_, i) => (73 * i + 11) & 255));
  RIGHT.flat().forEach((n, i) => b.writeUInt16LE(n, i * 2));
  for (const [at, n] of [[0x100, 2], [0x102, 0x8001], [0x180, 0x8001], [0x200, 0x8001], [0x202, 2], [0x300, 1]]) b.writeUInt16LE(n, at);
  return b.toString('hex');
}
export function bankPlan() {
  const b = Buffer.alloc(4096, 0xcc), forms = [], groups = []; let at = 0;
  function form(axis, kind, destination, tail, fields = {}) {
    const raw = [0x66, kind === 'add' ? 0x03 : 0x2b, tail[0] | destination << 3, ...tail.slice(1)];
    const row = {id: forms.length, axis, kind, destination, pc: CODE + at, length: raw.length, hex: Buffer.from(raw).toString('hex'), overrides: {}, ...fields};
    forms.push(row); b.set(raw, at); at += raw.length; return row;
  }
  for (const [type, kind] of ['add', 'sub'].entries()) for (let sample = 0; sample < 8; sample++) {
    form('edge', kind, sample, [5, ...word(DATA + 2 * (type * 8 + sample))], {case: sample, left: LEFT[type][sample]});
  }
  const wraps = [[3, [0x83, ...word(0x5180)], {3: 0xfffff000}], [6, [4, 0xf5, ...word(ALIAS)], {6: 0x20000000}],
    [1, [0x84, 0x0b, ...word(ALIAS)], {3: 0x80000000, 1: 0x80000000}], [0, [0x84, 0x30, ...word(ALIAS)], {0: 0xffffffff, 6: 1}],
    [5, [0x84, 0x3d, ...word(ALIAS)], {5: 1, 7: 0xffffffff}], [2, [4, 0x95, ...word(ALIAS)], {2: 0x40000000}],
    [3, [0x43, 0xff], {3: ALIAS + 1}], [4, [0x44, 0x24, 0xff], {4: ALIAS + 1}]];
  for (const kind of ['add', 'sub']) for (const [destination, tail, overrides] of wraps) form('wrap', kind, destination, tail, {overrides});
  b.set([0xeb, 0xfe], at); at += 2; groups.push({id: 'main', blocks: [[CODE, at]], instructions: 33});
  at = 0x200;
  for (const kind of ['add', 'sub']) for (let r = 0; r < 8; r++) {
    form('alias', kind, r, r === 4 ? [4, 0x24] : r === 5 ? [0x45, 0] : [r], {alias: 'base', overrides: {[r]: ALIAS}});
    if (r !== 4) {
      form('alias', kind, r, [4, r << 3 | 5, ...word(ALIAS)], {alias: 'index', overrides: {[r]: 0}});
      form('alias', kind, r, [r === 5 ? 0x44 : 4, r << 3 | r, ...(r === 5 ? [0] : [])], {alias: 'both', overrides: {[r]: ALIAS / 2}});
    }
  }
  b.set([0xeb, 0xfe], at); at += 2; groups.push({id: 'alias', blocks: [[CODE + 0x200, at - 0x200]], instructions: 45});
  at = 0x600; const faultForms = [];
  for (const kind of ['add', 'sub']) {const prefix = CODE + at; b[at++] = 0x90; faultForms.push({...form('fault', kind, 0, [3]), prefix});}
  b.set([0xeb, 0xfe], at); at += 2; const faultBlock = [CODE + 0x600, at - 0x600];
  const paths = [], pathBlocks = [];
  for (let path = 0; path < 4; path++) {
    const offset = 0x800 + path * 64, kind = path < 2 ? 'add' : 'sub', initial = [0x7fff, 0x7fff, 0x8000, 0x8000][path];
    const prefix = [0x66, 0xb8, initial & 255, initial >>> 8, 0x66, 0xbf, 0xff, 0xff];
    const piece = [0x66, kind === 'add' ? 3 : 0x2b, 3, 0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc1, 0x0f, 0x94, 0xc2,
      0x0f, 0x98, 0xc6, 0x0f, 0x9a, 0xc5, path % 2 ? 0x72 : 0x70, 2];
    b.set(prefix, offset); b.set(piece, offset + 8); b.set([0x0f, 0x0b, 0x66, 0x83, 0xd7, 0, 0x0f, 0x92, 0xc7, 0xeb, 0], offset + 28);
    pathBlocks.push([[CODE + offset, 28], [CODE + offset + 30, 9]]);
    paths.push({id: path, kind, prefix: CODE + offset, pc: CODE + offset + 8, end_pc: CODE + offset + 39, instructions: 10});
  }
  const nextPaths = [], nextBlocks = [];
  for (let path = 0; path < 2; path++) {
    const offset = 0xa00 + path * 64;
    const piece = [0x66, path ? 0x2b : 3, 0x9b, ...word(0x5bfc0000), 0x66, path ? 3 : 0x2b, 0x83, ...word(0x5bfc0000),
      0x0f, path ? 0x92 : 0x94, 0xc4, path ? 0x72 : 0x74, 2];
    b.set(piece, offset); b.set([0x0f, 0x0b, 0xeb, 0], offset + 19);
    nextBlocks.push([CODE + offset, 19], [CODE + offset + 21, 2]); nextPaths.push({id: path, pc: CODE + offset, end_pc: CODE + offset + 23, instructions: 5});
  }
  b.set([0x0f, 0x0b], 0xf00);
  groups.push({id: 'control-a', blocks: [faultBlock, ...pathBlocks.slice(0, 2).flat()], instructions: 29},
    {id: 'control-b', blocks: [...pathBlocks.slice(2).flat(), ...nextBlocks], instructions: 34},
    {id: 'helper', blocks: [[faultForms[0].prefix, 4]], instructions: 2}, {id: 'keeper', blocks: [[faultForms[0].pc, 3]], instructions: 1});
  for (const g of groups) {assert.ok(g.blocks.length <= 8 && g.instructions <= 64); for (const [pc, size] of g.blocks) assert.ok(pc >= CODE && pc + size <= CODE + 4096);}
  assert.equal(forms.length, 78);
  return {hex: b.toString('hex'), forms, fault_forms: faultForms, consumer: {paths, next_paths: nextPaths}, invalid_pc: CODE + 0xf00, groups};
}
export function helperCases() {
  const good = record('R3MH', 40, [0, 0x8001, 0, 0, 0, 2], 2), rows = [];
  const add = (id, status, mutate) => {const packet = Buffer.from(good); mutate(packet); rows.push({id, status, packet_hex: packet.toString('hex')});};
  add('status', 9, () => {}); add('version', 0, p => p.writeUInt32LE(0x10001, 4)); add('width', 0, p => p.writeUInt32LE(4, 36));
  add('value', 0, p => p.writeUInt32LE(0x10000, 20));
  add('span', 0, p => {p.writeUInt32LE(1, 16); p.writeUInt32LE(0, 20); p.writeUInt32LE(1, 24); p.writeUInt32LE(DATA + 2, 28); p.writeUInt32LE(1, 32);});
  add('infrastructure', 0, p => {p.writeUInt32LE(2, 16); p.writeUInt32LE(0, 20); p.writeUInt32LE(2, 24);}); return rows;
}
export function makePlan() {
  const bank = bankPlan(), data = dataHex(), groups = new Map(bank.groups.map(g => [g.id, g])), actions = [], contexts = []; let next = 0;
  function context(owner, entries, role = 'normal') {
    const spec = {id: contexts.length + 1, owner, entries, role, key: [0x57410000 + contexts.length + 1, 0x41524954], pages: 6}; contexts.push(spec); let diagnostic = 0;
    const add = (kind, fields = {}) => actions.push({id: next++, context: spec.id, kind, ...fields});
    const input = (offset, hex, label) => add('input', {offset, hex, label});
    const host = (name, args, fields = {}) => add('host', {name, args, ...fields});
    const call = (group, budget, label, retired, fields = {}) => add('call', {group, channel: owner === 'resident' && !entries ? 'dispatch' : 'direct', budget, label, planned_retired: retired, ...fields});
    const seed = (pc, flags = 0xcd7, overrides = {}, pattern = 0, label = 'CPU seed') => input(0, seedHex(pc, flags, pattern, overrides), label);
    const upload = (address, hex, label) => {input(140, hex, label); host('upload', [address, hex.length / 2]);};
    const diagnose = (address, count, phase) => add('data', {address, count, file: `${spec.id}-${diagnostic++}-${phase}.bin`, label: count === 1024 ? 'complete readable RAM page' : 'bounded readable watched word'});
    const watch = address => {diagnose(address - address % 4, 1, 'watch'); if (address % 4 === 3) diagnose(address + 1, 1, 'watch-second');};
    function compile(group, slot) {
      const g = groups.get(group), request = Buffer.alloc(g.blocks.length * (entries ? 4 : 8));
      g.blocks.forEach(([pc, n], i) => {request.writeUInt32LE(pc, i * (entries ? 4 : 8)); if (!entries) request.writeUInt32LE(n, i * 8 + 4);});
      input(140, request.toString('hex'), 'compiler descriptors');
      host(owner === 'resident' ? entries ? 'compile_resident_entries' : 'compile_resident' : entries ? 'compile_entries' : 'compile', entries ? [g.blocks.length, 0] : [g.blocks.length], {group});
      add('module', {target: 'child', group, slot, file: `${spec.id}-${group}.wasm`, helper_case: null});
      if (owner === 'resident') {
        add('table', {group, slot}); host('acknowledge_resident_installation', ['key_low', 'key_high', 'unit_low', 'unit_high', slot], {group});
        if (slot === 0) {host('dispatcher_module', ['key_low', 'key_high']); add('module', {target: 'dispatcher', group: 'dispatcher', slot: null, file: `${spec.id}-dispatcher.wasm`, helper_case: null});}
      }
    }
    function formSeed(f, flags, pattern = 0) {
      const overrides = {...f.overrides}; if (f.axis === 'edge') overrides[f.destination] = (((0xb101 + pattern * 0x111 + f.destination * 0x101) << 16) | f.left) >>> 0;
      seed(f.pc, flags, overrides, pattern, 'form and operand seed');
    }
    const consumerSeed = path => seed(path.pc, 0xcd7, {0: (0xa1010000 | [0x7fff, 0xffff, 0x8000, 0][path.id]) >>> 0, 3: CONSUMER, 7: 0xa808ffff});
    function repair(path) {
      const group = path.id === 1 ? 'control-a' : 'control-b';
      seed(path.prefix, 0xcd7, {3: 0x8000}); call(group, 20, 'one-seed repair initial fault', 2, {path: path.id});
      call(group, 1, 'one-seed repair unchanged retry', 0, {path: path.id});
      host('map', [0x8000, 1, 3]); upload(0x8000, data, 'declared one-seed repair upload'); host('protect', [0x8000, 1, 2]);
      call(group, 1, 'one-seed repair write-only fault', 0, {path: path.id});
      host('write8', [0x8000, 1]); host('write8', [0x8001, 0x80]); host('protect', [0x8000, 1, 1]);
      call(group, 1, 'one-seed repair arithmetic', 1, {path: path.id}); watch(0x8000);
      call(group, 9, 'one-seed repair consumers', 9, {path: path.id}); call(group, 1, 'one-seed repair no replay', 0, {path: path.id});
      diagnose(0x8000, 1024, 'repair-final'); host('unmap', [0x8000, 1]);
    }
    add('open'); input(0, initialHex(), 'full authored nondefault arena'); host('map', [CODE, 1, 7]); upload(CODE, bank.hex, 'complete literal bank');
    if (role !== 'stale') host('protect', [CODE, 1, 5]);
    host('map', [DATA, 1, 3]); upload(DATA, data, 'literal data page'); host('protect', [DATA, 1, 1]); diagnose(DATA, 1024, 'initial-data');
    if (role === 'normal') {host('map', [TOP, 1, 3]); upload(TOP, data, 'literal top page'); host('protect', [TOP, 1, 1]); diagnose(TOP, 1024, 'initial-top');}
    if (role === 'normal') {
      compile('main', 0);
      for (const f of bank.forms.filter(f => f.axis === 'edge')) for (const flags of entries ? [0xcd7] : [2, 0xcd7]) {
        formSeed(f, flags, flags === 2 ? 0 : 2); call('main', 1, entries ? 'entry form representative' : 'edge form and FLAGS', 1, {form: f.id}); watch(DATA + 2 * f.id);
      }
      if (!entries) {
        for (const f of bank.forms.filter(f => f.axis === 'wrap')) {formSeed(f, 0xcd7); call('main', 1, 'wrapping effective address', 1, {form: f.id}); watch(ALIAS);}
        compile('alias', 1);
        for (const f of bank.forms.filter(f => f.axis === 'alias')) for (const flags of [2, 0xcd7]) {formSeed(f, flags); call('alias', 1, 'legal destination address alias', 1, {form: f.id}); watch(ALIAS);}
        for (const f of bank.forms.filter(f => f.axis === 'alias' && f.alias === 'base' && f.destination === 3)) {
          seed(f.pc, 0xcd7, {3: 0xffffffff}); call('alias', 1, 'overflow alias fault', 0, {form: f.id}); call('alias', 1, 'overflow alias unchanged retry', 0, {form: f.id});
          input(28, Buffer.from(word(CONSUMER)).toString('hex'), 'explicit overflow address-register different input'); call('alias', 1, 'overflow alias repair completion', 1, {form: f.id}); watch(CONSUMER);
        }
      }
      compile('control-a', entries ? 1 : 2);
      if (!entries) {
        const shapes = [{id: 'unmapped', address: 0x8000, map: 0x8000}, {id: 'none', address: DATA, permission: DATA, bits: 0},
          {id: 'write', address: DATA, permission: DATA, bits: 2}, {id: 'execute', address: DATA, permission: DATA, bits: 4},
          {id: 'cross-unmapped', address: DATA + 4095, map: DATA + 4096}, {id: 'cross-write', address: DATA + 4095, permission: DATA + 4096, bits: 2},
          {id: 'overflow', address: 0xffffffff}];
        for (const shape of shapes) {
          if (shape.id === 'cross-write') {host('map', [DATA + 4096, 1, 3]); upload(DATA + 4096, data, 'cross-page data');}
          for (const f of bank.fault_forms) {
            if (shape.permission !== undefined) host('protect', [shape.permission, 1, shape.bits]);
            seed(f.prefix, 0xcd7, {0: 0xb101ffff, 3: shape.address}); call('control-a', 20, `${shape.id} prefix fault`, 1, {form: f.id});
            call('control-a', 1, `${shape.id} unchanged retry`, 0, {form: f.id});
            if (shape.map !== undefined) {host('map', [shape.map, 1, 3]); upload(shape.map, data, 'data-only fault repair'); host('protect', [shape.map, 1, 1]);}
            else if (shape.permission !== undefined) host('protect', [shape.permission, 1, 1]);
            else input(28, Buffer.from(word(CONSUMER)).toString('hex'), 'explicit overflow address-register different input');
            call('control-a', 1, `${shape.id} repair completion`, 1, {form: f.id}); watch(shape.id === 'overflow' ? CONSUMER : shape.address);
            if (shape.map !== undefined) host('unmap', [shape.map, 1]);
          }
          if (shape.id === 'cross-write') host('unmap', [DATA + 4096, 1]);
        }
        for (const address of [DATA + 17, DATA + 4094, 0xfffffffe]) for (const f of bank.fault_forms) for (const pattern of [0, 2]) {
          seed(f.pc, pattern ? 0xcd7 : 2, {3: address}, pattern); call('control-a', 1, 'unaligned or last-two-byte read', 1, {form: f.id}); watch(address);
        }
        host('map', [DATA + 4096, 1, 3]); upload(DATA + 4096, data, 'cross-page success data'); host('protect', [DATA + 4096, 1, 1]);
        for (const f of bank.fault_forms) for (const pattern of [0, 2]) {seed(f.pc, pattern ? 0xcd7 : 2, {3: DATA + 4095}, pattern); call('control-a', 1, 'successful cross-page read', 1, {form: f.id}); watch(DATA + 4095);}
        diagnose(DATA + 4096, 1024, 'cross-final'); host('unmap', [DATA + 4096, 1]);
      }
      for (const path of bank.consumer.paths.filter(p => p.id < 2)) {
        consumerSeed(path); call('control-a', 11, 'continuous consumer', 10, {path: path.id}); watch(CONSUMER);
        consumerSeed(path); call('control-a', 1, 'split consumer', 1, {path: path.id});
        if (path.id === 0) {input(96, '01000000', 'set consumer cancel'); call('control-a', 5, 'consumer cancel continuation', 0, {path: path.id}); input(96, '00000000', 'clear consumer cancel');}
        for (const budget of [5, 4]) call('control-a', budget, 'split consumer', budget, {path: path.id});
        call('control-a', 1, 'consumer no replay', 0, {path: path.id}); watch(CONSUMER);
      }
      if (!entries) {
        repair(bank.consumer.paths[1]);
        compile('control-b', 3);
        for (const path of bank.consumer.paths.filter(p => p.id >= 2)) {
          consumerSeed(path); call('control-b', 11, 'continuous consumer', 10, {path: path.id}); watch(CONSUMER);
          consumerSeed(path); for (const budget of [1, 5, 4]) call('control-b', budget, 'split consumer', budget, {path: path.id});
          call('control-b', 1, 'consumer no replay', 0, {path: path.id}); watch(CONSUMER);
        }
        for (const path of bank.consumer.next_paths) {
          const overrides = {0: path.id ? 0xa1017fff : 0xa1018001, 3: path.id ? 0xa4044202 : 0xa4044100};
          seed(path.pc, 0xcd7, overrides); call('control-b', 6, 'current-address continuous', 5, {path: path.id});
          watch(DATA + (path.id ? 0x200 : 0x100)); watch(DATA + (path.id ? 0x202 : 0x102));
          seed(path.pc, 0xcd7, overrides); for (const budget of [1, 1, 3]) call('control-b', budget, 'current-address split', budget, {path: path.id});
          call('control-b', 1, 'current-address no replay', 0, {path: path.id}); watch(DATA + (path.id ? 0x200 : 0x100)); watch(DATA + (path.id ? 0x202 : 0x102));
        }
        repair(bank.consumer.paths[3]);
      }
      const current = entries ? 'control-a' : 'control-b', survivor = bank.consumer.paths[entries ? 0 : 2];
      seed(survivor.pc, 0xcd7, {3: DATA}); call(current, 0, 'zero budget', 0);
      input(96, '01000000', 'set cancel'); call(current, 20, 'cancel positive budget', 0); call(current, 0, 'cancel zero budget', 0); input(96, '00000000', 'clear cancel');
      const first = bank.consumer.paths[0].prefix, request = Buffer.alloc(entries ? 8 : 16); request.writeUInt32LE(first);
      if (!entries) request.writeUInt32LE(28, 4); request.writeUInt32LE(bank.invalid_pc, entries ? 4 : 8); if (!entries) request.writeUInt32LE(2, 12);
      input(140, request.toString('hex'), 'late admitted arithmetic then UD2');
      host(owner === 'resident' ? entries ? 'compile_resident_entries' : 'compile_resident' : entries ? 'compile_entries' : 'compile', entries ? [2, 0] : [2], {group: 'late-failure', allowed_failure: true});
      consumerSeed(survivor); call(current, 1, 'prior owner survival', 1); add('guard', {group: current, wrong_key: true});
    } else if (role === 'helper') {
      const request = Buffer.alloc(8); request.writeUInt32LE(bank.fault_forms[0].prefix); request.writeUInt32LE(4, 4);
      input(140, request.toString('hex'), 'inert helper module descriptors'); host('compile', [1], {group: 'helper'});
      for (const h of helperCases()) {
        add('module', {target: 'child', group: 'helper', slot: null, file: `${spec.id}-helper-${h.id}.wasm`, helper_case: h.id});
        seed(bank.fault_forms[0].prefix, 0xcd7, {0: 0xb1010001, 3: DATA}); call('helper', 20, 'malformed inert helper ' + h.id, 1, {channel: 'direct', helper_case: h.id});
      }
    } else {
      compile('keeper', 0); const pc = bank.fault_forms[0].pc;
      seed(pc, 0xcd7, {0: 0xb1010001, 3: DATA}); call('keeper', 1, 'current before same-byte write', 1); watch(DATA);
      seed(pc, 0xcd7, {0: 0xb1010001, 3: DATA}); host('write8', [CODE, 0x66]); call('keeper', 1, 'same-byte stale direct', 0, {channel: 'direct'});
      if (owner === 'resident') call('keeper', 1, 'same-byte stale dispatcher', 0, {channel: 'dispatch'});
    }
    diagnose(DATA, 1024, 'final-data'); if (role === 'normal') diagnose(TOP, 1024, 'final-top');
    add('close'); add('closed_call', {group: role === 'normal' ? entries ? 'control-a' : 'control-b' : role === 'helper' ? 'helper' : 'keeper', channel: 'direct', budget: 1, label: 'cached closed function', planned_retired: 0});
  }
  for (const owner of ['replacement', 'resident']) for (const entries of [false, true]) context(owner, entries);
  context('replacement', false, 'helper'); context('replacement', false, 'stale'); context('resident', false, 'stale');
  const count = kind => actions.filter(a => a.kind === kind).length, calls = actions.filter(a => ['call', 'closed_call'].includes(a.kind));
  const frames = actions.reduce((n, a) => n + (['open', 'close'].includes(a.kind) ? 1 : a.kind === 'closed_call' ? 0 : 2), 0);
  const diagnostics = actions.filter(a => a.kind === 'data');
  const counts = {contexts: contexts.length, actions: actions.length, generated_calls: calls.length, planned_retired: calls.reduce((n, a) => n + a.planned_retired, 0),
    modules: count('module'), frames, raw_bytes: frames * SIZE, data_files: count('data'), diagnostic_reads: diagnostics.reduce((n, a) => n + a.count, 0),
    inputs: count('input'), guards: count('guard'), tables: count('table'), files: 10 + count('module') + count('data'),
    checkpoints: calls.length + count('guard') + count('close') + count('data') + 1};
  assert.ok(counts.generated_calls <= 900 && counts.modules <= 28 && counts.frames <= 9000);
  return {schema_version: 1, size: SIZE, code: CODE, data_address: DATA, top: TOP, data_hex: data, bank, helper_cases: helperCases(), contexts, actions, counts};
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const output = resolve(process.argv[2]); mkdirSync(output);
  const plan = makePlan(); writeFileSync(resolve(output, 'plan.json'), JSON.stringify(plan, null, 2) + '\n', {flag: 'wx'}); console.log(JSON.stringify(plan.counts));
}
