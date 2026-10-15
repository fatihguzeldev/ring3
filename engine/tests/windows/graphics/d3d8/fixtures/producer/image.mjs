import assert from 'node:assert/strict';

const BASE = 0x400000, TEXT = 0x1000, DATA = 0x4000;
const u32 = value => {const b = Buffer.alloc(4); b.writeUInt32LE(value >>> 0); return [...b];};
const file = rva => rva >= DATA ? 0x2200 + rva - DATA : 0x200 + rva - TEXT;
const gate = (tag, slot) => 0x40000 | tag << 8 | slot;

export function image() {
  const bytes = Buffer.alloc(0x6200), code = [], calls = [], addresses = {}, iats = new Map();
  const word = (rva, value) => bytes.writeUInt32LE(value >>> 0, file(rva));
  const words = (rva, values) => values.forEach((value, index) => word(rva + 4 * index, value));
  const text = (rva, value) => bytes.write(value + '\0', file(rva));
  bytes.write('MZ'); bytes.writeUInt32LE(0x80, 0x3c); bytes.write('PE\0\0', 0x80);
  for (const [at, value] of [[0x84, 0x14c], [0x86, 2], [0x94, 224], [0x96, 0x102], [0x98, 0x10b], [0xdc, 3]]) bytes.writeUInt16LE(value, at);
  for (const [at, value] of [[16, TEXT], [28, BASE], [32, 4096], [36, 512], [56, 0x8000], [60, 512], [72, 0x100000], [76, 4096], [80, 0x100000], [84, 4096], [92, 16]]) bytes.writeUInt32LE(value, 0x98 + at);
  for (const [index, rva, raw, length, flags] of [[0, TEXT, 0x200, 0x2000, 0x60000020], [1, DATA, 0x2200, 0x4000, 0xc0000040]]) {
    const at = 0x178 + 40 * index;
    bytes.write(index ? '.data' : '.text', at);
    for (const [offset, value] of [[8, length], [12, rva], [16, length], [20, raw], [36, flags]]) bytes.writeUInt32LE(value, at + offset);
  }
  const imports = [
    ['kernel32.dll', ['ExitProcess']], ['user32.dll', ['RegisterClassA', 'CreateWindowExA', 'DestroyWindow']],
    ['d3d8.dll', ['Direct3DCreate8']],
  ];
  bytes.writeUInt32LE(DATA, 0x98 + 104); bytes.writeUInt32LE(80, 0x98 + 108);
  let moduleName = 0x4100, hint = 0x4400, slot = 0;
  imports.forEach(([module, symbols], index) => {
    word(DATA + 20 * index, 0x4200 + slot * 4); word(DATA + 20 * index + 12, moduleName); word(DATA + 20 * index + 16, 0x4300 + slot * 4);
    text(moduleName, module); moduleName += module.length + 1;
    for (const symbol of symbols) {
      bytes.writeUInt16LE(0, file(hint)); text(hint + 2, symbol);
      word(0x4200 + slot * 4, hint); word(0x4300 + slot * 4, hint);
      iats.set(symbol, BASE + 0x4300 + slot++ * 4); hint = (hint + symbol.length + 4) & ~1;
    }
    slot++;
  });
  let variable = 0x4800, constant = 0x5400;
  const allocate = (name, initial = [0]) => {const rva = variable; variable += Math.max(initial.length, 1) * 4; words(rva, initial); return addresses[name] = BASE + rva;};
  const data = (name, values) => {const rva = constant; constant += values.length * 4; words(rva, values); return addresses[name] = BASE + rva;};
  for (const name of ['window', 'd3d', 'device', 'vb', 'bits', 'source', 'texture', 'vs', 'ps', 'block', 'surface', 'back', 'chain', 'chainBack']) allocate(name);
  allocate('locked', [0, 0]); allocate('size', [1]); allocate('small', [0xdddddddd]); allocate('observed', [0]); allocate('observedBack', [0]);
  const className = BASE + 0x5200, title = BASE + 0x5220; text(0x5200, 'Ring3D3D8Fixture'); text(0x5220, 'authored guest');
  const callback = BASE + 0x2f00, returnPc = BASE + 0x2f20, returnId = 0x5f000001;
  bytes.set([0xb8, 1, 0, 0, 0, 0xc2, 16, 0], file(0x2f00)); bytes.set([0x0f, 0x0b], file(0x2f20));
  const wndclass = data('wndclass', [0, callback, 0, 0, BASE, 0, 0, 0, 0, className]);
  const pp = data('pp', [16, 16, 21, 1, 0, 3, 0, 1, 0, 0, 1, 0, 0]);
  const vertices = data('vertices', [0, 0, 0, 0x3f800000, 0xffff0000, 0, 0,
    0x41800000, 0, 0, 0x3f800000, 0xff00ff00, 0x3f800000, 0,
    0, 0x41800000, 0, 0x3f800000, 0xff0000ff, 0, 0x3f800000]);
  const declaration = data('declaration', [0x20000000, 0x40020000, 0xffffffff]);
  const vsProgram = data('vsProgram', [0xfffe0101, 1, 0xc00f0000, 0x90e40000, 0xffff]);
  const psProgram = data('psProgram', [0xffff0101, 1, 0x800f0000, 0x90e40000, 0xffff]);
  const constants = data('constants', [0x3f800000, 0, 0, 0x3f800000]);
  const material = data('material', [0x3f800000, 0x3f800000, 0x3f800000, 0x3f800000, ...Array(12).fill(0), 0]);
  const light = data('light', [3, 0x3f800000, 0x3f800000, 0x3f800000, 0, ...Array(11).fill(0), 0x3f800000, ...Array(9).fill(0)]);
  const identity = data('identity', Array.from({length: 16}, (_, i) => i % 5 === 0 ? 0x3f800000 : 0));
  const ref = name => ({address: addresses[name]});
  const push = value => code.push(...(typeof value === 'object' ? [0xff, 0x35, ...u32(value.address)] : [0x68, ...u32(value)]));
  const save = address => code.push(0xa3, ...u32(address));
  const write = (address, value) => code.push(0xc7, 0x05, ...u32(address), ...u32(value));
  const mark = (name, id, expected = 0, hook) => {
    const status = allocate(`status${calls.length}`);
    calls.push({name, id, end: code.length, status, expected, hook}); save(status);
  };
  function external(name, id, args, expected = 0, output, hook) {
    [...args].reverse().forEach(push); code.push(0xff, 0x15, ...u32(iats.get(name))); mark(name, id, expected, hook); if (output) save(addresses[output]);
  }
  function com(name, object, tag, slot, args = [], expected = 0, hook) {
    code.push(0x8b, 0x0d, ...u32(addresses[object]), 0x8b, 0x01);
    [...args].reverse().forEach(push); code.push(0x51, 0xff, 0x90, ...u32(slot * 4)); mark(name, gate(tag, slot), expected, hook);
  }
  const device = (name, slot, args = [], expected = 0, hook) => com(name, 'device', 2, slot, args, expected, hook);
  function writeLocked(pointer, values) {
    code.push(0xa1, ...u32(pointer));
    values.forEach((value, index) => code.push(0xc7, 0x80, ...u32(index * 4), ...u32(value)));
  }
  external('RegisterClassA', 0x20001, [wndclass], 'atom');
  external('CreateWindowExA', 0x20007, [0, className, title, 0x10000000, 0, 0, 16, 16, 0, 0, BASE, 0], 'pointer', 'window');
  external('Direct3DCreate8', 0x50001, [220], 'pointer', 'd3d');
  com('CreateDevice', 'd3d', 1, 15, [0, 1, ref('window'), 0x40, pp, addresses.device]);
  device('CreateVertexBuffer', 23, [84, 0, 0x144, 1, addresses.vb]);
  com('LockVB', 'vb', 8, 11, [0, 84, addresses.bits, 0]);
  writeLocked(addresses.bits, Array.from({length: 21}, (_, i) => bytes.readUInt32LE(file(vertices - BASE) + i * 4)));
  com('UnlockVB', 'vb', 8, 12);
  device('SetStream', 83, [0, ref('vb'), 28]); device('SetFVF', 76, [0x144]);
  device('CreateSystemTexture', 20, [4, 4, 2, 0, 21, 2, addresses.source]);
  for (let level = 0; level < 2; level++) {
    com(`LockTexture${level}`, 'source', 5, 16, [level, addresses.locked, 0, 0]);
    writeLocked(addresses.locked + 4, Array((4 >> level) ** 2).fill(0xff123456 + level));
    com(`UnlockTexture${level}`, 'source', 5, 17, [level]);
  }
  device('CreateTexture', 20, [4, 4, 2, 0, 21, 0, addresses.texture]);
  device('UpdateTexture', 29, [ref('source'), ref('texture')]); device('SetTexture', 61, [0, ref('texture')]);
  device('SetLighting', 50, [137, 0]); device('SetMaterial', 42, [material]);
  device('SetLight', 44, [0, light]); device('LightEnable', 46, [0, 1]); device('SetTransform', 37, [256, identity]);
  device('BeginBlock', 52); device('RecordAlphaBlend', 50, [27, 1]); device('EndBlock', 53, [addresses.block]);
  device('ApplyBlock', 54, [ref('block')]); device('CaptureBlock', 55, [ref('block')]); device('DeleteBlock', 56, [ref('block')]);
  device('BeginSceneFF', 34); device('Clear', 36, [0, 0, 1, 0xff102030, 0x3f800000, 0]);
  device('DrawFF', 70, [4, 0, 1]); device('EndSceneFF', 35);
  device('CreateVS', 75, [declaration, vsProgram, addresses.vs, 0]); device('SetVS', 76, [ref('vs')]);
  device('SetVSConstants', 79, [0, constants, 1]);
  device('GetVSFunctionSmall', 82, [ref('vs'), addresses.small, addresses.size], 0x8876086c);
  device('CreatePS', 87, [psProgram, addresses.ps]); device('SetPS', 88, [ref('ps')]); device('SetPSConstants', 91, [0, constants, 1]);
  device('BeginSceneShader', 34); device('DrawShader', 70, [4, 0, 1]); device('EndSceneShader', 35);
  device('Present', 15, [0, 0, 0, 0]);
  device('GetBackBuffer', 16, [0, 0, addresses.back]);
  device('CreateImageSurface', 27, [64, 64, 21, addresses.surface]);
  device('CopyRects', 28, [ref('back'), 0, 0, ref('surface'), 0]);
  com('LockGPUBackBuffer', 'back', 10, 9, [addresses.locked, 0, 0x10]);
  code.push(0xa1, ...u32(addresses.locked + 4), 0x8b, 0x00); save(addresses.observedBack);
  com('UnlockGPUBackBuffer', 'back', 10, 10);
  com('ReleaseBackBuffer', 'back', 10, 2); device('GetFrontBuffer', 30, [ref('surface')]);
  com('LockReadbackSurface', 'surface', 10, 9, [addresses.locked, 0, 0x10]);
  code.push(0xa1, ...u32(addresses.locked + 4), 0x8b, 0x00); save(addresses.observed);
  com('UnlockReadbackSurface', 'surface', 10, 10); com('ReleaseReadbackSurface', 'surface', 10, 2);
  device('UnbindTexture', 61, [0, 0]); device('UnbindStream', 83, [0, 0, 0]);
  com('ReleaseTexture', 'texture', 5, 2); com('ReleaseSource', 'source', 5, 2); com('ReleaseVB', 'vb', 8, 2);
  device('TestLost', 3, [], 0x88760868, 'lost'); device('TestResettable', 3, [], 0x88760869, 'resettable');
  device('Reset', 14, [pp]); device('DeleteVSAfterReset', 78, [ref('vs')]); device('DeletePSAfterReset', 90, [ref('ps')]);
  device('ClearAfterReset', 36, [0, 0, 1, 0xff203040, 0x3f800000, 0]);
  device('PresentAfterShaderDelete', 15, [0, 0, 0, 0]);
  com('ReleaseDevice', 'device', 2, 2); com('ReleaseD3D', 'd3d', 1, 2);
  external('DestroyWindow', 0x20009, [ref('window')], 1);
  external('ExitProcess', 0x10003, [0], 'exit');
  code.push(0x0f, 0x0b);
  assert.ok(code.length < 0x1f00, `main fits before callback: ${code.length}`);
  assert.ok(variable < 0x5200); assert.ok(constant < 0x8000);
  bytes.set(code, file(TEXT));
  return {bytes, code: Buffer.from(code), calls, addresses, callback, returnPc, returnId, base: BASE, entry: BASE + TEXT};
}
