import assert from 'node:assert/strict';

function readNames(module) {
  const sections = WebAssembly.Module.customSections(module, 'name');
  assert.equal(sections.length, 1, 'generated module has one profiler name section');
  const bytes = new Uint8Array(sections[0]);
  let cursor = 0, moduleName;
  const functions = [];
  const unsigned = () => {
    let value = 0;
    for (let shift = 0; shift < 35; shift += 7) {
      assert(cursor < bytes.length, 'complete name integer');
      const byte = bytes[cursor++];
      assert(shift < 28 || (byte & 0xf0) === 0, 'u32 name integer');
      value += (byte & 127) * 2 ** shift;
      if (byte < 128) return value;
    }
    assert.fail('unterminated name integer');
  };
  const text = () => {
    const length = unsigned(), end = cursor + length;
    assert(end <= bytes.length, 'complete name string');
    const value = new TextDecoder('utf-8', {fatal: true}).decode(bytes.subarray(cursor, end));
    cursor = end;
    return value;
  };
  while (cursor < bytes.length) {
    const kind = bytes[cursor++], length = unsigned(), end = cursor + length;
    assert(end <= bytes.length, 'complete name subsection');
    if (kind === 0) { assert.equal(moduleName, undefined); moduleName = text(); }
    else {
      assert.equal(kind, 1, 'module and function names only');
      const count = unsigned();
      for (let index = 0; index < count; index++) functions.push([unsigned(), text()]);
    }
    assert.equal(cursor, end, 'consumed complete name subsection');
  }
  return {moduleName, functions};
}

export function assertCoreNames(module, entry, identity) {
  const {moduleName, functions} = readNames(module);
  const prefix = `ring3.guest.${entry.toString(16).padStart(8, '0')}.`;
  assert(moduleName.startsWith(prefix), 'profiler module identifies the requested guest entry');
  assert.match(moduleName.slice(prefix.length), /^[1-9][0-9]*$/, 'profiler module identifies its compilation');
  if (identity !== undefined) assert.equal(moduleName, prefix + identity);
  const numeric = WebAssembly.Module.imports(module).some(item => item.name === 'numeric');
  const names = ['guest.read_slow', 'guest.write_slow', ...(numeric ? ['guest.numeric'] : []),
    'guest.flags', 'guest.run', 'guest.lookup', 'guest.publish', 'guest.precise', 'guest.materialize_numeric', 'guest.resolve_status',
    'guest.translate_page'];
  const wrappers = ['guest.entry', 'guest.link', 'guest.publish_link'];
  const stores = functions.slice(names.length,-wrappers.length).map(([, name]) => name);
  assert(stores.length <= 3 && stores.every(name => ['guest.store8', 'guest.store16', 'guest.store32'].includes(name)),
    'only supported fixed-width store helpers follow the numerical materializer');
  assert.equal(new Set(stores).size, stores.length, 'store helper widths are unique');
  names.push(...stores,...wrappers);
  assert.deepEqual(functions, names.map((name, index) => [index, name]), 'names use actual function indices');
}

export function assertDispatchNames(module) {
  assert.deepEqual(readNames(module), {moduleName: 'ring3.dispatch',
    functions: [[0, 'guest.probe'], [1, 'guest.cold'], [2, 'guest.chain']]});
}

export function coreFunctionNames(module) {
  return readNames(module).functions;
}
