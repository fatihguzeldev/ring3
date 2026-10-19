import assert from 'node:assert/strict';

let input = '';
for await (const chunk of process.stdin) input += chunk;
const fixture = JSON.parse(input), initial = Buffer.from(fixture.arena, 'hex');
const module = new WebAssembly.Module(Buffer.from(fixture.wrapper, 'hex'));
const DESC = 0x1800, MAP = 0x2000, PERMS = MAP + (4 << 20), BACK = PERMS + (1 << 20);
const BYTES = 128 * 65536, ADDRESS = 0x4000;
let controls = 0;

function execute(row, enabled) {
  const memory = new WebAssembly.Memory({initial: 128, maximum: row.grow ? 256 : 128});
  let data = new DataView(memory.buffer), bytes = new Uint8Array(memory.buffer);
  const backend = {mappings: MAP, permissions: PERMS, backing: BACK, pages: 2}, values = [];
  const set = (offset, value) => data.setUint32(offset, value >>> 0, true);
  const get = offset => data.getUint32(offset, true);
  const address = row.address ?? ADDRESS, epoch = row.epoch ?? 1n, count = row.count ?? 1;
  bytes.fill(0x5a); bytes.set(initial); bytes.fill(0, MAP, BACK);
  data.setUint16(fixture.segments + 4, 1, true);
  set(fixture.segments + 32, 0x020fffff); set(fixture.bases + 20, 0);
  data.setBigUint64(520, epoch, true);
  data.setBigUint64(DESC, row.liveEpoch ?? epoch, true);
  [MAP, PERMS, BACK, 2].forEach((value, index) => set(DESC + 8 + index * 4, value));
  const first = address >>> 12, next = ((address + 4096) >>> 0) >>> 12;
  set(MAP + first * 4, 1); set(MAP + next * 4, 2);
  bytes[PERMS + first] = 1; bytes[PERMS + next] = 1;
  const value = row.value ?? 0x80000000;
  for (let index = 0; index < 4; index++) {
    const at = (address + index) >>> 0, slot = get(MAP + (at >>> 12) * 4);
    bytes[BACK + (slot - 1) * 4096 + (at & 4095)] = value >>> (index * 8);
  }
  if (row.poison) { bytes.fill(0xa5, 100, 140); bytes[99] = 0x6b; bytes[140] = 0xd8; }
  if (row.field) set(DESC + row.field[0], row.field[1]);
  if (row.mapping !== undefined) set(MAP + first * 4, row.mapping);
  if (row.permission !== undefined) bytes[PERMS + first] = row.permission;
  if (row.firstContext) {
    data.setUint16(fixture.segments + 4, 3, true);
    set(fixture.bases + 20, 0x8000);
  }
  let calls = 0, getters = 0, phase = 0, closed = false;
  function packet(words) { words.forEach((word, index) => set(100 + index * 4, word)); }
  function success(payload) { packet([0x484d3352, 0x1000a, 40, 0, 0, payload, 0, 0, 0, 4]); }
  function read(at) {
    calls++;
    assert.equal(at >>> 0, address);
    if (closed || row.mode === 'transport') return 7;
    if (row.mode === 'malformed') { success(value); set(100, 0); return 0; }
    if (row.mode === 'legacy23') { success(value); return 23; }
    let result = 0;
    for (let index = 0; index < 4; index++) {
      const linear = (address + index) >>> 0, slot = get(backend.mappings + (linear >>> 12) * 4);
      const readable = bytes[backend.permissions + (linear >>> 12)] & 1;
      if (slot < 1 || slot > backend.pages || !readable) {
        packet([0x484d3352, 0x1000f, 40, 0, 9, 0, readable ? 4 : 5, linear, 1, 4]);
        data.setBigUint64(DESC, 0n, true);
        return 23;
      }
      result |= bytes[backend.backing + (slot - 1) * 4096 + (linear & 4095)] << (index * 8);
    }
    success(result);
    if (row.mode === 'reactivate' || (row.mode === 'witness' && phase >= 2)) data.setBigUint64(DESC, epoch, true);
    return 0;
  }
  function barrier() {
    phase++;
    if (row.trace) values.push(get(120));
    row.barrier?.({phase, memory, backend, get, set,
      get data() { return data; }, get bytes() { return bytes; },
      grow(pages) { memory.grow(pages); data = new DataView(memory.buffer); bytes = new Uint8Array(memory.buffer); }});
    if (phase !== 1) return;
    if (row.mode === 'remap') { set(MAP + first * 4, 2); set(BACK + 4096 + (address & 4095), 0x11223344); }
    if (row.mode === 'protect') bytes[PERMS + first] = 2;
    if (row.mode === 'reactivate' || row.mode === 'witness') data.setBigUint64(DESC, 0n, true);
    if (row.mode === 'closed') { data.setBigUint64(DESC, 0n, true); closed = true; }
    if (row.mode === 'context') { data.setUint16(fixture.segments + 4, 3, true); set(fixture.bases + 20, 0x8000); }
  }
  const bindings = {cpu_read32: read, cpu_read32_view: saved => {
    getters++; assert.equal(saved, epoch);
    return enabled ? row.pointer ?? DESC : 0;
  }, barrier};
  const selected = row.grow ? new WebAssembly.Module(Buffer.from(fixture.growWrapper, 'hex')) : module;
  const {run} = new WebAssembly.Instance(selected, {env: {memory}, ring3: bindings}).exports;
  const output = run(address, count, row.priorReason ?? 0).map(value => value >>> 0);
  if (row.trace && output[9] > values.length) values.push(output[8]);
  const result = {output, bytes: Buffer.from(bytes), calls, values};
  if (row.reenter) {
    const before = calls;
    result.reentry = {output: run(address, 1, row.priorReason ?? 0).map(value => value >>> 0), calls: calls - before};
  }
  assert.equal(getters, row.reenter ? 2 : 1, 'only the descriptor pointer is initialized once per call');
  return result;
}

const rows = [];
for (const value of [0, 0x80000000, 0xffffffff]) for (const offset of [0, 1, 4092]) {
  rows.push({name: `same-page ${value}/${offset}`, address: ADDRESS + offset, value, fallback: 0, literal: true});
}
for (const [name, extra] of [
  ['zero pointer', {pointer: 0}], ['descriptor tail', {pointer: BYTES - 16}], ['descriptor overflow', {pointer: 0xfffffff8}],
  ['revoked epoch', {liveEpoch: 0n}], ['different epoch', {liveEpoch: 2n}], ['epoch high word', {liveEpoch: 0x100000001n}],
  ['zero capacity', {field: [20, 0]}], ['excess capacity', {field: [20, 32769]}], ['wrapped capacity', {field: [20, 0xffffffff]}],
  ['map range', {field: [8, BYTES - 4]}], ['permission range', {field: [12, BYTES - 1]}], ['backing range', {field: [16, BYTES - 1]}],
  ['null map', {field: [8, 0]}], ['null permissions', {field: [12, 0]}], ['null backing', {field: [16, 0]}],
  ['unmapped', {mapping: 0}], ['out-of-range mapping', {mapping: 3}], ['write-only', {permission: 2}], ['guard encoding', {permission: 0x18}],
  ['malformed fallback packet', {pointer: 0, mode: 'malformed'}], ['typed transport requires typed packet', {pointer: 0, mode: 'legacy23'}],
  ['failed transport ignores stale packet', {pointer: 0, mode: 'transport'}],
]) rows.push({name, ...extra, fallback: 1});
for (const address of [ADDRESS + 4093, ADDRESS + 4094, ADDRESS + 4095, 0xfffffffe]) rows.push({name: `cross-page ${address}`, address, fallback: 1});
rows.push(
  {name: 'full64-bit admitted epoch', epoch: 0x100000001n, fallback: 0, literal: true},
  // this prior-reason input is a private emitter contract, not a claimed public caller path.
  {name: 'prior reason survives certified success', priorReason: 7, fallback: 0, expected: [0, 7, 0x91, 0xa2, 0xb3, 0xc4, 0, 0, 0xf7, 0]},
  {name: 'first read context failure precedes scratch reset', firstContext: true, fallback: 0, expected: [0, 3, 0, 0, 0, 0, 0xd5, 0xe6, 0xf7, 0]},
  {name: 'live mapping after import', count: 2, mode: 'remap', fallback: 0},
  {name: 'live permission after import', count: 2, mode: 'protect', fallback: 1},
  {name: 'reactivated epoch after fallback', count: 3, mode: 'reactivate', fallback: 1},
  {name: 'forged success cannot reactivate revoked view', count: 4, mode: 'witness', fallback: 2},
  {name: 'close retains descriptor but revokes reads', count: 2, mode: 'closed', fallback: 1},
  {name: 'live CS check still follows successful read', count: 2, mode: 'context', fallback: 0},
);
for (const row of rows) {
  const old = execute(row, false), direct = execute(row, true);
  assert.deepEqual(direct.output, old.output, `${row.name}: complete transport/terminal/scratch/value/progress tuple`);
  assert.deepEqual(direct.bytes, old.bytes, `${row.name}: complete Wasm RAM and opaque arena parity`);
  assert.equal(direct.calls, row.fallback, `${row.name}: exact optional-view/fallback branch`);
  if (row.literal) assert.deepEqual(direct.output, [0, 0, 0x91, 0xa2, 0xb3, 0xc4, 0, 0, row.value ?? 0x80000000, 1]);
  if (row.expected) assert.deepEqual(direct.output, row.expected, `${row.name}: independent literal scratch/terminal/frontier`);
  if (row.mode === 'malformed' || row.mode === 'legacy23') assert.deepEqual(direct.output.slice(0, 3), [row.mode === 'legacy23' ? 23 : 0, 7, 2]);
  if (row.mode === 'transport') assert.deepEqual(direct.output.slice(0, 3), [7, 7, 3]);
  if (row.mode === 'context') assert.deepEqual(direct.output.slice(1, 6), [3, 0, 0, 0, 0]);
  controls++;
}

for (const item of fixture.modules) {
  const generated = new WebAssembly.Module(Buffer.from(item.bytes, 'hex'));
  const imports = WebAssembly.Module.imports(generated), getter = imports.filter(item => item.name === 'cpu_read32_view');
  assert.equal(getter.length, Number(item.view), `${item.name}: exact resident/non-string eligibility`);
  if (item.view) assert.equal(imports.at(-1).name, 'cpu_read32_view', 'old import indices stay unchanged');
}
const readModule = fixture.modules.find(item => item.name === 'read' && item.view);
for (const corrupt of [null, 'state', 'segment']) {
  const memory = new WebAssembly.Memory({initial: 128}), bytes = new Uint8Array(memory.buffer), data = new DataView(memory.buffer);
  bytes.set(initial); data.setUint32(4, 0x10003, true); data.setUint32(48, 0x1000, true); data.setUint32(52, 2, true);
  let getters = 0, reads = 0;
  if (corrupt === 'state') data.setUint32(0, 0, true);
  if (corrupt === 'segment') data.setUint32(fixture.segments, 0, true);
  const bindings = {guard_detect_resident: () => 1n, finish_exception_detection: () => 0,
    cpu_read32_view: () => { getters++; return 0; }, cpu_read32: () => { reads++; return 0; }};
  const {run} = new WebAssembly.Instance(new WebAssembly.Module(Buffer.from(readModule.bytes, 'hex')), {env: {memory}, ring3: bindings}).exports;
  const before = Buffer.from(bytes), status = run(0, 56, 0, 96);
  assert.equal(status, corrupt ? 2 : 0);
  assert.equal(getters, corrupt ? 0 : 1, 'metadata query follows existing state and segment preflight');
  assert.equal(reads, 0, 'budget-zero never attempts a guest read');
  if (corrupt) assert.deepEqual(Buffer.from(bytes), before, 'failed preflight is RAM-neutral');
  controls++;
}
console.log(`direct Read32 view generated controls passed (${controls} cases, ${fixture.modules.length} composition modules)`);
