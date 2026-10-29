import assert from 'node:assert/strict';
import test from 'node:test';
import {Reader, PacketStream, pipeline} from '../graphics/packet.js';
import {stateBytes} from '../graphics/shaders.js';

test('packet scalar reads honor byte offsets, unaligned fields and little endian values', () => {
  const buffer = new ArrayBuffer(40), view = new DataView(buffer);
  view.setUint32(3, 0xfedcba98, true);
  view.setBigUint64(7, 0xfedcba9876543210n, true);
  view.setFloat32(15, -0, true);
  view.setUint32(19, 0x7fc12345, true);
  const r = new Reader(new Uint8Array(buffer, 3, 20));
  assert.equal(r.u32(), 0xfedcba98);
  assert.equal(r.u64(), 0xfedcba9876543210n);
  assert.ok(Object.is(r.f32(), -0));
  assert.ok(Number.isNaN(r.f32()));
  r.end();
});

test('bulk reads preserve unsigned words and IEEE float values across an unaligned view', () => {
  const buffer = new ArrayBuffer(43), view = new DataView(buffer);
  const words = [0, 0xffffffff, 0x80000000, 0x12345678];
  words.forEach((value, i) => view.setUint32(1 + i * 4, value, true));
  const floats = [0, -0, 1.5, Infinity, -Infinity, NaN];
  floats.forEach((value, i) => view.setFloat32(17 + i * 4, value, true));
  const r = new Reader(new Uint8Array(buffer, 1, 40));
  assert.deepEqual(r.words(words.length), words);
  assert.deepEqual(r.floats(floats.length), floats);
  assert.deepEqual(r.words(0), []);
  assert.deepEqual(r.floats(0), []);
  r.end();
});

test('truncated scalar fields reject before consuming bytes', () => {
  for (const [method, width] of [['u32', 4], ['u64', 8], ['f32', 4]]) {
    const r = new Reader(new Uint8Array(width - 1));
    assert.throws(() => r[method](), /D3D8 host: truncated payload/);
    assert.equal(r.at, 0);
  }
});

test('bulk fields reject truncated spans, excessive counts and trailing bytes', () => {
  for (const method of ['words', 'floats']) {
    assert.throws(() => new Reader(new Uint8Array(7))[method](2), /truncated payload/);
    assert.throws(() => new Reader(new Uint8Array())[method](65537), /array limit/);
  }
  const r = new Reader(new Uint8Array(5));
  r.u32();
  assert.throws(() => r.end(), /trailing payload bytes 1/);
});

function pipelineBytes(transforms = new Map([[0, Array(16).fill(1)]])) {
  const fields = [], word = value => fields.push(value >>> 0);
  const scalar = new DataView(new ArrayBuffer(4));
  const float = value => { scalar.setFloat32(0, value, true); word(scalar.getUint32(0, true)); };
  word(2); word(174);
  for (let i = 0; i < 174; i++) word(i);
  word(8);
  for (let stage = 0; stage < 8; stage++) {
    word(29);
    for (let field = 0; field < 29; field++) word(stage * 100 + field);
  }
  word(266); word(9);
  for (let i = 0; i < 9; i++) {
    let mask = 0;
    for (let bit = 0; bit < 32; bit++) if (transforms.has(i * 32 + bit)) mask |= 1 << bit;
    word(mask);
  }
  for (let i = 0; i < 266; i++) if (transforms.has(i)) transforms.get(i).forEach(float);
  for (let i = 0; i < 17; i++) word(0x40000000);
  word(0); word(0); word(3); word(4);
  for (const value of [1, 2, 640, 480, 0, 0x3f800000, 0x112, 0, 9]) word(value);
  word(96);
  for (let i = 0; i < 96 * 4; i++) word(0x3f000000);
  word(8);
  for (let i = 0; i < 8 * 4; i++) word(0xbf800000);
  const bytes = new Uint8Array(fields.length * 4), view = new DataView(bytes.buffer);
  fields.forEach((value, i) => view.setUint32(i * 4, value, true));
  return bytes;
}

test('complete pipeline snapshots retain state arrays, sparse transforms and constants', () => {
  const r = new Reader(pipelineBytes()), p = pipeline(r);
  r.end();
  assert.equal(p.render.length, 174);
  assert.equal(p.render[173], 173);
  assert.deepEqual(p.stages[7], Array.from({length: 29}, (_, i) => 700 + i));
  assert.deepEqual(p.transforms[0], Array(16).fill(1));
  assert.deepEqual(p.transforms[265], [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
  assert.deepEqual(p.material, Array(17).fill(2));
  assert.deepEqual(p.clipStatus, [3, 4]);
  assert.deepEqual(p.viewport, {x: 1, y: 2, width: 640, height: 480, min: 0, max: 1});
  assert.equal(p.vertexShader, 0x112);
  assert.equal(p.palette, 9);
  assert.deepEqual(p.vc[95], [0.5, 0.5, 0.5, 0.5]);
  assert.deepEqual(p.pc[7], [-1, -1, -1, -1]);
});

test('implicit transform defaults are shared immutable identities across decoded snapshots', () => {
  const first = pipeline(new Reader(pipelineBytes())), second = pipeline(new Reader(pipelineBytes()));
  assert.equal(first.transforms[1], first.transforms[265]);
  assert.equal(first.transforms[1], second.transforms[1]);
  assert.equal(Object.isFrozen(first.transforms[1]), true);
  assert.throws(() => { first.transforms[1][0] = 99; }, TypeError);
  assert.equal(second.transforms[1][0], 1);
  assert.notEqual(first.transforms[0], second.transforms[0]);
});

test('sparse uniform packing reads only explicit matrices and preserves float values and program overrides', () => {
  const matrices = new Map([
    [0, Array.from({length: 16}, (_, i) => i / 3)],
    [31, [-0, Infinity, -Infinity, NaN, ...Array(12).fill(2)]],
    [32, Array(16).fill(-2)],
    [265, Array.from({length: 16}, (_, i) => 100 + i)],
  ]);
  const r = new Reader(pipelineBytes(matrices)), p = pipeline(r);r.end();
  const reads = [];
  p.transforms = new Proxy(p.transforms, {get(target, key, receiver) {
    if (typeof key === 'string' && /^\d+$/.test(key)) reads.push(Number(key));
    return Reflect.get(target, key, receiver);
  }});
  const vertex = {declaration: {constants: [[0, [1, 2, 3, 4]]]}, program: {constants: [[0, [5, 6, 7, 8]]]}},
    pixel = {program: {constants: [[0, [9, 10, 11, 12]]]}},
    packed = new Float32Array(stateBytes(p, vertex, pixel).buffer);
  assert.deepEqual(reads, [0, 31, 32, 265]);
  assert.deepEqual([...packed.subarray(0, 4)], [5, 6, 7, 8]);
  assert.deepEqual([...packed.subarray(96 * 4, 97 * 4)], [9, 10, 11, 12]);
  const identity = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
  for (let i = 0; i < 266; i++)
    assert.deepEqual(packed.subarray((104 + i * 4) * 4, (108 + i * 4) * 4), new Float32Array(matrices.get(i) ?? identity));
  assert.deepEqual([...packed.subarray(1168 * 4, 1168 * 4 + 17)], Array(17).fill(2));
  packed.fill(99);
  assert.equal(new Float32Array(stateBytes(p).buffer)[(104 + 4) * 4], 1, 'uniform results cannot mutate the shared template');
});

test('manual snapshots without a wire mask retain zero-filled missing matrices and dynamic copies', () => {
  const p = {vc: [[1, 2, 3, 4]], pc: [], transforms: [Array(16).fill(3)], material: [7]};
  const first = new Float32Array(stateBytes(p).buffer);
  assert.deepEqual([...first.subarray(104 * 4, 108 * 4)], Array(16).fill(3));
  assert.deepEqual([...first.subarray(108 * 4, 112 * 4)], Array(16).fill(0));
  p.transforms[0][0] = -0;p.vc[0][0] = 8;
  const second = new Float32Array(stateBytes(p).buffer);
  assert.ok(Object.is(second[104 * 4], -0));assert.equal(second[0], 8);
  assert.equal(second[1168 * 4], 7);assert.equal(first[104 * 4], 3);
});

function fragment(sequence, flags, offset, bytes, total = 7) {
  const packet = new Uint8Array(96 + bytes.length), view = new DataView(packet.buffer);
  packet.set([82, 51, 68, 56]);
  view.setUint16(4, 2, true); view.setUint16(6, 8, true);
  view.setUint32(8, packet.length, true); view.setUint32(12, 1, true);
  for (const at of [16, 32, 40, 72]) view.setBigUint64(at, 1n, true);
  view.setBigUint64(24, BigInt(sequence), true);
  view.setUint16(64, 15, true); view.setUint16(66, flags, true);
  view.setUint32(68, 32 + bytes.length, true);
  view.setUint32(80, offset, true); view.setUint32(84, total, true);
  packet.set(bytes, 96);
  return packet;
}

test('fragmented commands retain owned bytes and reject stale packet ordering', () => {
  const stream = new PacketStream(), first = fragment(1, 1, 0, [1, 2, 3]);
  assert.deepEqual(stream.decode(first).commands, []);
  first.fill(0);
  const result = stream.decode(fragment(2, 2, 3, [4, 5, 6, 7]));
  assert.equal(result.commands[0].id, 1n);
  assert.deepEqual([...result.commands[0].bytes], [1, 2, 3, 4, 5, 6, 7]);
  assert.throws(() => stream.decode(fragment(2, 3, 0, [1, 2, 3], 3)), /stale\/out-of-order batch/);
});
