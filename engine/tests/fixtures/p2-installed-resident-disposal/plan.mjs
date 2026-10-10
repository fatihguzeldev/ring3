import assert from 'node:assert/strict';
import {mkdirSync, writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

export const SIZE = 4364, CODE = 0x1000;
function record(magic, length, fields = [], version = 1) {
  const bytes = Buffer.alloc(length); bytes.write(magic); bytes.writeUInt32LE(0x10000 + version, 4); bytes.writeUInt32LE(length, 8);
  fields.forEach((n, i) => bytes.writeUInt32LE(n >>> 0, 16 + 4 * i)); return bytes;
}
function word(n) {const b = Buffer.alloc(4); b.writeUInt32LE(n >>> 0); return b.toString('hex');}
export function initialHex() {
  const b = Buffer.alloc(SIZE);
  record('R3ST', 56, [0, 0, 0, 0, 0, 0, 0, 0, 0, 2]).copy(b);
  record('R3EX', 40, [1, 0, 0, 0, 0, 0]).copy(b, 56);
  record('R3MH', 40, [0, 0, 0, 0, 0, 0]).copy(b, 100);
  const fp = record('R3FP', 128); fp.writeUInt16LE(0x37f, 16); fp.writeUInt16LE(0xffff, 20); fp.copy(b, 4236);
  return b.toString('hex');
}
export function authoredHex() {
  const b = Buffer.from(Array.from({length: SIZE}, (_, i) => (37 * i + 19) & 255));
  record('R3ST', 56, [0xfffffff0, 0x11112222, 0x33334444, 0x55556666, 0x77778888, 0x9999aaaa, 0xbbbbcccc, 0xddddeeee, CODE, 0xcd7]).copy(b);
  record('R3EX', 40, [3, 0, 0, 0, 0, 0], 3).copy(b, 56); b.writeUInt32LE(0, 96);
  record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]).copy(b, 100);
  const fp = record('R3FP', 128);
  for (const [at, n] of [[16, 0x27f], [18, 0x2840], [20, 0xaaaa], [22, 0x345], [32, 0x33], [34, 0x2b]]) fp.writeUInt16LE(n, at);
  fp.writeUInt32LE(0x12345678, 24); fp.writeUInt32LE(0x9abcdef0, 28);
  for (let i = 40; i < 120; i++) fp[i] = (29 * i + 7) & 255;
  fp.copy(b, 4236); return b.toString('hex');
}
export function bankPlan() {
  const b = Buffer.alloc(4096, 0xcc), groups = [];
  for (let i = 0; i < 9; i++) {
    const pc = CODE + 0x20 * i, raw = Buffer.from([0x8d, 0x40, i + 1, 0xe9, 0x18, 0, 0, 0]);
    raw.copy(b, pc - CODE); groups.push({id: String.fromCharCode(65 + i), blocks: [[pc, 8]], instructions: 2});
  }
  return {hex: b.toString('hex'), groups, gap_pc: CODE + 9 * 0x20};
}
export function makePlan() {
  const bank = bankPlan(), groups = new Map(bank.groups.map(g => [g.id, g])), actions = [], contexts = [];
  const add = (context, kind, fields = {}) => actions.push({id: actions.length, context, kind, ...fields});
  for (const entries of [false, true]) {
    const c = {id: contexts.length + 1, owner: 'resident', entries, role: 'main', key: [0x47700000 + contexts.length + 1, 0x44495350], pages: 1};
    contexts.push(c);
    const input = (offset, hex, label) => add(c.id, 'input', {offset, hex, label});
    const host = (name, args, fields = {}) => add(c.id, 'host', {name, args, ...fields});
    const table = (binding, group, slot, label, expected_binding = null) => add(c.id, 'table', {binding, group, slot, label, expected_binding});
    const call = (binding, channel, budget, label) => add(c.id, 'call', {binding, channel, budget, label});
    const unitArgs = slot => ['key_low', 'key_high', 'unit_low', 'unit_high', slot];
    function compile(group, binding, slot, allowed_failure = false) {
      const g = groups.get(group), descriptor = Buffer.alloc(entries ? 4 : 8);
      descriptor.writeUInt32LE(g.blocks[0][0]); if (!entries) descriptor.writeUInt32LE(8, 4);
      input(140, descriptor.toString('hex'), `${binding} compile descriptor`);
      host(entries ? 'compile_resident_entries' : 'compile_resident', entries ? [1, 0] : [1], {group, binding, allowed_failure});
      if (!allowed_failure) add(c.id, 'module', {target: 'child', group, binding, slot, file: `${c.id}-${binding}.wasm`});
    }
    function install(group, binding, slot) {
      table(binding, group, slot, `${binding} host install`);
      host('acknowledge_resident_installation', unitArgs(slot), {binding});
    }
    add(c.id, 'open'); input(0, authoredHex(), 'declared full4364 input with nondefault FP');
    host('map', [CODE, 1, 7]); input(140, bank.hex, 'preloaded complete code page'); host('upload', [CODE, 4096]); host('protect', [CODE, 1, 4]);
    for (let i = 0; i < 8; i++) {
      const group = String.fromCharCode(65 + i), binding = `${group}0`;
      compile(group, binding, i);
      if (i === 0) host('discard_installed_resident', unitArgs(0), {binding, allowed_failure: true, label: 'current unacknowledged target rejects'});
      install(group, binding, i);
    }
    host('dispatcher_module', ['key_low', 'key_high']);
    add(c.id, 'module', {target: 'dispatcher', group: 'dispatcher', binding: 'dispatcher', slot: null, file: `${c.id}-dispatcher.wasm`});
    table(null, null, 0, 'clear A0 before rejected disposal controls', 'A0');
    for (const [label, args] of [
      ['wrong key low', [{ref: 'key_low', xor: 1}, 'key_high', 'unit_low', 'unit_high', 0]],
      ['wrong key high', ['key_low', {ref: 'key_high', xor: 1}, 'unit_low', 'unit_high', 0]],
      ['absent ID low', ['key_low', 'key_high', {ref: 'unit_low', xor: 0x80000000}, 'unit_high', 0]],
      ['absent ID high', ['key_low', 'key_high', 'unit_low', {ref: 'unit_high', xor: 1}, 0]],
      ['zero ID', ['key_low', 'key_high', 0, 0, 0]],
      ['slot outside range', unitArgs(8)], ['mismatched slot', unitArgs(1)],
    ]) host('discard_installed_resident', args, {binding: 'A0', allowed_failure: true, label});
    host('discard_unacknowledged_resident', unitArgs(0).slice(0, 4), {binding: 'A0', allowed_failure: true, label: 'old installed refusal unchanged'});
    host('retire_stale_resident', unitArgs(0).slice(0, 4), {binding: 'A0', allowed_failure: true, label: 'old current refusal unchanged'});
    input(96, word(1), 'declared cancellation input');
    host('discard_installed_resident', unitArgs(1), {binding: 'A0', allowed_failure: true, label: 'invalid slot precedes cancellation'});
    host('discard_installed_resident', unitArgs(0), {binding: 'A0', allowed_failure: true, label: 'exact cancelled request rejects'});
    input(96, word(0), 'clear cancellation input'); table('A0', 'A', 0, 'restore A0 after rejected disposal controls');
    call('dispatcher', 'dispatch', 17, 'unseeded A through H to I gap');
    compile('I', 'I-capacity', 0, true);
    table(null, null, 0, 'clear current A0 before successful disposal', 'A0');
    host('discard_installed_resident', unitArgs(0), {binding: 'A0', label: 'dispose A0, exact unit and slot'});
    call('A0', 'direct', 17, 'cached removed A0 positive budget rejects');
    call('A0', 'direct', 0, 'cached removed A0 zero budget rejects');
    input(96, word(1), 'cancelled cached guard input'); call('A0', 'direct', 0, 'cached removed A0 cancellation rejects'); input(96, word(0), 'clear cached guard cancellation');
    compile('I', 'I0', 0); install('I', 'I0', 0);
    call('dispatcher', 'dispatch', 3, 'unseeded continuation I to J gap');
    input(48, word(CODE), 'explicit later gap control EIP input only');
    host('find_installed_resident', ['key_low', 'key_high', CODE], {allowed_failure: true, label: 'evicted disjoint A lookup miss'});
    call('dispatcher', 'dispatch', 1, 'explicit A gap seed gives zero retirement');
    table(null, null, 0, 'clear I0 for second disposal cycle', 'I0');
    host('discard_installed_resident', unitArgs(0), {binding: 'I0', label: 'dispose I0 before same PC reinstall'});
    call('I0', 'direct', 0, 'cached removed I0 rejects');
    compile('A', 'A1', 0); install('A', 'A1', 0);
    host('discard_installed_resident', unitArgs(0), {binding: 'A0', allowed_failure: true, label: 'original removed ID cannot discard new same-slot owner'});
    call('A0', 'direct', 17, 'original cached A0 rejects after same PC and slot reuse');
    call('A1', 'direct', 3, 'fresh A1 at unchanged A PC executes');
    call('B0', 'direct', 3, 'surviving B0 direct child Exit v1 witness');
    add(c.id, 'close');
    for (const [binding, channel] of [['A0', 'direct'], ['A1', 'direct'], ['dispatcher', 'dispatch']]) add(c.id, 'closed_call', {binding, channel, budget: 3, label: 'cached closed status only, no arena read'});
    add(c.id, 'closed_host', {name: 'discard_installed_resident', args: unitArgs(0), binding: 'A1', label: 'closed method status only'});
  }
  const counts = {contexts: contexts.length, engine_instances: contexts.length, actions: actions.length,
    generated_calls: actions.filter(a => ['call', 'closed_call'].includes(a.kind)).length,
    inputs: actions.filter(a => a.kind === 'input').length, tables: actions.filter(a => a.kind === 'table').length,
    child_modules: actions.filter(a => a.kind === 'module' && a.target === 'child').length,
    dispatchers: actions.filter(a => a.kind === 'module' && a.target === 'dispatcher').length,
    host_api_calls: actions.filter(a => ['host', 'closed_host', 'close'].includes(a.kind)).length + 2 * contexts.length,
    frames: actions.reduce((n, a) => n + (a.kind === 'open' || a.kind === 'close' ? 1 : ['closed_call', 'closed_host'].includes(a.kind) ? 0 : 2), 0),
    checkpoints: actions.filter(a => ['call', 'closed_call', 'closed_host', 'close'].includes(a.kind) || a.kind === 'host' && a.name === 'discard_installed_resident').length + 1};
  counts.modules = counts.child_modules + counts.dispatchers; counts.journal_rows = counts.frames + counts.actions + counts.modules;
  counts.physical_files = counts.modules + 10;
  assert.ok(counts.generated_calls <= 64 && counts.modules <= 32 && counts.frames <= 1500 && counts.contexts <= 4);
  return {schema_version: 1, bank, initial_hex: initialHex(), authored_hex: authoredHex(), contexts, actions, counts};
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 3) throw new Error('usage: node plan.mjs FRESH_OUTPUT_DIRECTORY');
  const output = resolve(process.argv[2]); mkdirSync(output);
  const plan = makePlan(); writeFileSync(`${output}/plan.json`, JSON.stringify(plan, null, 2) + '\n', {flag: 'wx'});
  writeFileSync(`${output}/bank.x86`, Buffer.from(plan.bank.hex, 'hex'), {flag: 'wx'});
  console.log(JSON.stringify(plan.counts));
}
