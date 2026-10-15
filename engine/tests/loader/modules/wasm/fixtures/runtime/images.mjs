import assert from 'node:assert/strict';

const TEXT = 0x1000, DATA = 0x2000;
const file = rva => rva < 0x200 ? rva : rva >= TEXT && rva < TEXT + 0x200 ? 0x200 + rva - TEXT : 0x400 + rva - DATA;
const u32 = value => {const bytes = Buffer.alloc(4); bytes.writeUInt32LE(value >>> 0); return [...bytes];};

export function image({base, dll = false, imports = [], exports = [], tls = false, fail = false, game = false, answer = 77}) {
  const bytes = Buffer.alloc(0x2400), relocations = [], code = [];
  const word = (rva, value) => bytes.writeUInt32LE(value >>> 0, file(rva));
  const pointer = (rva, value) => {word(rva, base + value); relocations.push(rva);};
  const text = (rva, value) => bytes.write(value + '\0', file(rva));
  bytes.write('MZ'); bytes.writeUInt32LE(0x80, 0x3c); bytes.write('PE\0\0', 0x80);
  for (const [at, value] of [[0x84, 0x14c], [0x86, 2], [0x94, 224], [0x96, dll ? 0x2102 : 0x102], [0x98, 0x10b], [0xdc, 3]]) bytes.writeUInt16LE(value, at);
  for (const [at, value] of [[16, dll ? 0x1020 : TEXT], [28, base], [32, 4096], [36, 512], [56, 0x4000], [60, 512], [72, 0x100000], [76, 4096], [80, 0x100000], [84, 4096], [92, 16]]) bytes.writeUInt32LE(value, 0x98 + at);
  for (const [index, rva, raw, length, flags] of [[0, TEXT, 0x200, 0x200, 0x60000020], [1, DATA, 0x400, 0x2000, 0xc0000040]]) {
    const at = 0x178 + 40 * index;
    for (const [offset, value] of [[8, length], [12, rva], [16, length], [20, raw], [36, flags]]) bytes.writeUInt32LE(value, at + offset);
  }
  const directory = (index, rva, length) => {bytes.writeUInt32LE(rva, 0x98 + 96 + 8 * index); bytes.writeUInt32LE(length, 0x98 + 100 + 8 * index);};
  const functions = new Map(), boundaries = new Map();
  function procedure(rva, instructions, ends = [instructions.length]) {assert.ok(rva + instructions.length <= TEXT + 0x200); bytes.set(instructions, file(rva)); functions.set(rva, Buffer.from(instructions)); boundaries.set(rva, ends);}
  function absolute(instructions, rva, opcode, address) {instructions.push(...opcode); relocations.push(rva + instructions.length); instructions.push(...u32(base + address));}
  const iats = new Map();
  if (imports.length) {
    directory(1, 0x2000, 20 * (imports.length + 1));
    let moduleName = 0x2100, hint = 0x2400, slot = 0;
    imports.forEach((item, index) => {
      word(0x2000 + 20 * index, item.oftZero ? 0 : 0x2200 + slot * 4);
      word(0x200c + 20 * index, moduleName); word(0x2010 + 20 * index, 0x2300 + slot * 4);
      text(moduleName, item.module); moduleName += item.module.length + 1;
      for (const symbol of item.symbols) {
        const value = typeof symbol === 'number' ? (0x80000000 | symbol) >>> 0 : hint;
        if (typeof symbol === 'string') {bytes.writeUInt16LE(0, file(hint)); text(hint + 2, symbol); hint = (hint + symbol.length + 4) & ~1;}
        word(0x2200 + slot * 4, value); word(0x2300 + slot * 4, value);
        iats.set(`${item.module}:${symbol}`, 0x2300 + slot++ * 4);
      }
      slot++;
    });
  }
  if (exports.length) {
    directory(0, 0x2500, 0x200);
    const ordinalBase = Math.min(...exports.map(item => item.ordinal)), count = Math.max(...exports.map(item => item.ordinal)) - ordinalBase + 1;
    const named = exports.filter(item => item.name).sort((a, b) => a.name < b.name ? -1 : 1);
    for (const [offset, value] of [[16, ordinalBase], [20, count], [24, named.length], [28, 0x2540], [32, 0x2580], [36, 0x25c0]]) word(0x2500 + offset, value);
    let cursor = 0x2600;
    for (const item of exports) {
      const target = typeof item.target === 'string' ? cursor : item.target;
      if (typeof item.target === 'string') {text(cursor, item.target); cursor += item.target.length + 1;}
      word(0x2540 + (item.ordinal - ordinalBase) * 4, target);
    }
    named.forEach((item, index) => {word(0x2580 + index * 4, cursor); text(cursor, item.name); cursor += item.name.length + 1; bytes.writeUInt16LE(item.ordinal - ordinalBase, file(0x25c0 + index * 2));});
    assert.ok(cursor < 0x2700);
  }
  if (tls) {
    directory(9, 0x2800, 24);
    for (const [at, rva] of [[0x2800, 0x2840], [0x2804, 0x2844], [0x2808, 0x2850], [0x280c, 0x2860], [0x2860, 0x1060]]) pointer(at, rva);
    word(0x2810, 8); word(0x2814, 0x00400000); word(0x2840, 0x0a090807);
    const callback = [0x64, 0xa1, 0x2c, 0, 0, 0];
    absolute(callback, 0x1060, [0x8b, 0x0d], 0x2850);
    callback.push(0x8b, 0x14, 0x88, 0xff, 0x02, 0x8b, 0x44, 0x24, 8);
    absolute(callback, 0x1060, [0xa3], 0x2908);
    callback.push(0xc2, 12, 0); procedure(0x1060, callback);
  }
  if (dll) {
    procedure(TEXT, [0xb8, ...u32(answer), 0xc3]);
    const main = [0x8b, 0x44, 0x24, 8];
    absolute(main, 0x1020, [0xa3], 0x2904);
    main.push(0xb8, fail ? 0 : 1, 0, 0, 0, 0xc2, 12, 0); procedure(0x1020, main);
  } else if (game) {
    const ends = [];
    const callImport = (module, symbol) => {absolute(code, TEXT, [0xff, 0x15], iats.get(`${module}:${symbol}`)); ends.push(code.length);};
    callImport('dep.dll', 'Answer'); absolute(code, TEXT, [0xa3], 0x2900);
    callImport('dep.dll', 7); absolute(code, TEXT, [0xa3], 0x290c);
    absolute(code, TEXT, [0x68], 0x2a00); callImport('kernel32.dll', 'LoadLibraryA'); code.push(0x89, 0xc3);
    absolute(code, TEXT, [0x68], 0x2a20); code.push(0x53); callImport('kernel32.dll', 'GetProcAddress');
    absolute(code, TEXT, [0xa3], 0x2b80); absolute(code, TEXT, [0xff, 0x15], 0x2b80); ends.push(code.length); absolute(code, TEXT, [0xa3], 0x2910);
    code.push(0x53); callImport('kernel32.dll', 'FreeLibrary'); absolute(code, TEXT, [0xa3], 0x2914);
    code.push(0x6a, 0); callImport('kernel32.dll', 'ExitProcess'); code.push(0x0f, 0x0b);
    text(0x2a00, 'forwarder.dll'); text(0x2a20, 'Run'); procedure(TEXT, code, [...ends, code.length]);
    directory(13, 0x2b00, 64);
    for (const [at, value] of [[0x2b00, 1], [0x2b04, 0x2b50], [0x2b08, 0x2b70], [0x2b0c, 0x2b80], [0x2b10, 0x2ba0], [0x2b80, 0x2bc0], [0x2ba0, 0x2bc0]]) word(at, value);
    text(0x2b50, 'forwarder.dll'); text(0x2bc2, 'Run');
  } else procedure(TEXT, [0xb8, 42, 0, 0, 0, 0x0f, 0x0b]);
  if (relocations.length) {
    let at = 0x3e00;
    for (const page of [...new Set(relocations.map(rva => rva & ~0xfff))].sort((a, b) => a - b)) {
      const slots = relocations.filter(rva => (rva & ~0xfff) === page).sort((a, b) => a - b).map(rva => 0x3000 | (rva & 0xfff));
      if (slots.length % 2) slots.push(0);
      word(at, page); word(at + 4, 8 + slots.length * 2);
      slots.forEach((value, index) => bytes.writeUInt16LE(value, file(at + 8 + index * 2)));
      at += 8 + slots.length * 2;
    }
    directory(5, 0x3e00, at - 0x3e00);
  }
  return {base, bytes, functions, boundaries, iats};
}
