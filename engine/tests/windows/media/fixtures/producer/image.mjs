import assert from 'node:assert/strict';

const BASE = 0x400000, TEXT = 0x1000, DATA = 0x6000;
const u32 = value => {const b = Buffer.alloc(4); b.writeUInt32LE(value >>> 0); return [...b];};
const file = rva => rva >= DATA ? 0x4200 + rva - DATA : 0x200 + rva - TEXT;

export function builder(imports) {
  const bytes = Buffer.alloc(0xc200), code = [], calls = [], addresses = {}, iats = new Map();
  const word = (rva, value) => bytes.writeUInt32LE(value >>> 0, file(rva));
  const words = (rva, values) => values.forEach((value, index) => word(rva + 4 * index, value));
  const text = (rva, value) => bytes.write(value + '\0', file(rva));
  bytes.write('MZ'); bytes.writeUInt32LE(0x80, 0x3c); bytes.write('PE\0\0', 0x80);
  for (const [at, value] of [[0x84, 0x14c], [0x86, 2], [0x94, 224], [0x96, 0x102], [0x98, 0x10b], [0xdc, 3]]) bytes.writeUInt16LE(value, at);
  for (const [at, value] of [[16, TEXT], [28, BASE], [32, 4096], [36, 512], [56, 0xe000], [60, 512], [72, 0x100000], [76, 4096], [80, 0x100000], [84, 4096], [92, 16]]) bytes.writeUInt32LE(value, 0x98 + at);
  for (const [index, rva, raw, length, flags] of [[0, TEXT, 0x200, 0x4000, 0x60000020], [1, DATA, 0x4200, 0x8000, 0xc0000040]]) {
    const at = 0x178 + 40 * index;
    bytes.write(index ? '.data' : '.text', at);
    for (const [offset, value] of [[8, length], [12, rva], [16, length], [20, raw], [36, flags]]) bytes.writeUInt32LE(value, at + offset);
  }
  bytes.writeUInt32LE(DATA, 0x98 + 104); bytes.writeUInt32LE(20 * (imports.length + 1), 0x98 + 108);
  let moduleName = 0x6100, hint = 0x6600, slot = 0;
  imports.forEach(([module, symbols], index) => {
    word(DATA + 20 * index, 0x6200 + slot * 4); word(DATA + 20 * index + 12, moduleName); word(DATA + 20 * index + 16, 0x6400 + slot * 4);
    text(moduleName, module); moduleName += module.length + 1;
    for (const symbol of symbols) {
      bytes.writeUInt16LE(0, file(hint)); text(hint + 2, symbol);
      word(0x6200 + slot * 4, hint); word(0x6400 + slot * 4, hint);
      iats.set(symbol, BASE + 0x6400 + slot++ * 4); hint = (hint + symbol.length + 4) & ~1;
    }
    slot++;
  });
  assert.ok(slot * 4 < 0x200 && hint < 0x7000 && moduleName < 0x6200);
  let variable = 0x7000, constant = 0x9000;
  const allocate = (name, initial = [0]) => {const rva = variable; variable += Math.max(initial.length, 1) * 4; words(rva, initial); return addresses[name] = BASE + rva;};
  const data = (name, values) => {const rva = constant; constant += values.length * 4; words(rva, values); return addresses[name] = BASE + rva;};
  const string = (name, value) => {const rva = constant; text(rva, value); constant = (constant + value.length + 4) & ~3; return addresses[name] = BASE + rva;};
  const ref = name => ({address: addresses[name]});
  const push = value => code.push(...(typeof value === 'object' ? [0xff, 0x35, ...u32(value.address)] : [0x68, ...u32(value)]));
  const save = address => code.push(0xa3, ...u32(address));
  const write = (address, value) => code.push(0xc7, 0x05, ...u32(address), ...u32(value));
  const copy = (address, source) => {code.push(0xa1, ...u32(source)); save(address);};
  const mark = (name, id, expected, hook) => {
    const status = allocate(`status${calls.length}`);
    calls.push({name, id, end: code.length, status, expected, hook}); save(status);
  };
  const external = (name, id, args, expected = 0, output, hook) => {
    assert.ok(iats.has(name), `import ${name}`);
    [...args].reverse().forEach(push); code.push(0xff, 0x15, ...u32(iats.get(name))); mark(name, id, expected, hook); if (output) save(addresses[output]);
  };
  const com = (name, object, tag, slot, args = [], expected = 0, hook) => {
    code.push(0x8b, 0x0d, ...u32(addresses[object]), 0x8b, 0x01);
    [...args].reverse().forEach(push); code.push(0x51, 0xff, 0x90, ...u32(slot * 4)); mark(name, tag | slot, expected, hook);
  };
  const writeLocked = (pointer, values) => {
    code.push(0xa1, ...u32(pointer));
    values.forEach((value, index) => code.push(0xc7, 0x80, ...u32(index * 4), ...u32(value)));
  };
  const readLocked = (pointer, output) => {code.push(0xa1, ...u32(pointer), 0x8b, 0x00); save(output);};
  const callbacks = [{entry: BASE + 0x4f00, words: 4}, {entry: BASE + 0x4f40, words: 2}];
  for (const callback of callbacks) bytes.set([0xb8, 1, 0, 0, 0, 0xc2, callback.words * 4, 0], file(callback.entry - BASE));
  const returnPc = BASE + 0x4f80, returnId = 0x5f000002;
  bytes.set([0x0f, 0x0b], file(returnPc - BASE));
  return {allocate, data, string, ref, save, write, copy, external, com, writeLocked, readLocked, addresses, callbacks,
    seal() {
      code.push(0x0f, 0x0b);
      assert.ok(code.length < 0x3e00 && variable < 0x9000 && constant < 0xe000);
      bytes.set(code, file(TEXT));
      return {bytes, calls, addresses, callbacks, returnPc, returnId, base: BASE, entry: BASE + TEXT};
    }};
}
