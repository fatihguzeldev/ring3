import assert from 'node:assert/strict';
import {supportNames} from './link.mjs';

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

export function assertCoreNames(module, ownerEntry, identity) {
  const {moduleName, functions} = readNames(module);
  const prefix = `ring3.guest.${ownerEntry.toString(16).padStart(8, '0')}.`;
  assert(moduleName.startsWith(prefix), 'profiler module identifies the immutable owner entry');
  assert.match(moduleName.slice(prefix.length), /^[1-9][0-9]*$/, 'profiler module identifies its compilation');
  if (identity !== undefined) assert.equal(moduleName, prefix + identity);
  const numeric = WebAssembly.Module.imports(module).some(item => item.name === 'numeric');
  const precise = WebAssembly.Module.imports(module).some(item => item.name === 'precise');
  const names = ['guest.read_slow', 'guest.write_slow', ...(numeric ? ['guest.numeric'] : []),
    ...(precise ? ['guest.precise'] : []),
    ...supportNames,'guest.run', 'guest.lookup', 'guest.publish', 'guest.commit_frame_cold','guest.select_frame'];
  const wrappers = ['guest.entry', 'guest.link', 'guest.publish_link'];
  names.push(...wrappers);
  if(precise)names.push('guest.hot_entry');
  assert.deepEqual(functions, names.map((name, index) => [index, name]), 'names use actual function indices');
}

export function assertDispatchNames(module) {
  assert.deepEqual(readNames(module), {moduleName: 'ring3.dispatch',
    functions: ['guest.probe','guest.cold','guest.chain',...(WebAssembly.Module.exports(module).some(item=>item.name==='link.continuation')?['guest.continuation']:[]),...supportNames].map((name,index)=>[index,name])});
}

export function coreFunctionNames(module) {
  return readNames(module).functions;
}
