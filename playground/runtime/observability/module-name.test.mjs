import assert from "node:assert/strict";
import test from "node:test";
import { appendResidentModuleName } from "./module-name.js";

const header = [0, 97, 115, 109, 1, 0, 0, 0];
const answer = new Uint8Array([
  ...header,
  1, 5, 1, 96, 0, 1, 127,
  3, 2, 1, 0,
  7, 10, 1, 6, 97, 110, 115, 119, 101, 114, 0, 0,
  10, 6, 1, 4, 0, 65, 42, 11,
]);
const options = { unitId: 18446744073709551615n, pc: 0xffffffff, tier: 1 };

function moduleNamePayload(name) {
  const utf8 = new TextEncoder().encode(name);
  return new Uint8Array([0, utf8.length + 1, utf8.length, ...utf8]);
}

test("module identity is standard metadata; original module bytes and execution survive", () => {
  const before = answer.slice();
  const result = appendResidentModuleName(answer, options);
  assert.equal(result.moduleName, "ring3.u18446744073709551615.pcffffffff.t1");
  assert.equal(result.appended, true);
  assert.equal(result.originalBytes, answer.length);
  assert.equal(result.compiledBytes, result.bytes.length);
  assert.equal(result.addedBytes, result.bytes.length - answer.length);
  assert.deepEqual(answer, before);
  assert.deepEqual(result.bytes.slice(0, answer.length), answer);
  const module = new WebAssembly.Module(result.bytes);
  assert.equal(new WebAssembly.Instance(module).exports.answer(), 42);
  assert.deepEqual(new Uint8Array(WebAssembly.Module.customSections(module, "name")[0]), moduleNamePayload(result.moduleName));
  const duplicate = appendResidentModuleName(result.bytes, { unitId: 1n, pc: 0, tier: 0 });
  assert.equal(duplicate.bytes, result.bytes);
  assert.equal(duplicate.moduleName, null);
  assert.equal(duplicate.appended, false);
  assert.equal(duplicate.addedBytes, 0);
});

test("long existing custom sections use bounded u32 decoding and preserve payload bytes", () => {
  const custom = new Uint8Array([0, 0x84, 1, 1, 120, ...new Uint8Array(130).fill(0xa5)]);
  const bytes = new Uint8Array([...header, ...custom]);
  const result = appendResidentModuleName(bytes, { unitId: 1n, pc: 0, tier: 0 });
  assert.equal(result.moduleName, "ring3.u1.pc00000000.t0");
  assert.deepEqual(result.bytes.slice(0, bytes.length), bytes);
  assert.equal(WebAssembly.Module.customSections(new WebAssembly.Module(result.bytes), "x")[0].byteLength, 130);
});

test("existing name sections are preserved even when no module-name subsection exists", () => {
  const existing = new Uint8Array([...header, 0, 5, 4, 110, 97, 109, 101]);
  const result = appendResidentModuleName(existing, options);
  assert.equal(result.bytes, existing);
  assert.equal(result.appended, false);
  assert.equal(result.moduleName, null);
});

test("malformed boundaries fail without modifying input", () => {
  const cases = [
    [], [0, 97, 115, 109, 2, 0, 0, 0],
    [...header, 0], [...header, 0, 0x80],
    [...header, 0, 0xff, 0xff, 0xff, 0xff, 0x10],
    [...header, 0, 0x80, 0x80, 0x80, 0x80, 0x80],
    [...header, 0, 5, 4, 110],
    [...header, 0, 2, 2, 110],
    [...header, 0, 0],
  ];
  for (const raw of cases) {
    const bytes = new Uint8Array(raw), before = bytes.slice();
    assert.throws(() => appendResidentModuleName(bytes, options));
    assert.deepEqual(bytes, before);
  }
});

test("identity inputs reject truncation, unsafe ids, and unsupported tiers", () => {
  for (const unitId of [0n, -1n, 1n << 64n, 1, "1"])
    assert.throws(() => appendResidentModuleName(answer, { ...options, unitId }));
  for (const pc of [-1, 0x100000000, 1.5, NaN, "0"])
    assert.throws(() => appendResidentModuleName(answer, { ...options, pc }));
  for (const tier of [-1, 2, true, "0"])
    assert.throws(() => appendResidentModuleName(answer, { ...options, tier }));
});
