import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {host} from '../../../threads/fixtures/runtime/host.mjs';
import {image} from '../producer/scenario.mjs';

const [enginePath, output, root] = process.argv.slice(2);
mkdirSync(output, {recursive: true});
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const engineHash = hash(readFileSync(enginePath));
assert.equal(engineHash, process.env.RING3_ENGINE_SHA256);
const guest = image(), a = guest.addresses;
assert.equal(guest.calls.length, 40);
writeFileSync(join(output, 'automation-authored.exe'), guest.bytes);
const engine = host(enginePath, root, output, guest, {
  gateOwners: 12,
  configure({api}) {assert.equal(api.register_oleaut32_module(0x74000000), 0);},
});
const {api, key, get, take, read} = engine;
const methods = guest.procedures.map((procedure, index) => ({...procedure,
  ...engine.compile(`interface-${index}`, procedure.blocks, []),
}));
const returnId = 0xffffe;
let returnPc;
for (let start = 0, total = 1; start < total;) {
  assert.equal(api.module_gates_page(start, 128), 0);
  const page = take(4096), count = page.readUInt32LE(24);
  assert.equal(page.subarray(0, 4).toString(), 'R3GP');
  assert.equal(page.readUInt32LE(20), start);
  total = page.readUInt32LE(16);
  assert.ok(count > 0 && start + count <= total);
  for (let index = 0; index < count; index++) {
    if (page.readUInt32LE(36 + index * 8) === returnId) returnPc = page.readUInt32LE(32 + index * 8);
  }
  start += count;
}
assert.ok(returnPc, 'canonical Windows callback return gate');
const returned = engine.compile('interface-return', [{entry: returnPc, length: 2}],
  [{entry: returnPc, id: returnId}]);
const callbackUnits = [...methods, returned];
const table = new WebAssembly.Table({element: 'anyfunc', initial: 8, maximum: 8});
callbackUnits.forEach((unit, slot) => {
  table.set(slot, unit.generated.run);
  assert.equal(api.acknowledge_resident_installation(...key, unit.low, unit.high, slot), 0);
  const record = take(32);
  assert.equal(record.subarray(0, 4).toString(), 'R3IN');
  assert.deepEqual([16, 20, 24, 28].map(offset => record.readUInt32LE(offset)), [unit.low, unit.high, slot, 0]);
});
const callbacks = [], calls = [], arrays = {};
function finishCallbacks() {
  for (let occurrence = 0; occurrence < 8; occurrence++) {
    assert.equal(api.pending_windows_callback(), 0);
    const pending = take(96), provider = pending.readUInt32LE(12);
    if (!provider) return;
    assert.equal(provider, 9);
    const procedure = pending.readUInt32LE(16), count = pending.readUInt32LE(20);
    const method = methods.find(method => method.entry === procedure);
    assert.ok(method); assert.equal(count, method.words);
    const object = pending.readUInt32LE(32);
    assert.ok([a.first, a.old, a.rejecting, a.numeric, a.failure, a.conversion, a.arrayObject].includes(object));
    if (count === 3) {
      const iid = pending.readUInt32LE(36);
      assert.deepEqual([0, 4, 8, 12].map(offset => read(iid + offset)), [0x20400, 0, 0xc0, 0x46000000]);
    }
    if (count === 9) {
      assert.equal(pending.readUInt32LE(36), 0);
      const iid = pending.readUInt32LE(40), parameters = pending.readUInt32LE(52);
      assert.deepEqual([0, 4, 8, 12].map(offset => read(iid + offset)), [0, 0, 0, 0]);
      assert.equal(pending.readUInt32LE(44), 0x407);
      assert.equal(pending.readUInt32LE(48), 2);
      assert.deepEqual([0, 4, 8, 12].map(offset => read(parameters + offset)), [0, 0, 0, 0]);
      assert.deepEqual([pending.readUInt32LE(60), pending.readUInt32LE(64)], [0, 0]);
    }
    assert.equal(api.begin_windows_resident_callback(...key, method.low, method.high, returnPc, returnId), 0);
    const record = take(72), token = record.readUInt32LE(16);
    assert.equal(record.subarray(0, 4).toString(), 'R3RC');
    assert.equal(record.readUInt32LE(48), count);
    const home = [record.readUInt32LE(64), record.readUInt32LE(68)];
    assert.deepEqual(home, [method.low, method.high]);
    assert.equal(api.authorize_resident_callback(...key, ...home, token), 0);
    let active = method, select = false, completed = false, retired = 0;
    const runs = [];
    for (let iteration = 0; iteration < 16; iteration++) {
      if (select) {
        const pc = get(48);
        active = pc === returnPc ? returned : methods.find(method => pc >= method.entry && pc < method.entry + method.length);
        assert.ok(active, `authored callback PC ${pc.toString(16)}`);
        assert.equal(api.select_resident_callback_unit(...key, ...home, token, active.low, active.high), 0);
      }
      active.generated.run(api.arena_ptr(), api.arena_ptr() + 56, 1000, api.arena_ptr() + 96);
      const reason = get(72), detail = get(80), count = get(76);
      retired += count; runs.push({reason, detail, count, pc: get(48)});
      if (reason === 1 || reason === 3) {select = reason === 3; continue;}
      assert.equal(reason, 8); assert.equal(detail, returnId); assert.equal(get(48), returnPc);
      assert.ok(retired > 0);
      const expected = method.result ?? (procedure === guest.procedures[0].entry ? (object === a.rejecting ? 0x80004002 : 0) : read(object + 4));
      assert.equal(get(16), expected, 'actual native-method guest return');
      completed = true; break;
    }
    assert.ok(completed, 'bounded interface guest callback reaches the real return gate');
    assert.equal(api.finish_windows_resident_callback(...key, ...home, token), 0);
    callbacks.push({procedure, object, count, runs});
  }
  assert.fail('bounded callbacks for one Automation call');
}
function object(pointer) {return [4, 8, 12, 16].map(offset => read(pointer + offset));}
try {
  for (const unit of engine.units) {
    const destinationBefore = [0, 4, 8, 12].map(offset => read(a.destination + offset));
    const owner = engine.run(unit);
    assert.equal(api.capture_resident_call(...key, owner.low, owner.high, 2, unit.words), 0);
    const token = take(112).readUInt32LE(16);
    assert.equal(api.complete_resident_windows_call(...key, owner.low, owner.high, token), 0);
    if (unit.expected === 'exit') {
      assert.equal(get(72), 9); assert.equal(get(80), 0);
    } else {
      finishCallbacks();
      if (unit.expected === 'pointer') assert.ok(get(16) > 0, `${unit.name} owns a nonnull native allocation`);
      else assert.equal(get(16), unit.expected, unit.name);
    }
    calls.push({name: unit.name, token, hook: unit.hook});
    if (unit.hook === 'copy') {
      assert.deepEqual(object(a.first), [2, 1, 0, 0]); assert.deepEqual(object(a.old), [0, 0, 1, 0]);
      assert.deepEqual([0, 4, 8, 12].map(offset => read(a.destination + offset)), [13, 0, a.first, 0]);
    }
    if (unit.hook === 'clear-copy') assert.deepEqual(object(a.first), [1, 1, 1, 0]);
    if (unit.hook === 'query') assert.deepEqual(object(a.first), [2, 2, 1, 1]);
    if (unit.hook === 'same-type') assert.deepEqual(object(a.first), [3, 3, 1, 1]);
    if (unit.hook === 'query-failure') {
      assert.deepEqual(object(a.rejecting), [1, 0, 0, 1]);
      assert.deepEqual([0, 4, 8, 12].map(offset => read(a.destination + offset)), [3, 0, 37, 0]);
    }
    if (unit.hook === 'indirect') assert.deepEqual(object(a.first), [2, 4, 3, 1]);
    if (unit.hook === 'borrowed') {
      assert.deepEqual(object(a.first), [1, 4, 4, 1]);
      assert.deepEqual([0, 4, 8, 12].map(offset => read(a.destination + offset)), [0x400d, 0, a.borrowed, 0]);
    }
    if (unit.hook === 'clear-borrowed') assert.deepEqual(object(a.first), [1, 4, 4, 1]);
    const kind = unit.hook;
    if (kind === 'array-created') {
      arrays.initial = get(16);
      assert.equal(read(arrays.initial) >>> 16, 0x2280);
      assert.deepEqual([8, 16, 20].map(offset => read(arrays.initial + offset)), [0, 2, 0]);
    }
    if (kind === 'array-put-zero' || kind === 'array-put-one') {
      assert.equal(read(a.array), arrays.initial);
      assert.deepEqual(object(a.arrayObject), [kind === 'array-put-zero' ? 2 : 3, kind === 'array-put-zero' ? 1 : 2, 0, 0]);
      assert.equal(read(arrays.initial + 8), 0);
      const data = read(arrays.initial + 12);
      assert.equal(read(data), a.arrayObject);
      assert.equal(read(data + 4), kind === 'array-put-one' ? a.arrayObject : 0);
    }
    if (kind === 'array-get') {
      assert.equal(read(a.element), a.arrayObject);
      assert.deepEqual(object(a.arrayObject), [4, 3, 0, 0]);
      assert.equal(read(arrays.initial + 8), 0);
    }
    if (kind === 'array-release-get') assert.deepEqual(object(a.arrayObject), [3, 3, 1, 0]);
    if (kind === 'array-copy') {
      arrays.copy = read(a.arrayCopy);
      assert.ok(arrays.copy > 0 && arrays.copy !== arrays.initial);
      const data = read(arrays.copy + 12);
      assert.deepEqual([read(data), read(data + 4)], [a.arrayObject, a.arrayObject]);
      assert.deepEqual(object(a.arrayObject), [5, 5, 1, 0]);
    }
    if (kind === 'array-redim') {
      assert.deepEqual([16, 20].map(offset => read(arrays.initial + offset)), [1, 7]);
      assert.equal(read(read(arrays.initial + 12)), a.arrayObject);
      assert.deepEqual(object(a.arrayObject), [4, 5, 2, 0]);
    }
    if (kind === 'array-destroy-copy') assert.deepEqual(object(a.arrayObject), [2, 5, 4, 0]);
    if (kind === 'array-destroy') assert.deepEqual(object(a.arrayObject), [1, 5, 5, 0]);
    if (kind === 'array-owner-release') assert.deepEqual(object(a.arrayObject), [0, 5, 6, 0]);
    if (kind === 'variant-string-created') {
      arrays.sourceString = get(16);
      assert.equal(read(arrays.sourceString - 4), 6);
      assert.deepEqual([read(arrays.sourceString), read(arrays.sourceString + 4)], [0x61, 0x7a]);
    }
    if (kind === 'variant-array-created') {
      arrays.variant = get(16);
      assert.equal(read(arrays.variant) >>> 16, 0x2880);
    }
    if (kind === 'variant-array-put') {
      assert.equal(read(a.variantArray), arrays.variant);
      const element = read(arrays.variant + 12);
      assert.equal(read(element), 8);
      arrays.elementString = read(element + 8);
      assert.ok(arrays.elementString > 0 && arrays.elementString !== arrays.sourceString);
      assert.equal(read(arrays.elementString - 4), 6);
      assert.deepEqual([read(arrays.elementString), read(arrays.elementString + 4)], [0x61, 0x7a]);
    }
    if (kind === 'variant-source-clear') assert.equal(read(a.stringSource), 0);
    if (kind === 'variant-array-get') {
      assert.deepEqual([read(a.variantReturned), read(a.variantReturned + 4)], [8, 0]);
      arrays.resultString = read(a.variantReturned + 8);
      assert.ok(arrays.resultString > 0 && arrays.resultString !== arrays.elementString);
      assert.equal(read(arrays.resultString - 4), 6);
      assert.deepEqual([read(arrays.resultString), read(arrays.resultString + 4)], [0x61, 0x7a]);
      assert.equal(read(arrays.variant + 8), 0);
    }
    if (kind === 'variant-array-copy') {
      const copied = read(a.variantCopy), element = read(copied + 12), string = read(element + 8);
      assert.ok(copied > 0 && copied !== arrays.variant);
      assert.equal(read(element), 8);
      assert.ok(string > 0 && string !== arrays.elementString && string !== arrays.resultString);
      assert.equal(read(string - 4), 6);
      assert.deepEqual([read(string), read(string + 4)], [0x61, 0x7a]);
    }
    if (kind === 'variant-result-clear') assert.equal(read(a.variantReturned), 0);
    if (unit.hook?.startsWith('invoke-')) {
      const pointer = unit.hook === 'invoke-numeric' ? a.numeric : unit.hook === 'invoke-failure' ? a.failure : a.conversion;
      assert.deepEqual([24, 28, 32, 36, 40, 44, 48, 56, 60].map(offset => read(pointer + offset)), [1, 0, 0x407, 2, 0, 0, 0, 0, 0]);
      assert.ok(read(pointer + 52) > 0, 'real Invoke VARIANT result pointer');
      assert.deepEqual(object(pointer), unit.hook === 'invoke-numeric' ? [1, 0, 0, 0] : [1, 1, 1, 0]);
      assert.deepEqual([0, 4, 8, 12].map(offset => read(a.destination + offset)), unit.hook === 'invoke-numeric' ? [3, 0, 37, 0] : destinationBefore);
    }
  }
  assert.equal(calls.length, 40); assert.equal(callbacks.length, 31);
  assert.deepEqual(object(a.first), [0, 4, 5, 1]);
  assert.deepEqual(object(a.old), [0, 0, 1, 0]);
  assert.deepEqual(object(a.rejecting), [0, 0, 1, 1]);
  assert.deepEqual(object(a.arrayObject), [0, 5, 6, 0]);
  assert.deepEqual(object(a.numeric), [0, 0, 1, 0]);
  assert.deepEqual(object(a.failure), [0, 1, 2, 0]);
  assert.deepEqual(object(a.conversion), [0, 1, 2, 0]);
  assert.deepEqual(callbacks.map(callback => [callback.procedure, callback.object]), [
    [guest.procedures[1].entry, a.first], [guest.procedures[2].entry, a.old],
    [guest.procedures[2].entry, a.first], [guest.procedures[0].entry, a.first],
    [guest.procedures[1].entry, a.first], [guest.procedures[2].entry, a.first],
    [guest.procedures[2].entry, a.first], [guest.procedures[0].entry, a.rejecting],
    [guest.procedures[1].entry, a.first], [guest.procedures[2].entry, a.first],
    [guest.procedures[2].entry, a.rejecting], [guest.procedures[2].entry, a.first],
    [guest.procedures[3].entry, a.numeric],
    [guest.procedures[4].entry, a.failure], [guest.procedures[2].entry, a.failure],
    [guest.procedures[5].entry, a.conversion], [guest.procedures[2].entry, a.conversion],
    [guest.procedures[2].entry, a.numeric], [guest.procedures[2].entry, a.failure], [guest.procedures[2].entry, a.conversion],
    [guest.procedures[1].entry, a.arrayObject], [guest.procedures[1].entry, a.arrayObject],
    [guest.procedures[1].entry, a.arrayObject], [guest.procedures[2].entry, a.arrayObject],
    [guest.procedures[1].entry, a.arrayObject], [guest.procedures[1].entry, a.arrayObject],
    [guest.procedures[2].entry, a.arrayObject], [guest.procedures[2].entry, a.arrayObject],
    [guest.procedures[2].entry, a.arrayObject], [guest.procedures[2].entry, a.arrayObject], [guest.procedures[2].entry, a.arrayObject],
  ]);
  const receipt = {engineHash, guestHash: hash(guest.bytes), calls: calls.length, callbacks: callbacks.length,
    objects: {first: object(a.first), old: object(a.old), rejecting: object(a.rejecting), numeric: object(a.numeric), failure: object(a.failure), conversion: object(a.conversion)},
    arrays, arrayObject: object(a.arrayObject), nativeMethodCallbacks: callbacks, runs: engine.runs,
    claim: 'authored PE32 executes generated Wasm AddRef/Release/QI and IDispatch Invoke methods through provider9; exact native Invoke signature, numeric37, owned-result Release on Invoke/conversion failure, prior copy/byref/E_NOINTERFACE oracles, interface SAFEARRAY Put/Get/Copy/Redim/Destroy exact5AddRefs/6Releases, VARIANT arrays with independent embedded-NUL BSTRs and uninitialized Get destination, and final zero-reference guest counters'};
  writeFileSync(join(output, 'receipt.json'), JSON.stringify(receipt, null, 2));
  console.log(JSON.stringify({engineHash, calls: calls.length, callbacks: callbacks.length}));
} catch (error) {
  writeFileSync(join(output, 'progress.json'), JSON.stringify({engineHash, calls, callbacks, pc: get(48), error: error.stack}, null, 2));
  throw error;
}
