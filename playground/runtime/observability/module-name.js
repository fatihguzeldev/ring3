const header = [0, 97, 115, 109, 1, 0, 0, 0];
const encoder = new TextEncoder();

function uleb(value) {
  if (!Number.isInteger(value) || value < 0 || value > 0xffffffff)
    throw new RangeError("invalid wasm u32");
  const bytes = [];
  do {
    const low = value % 128;
    value = Math.floor(value / 128);
    bytes.push(low | (value === 0 ? 0 : 128));
  } while (value !== 0);
  return bytes;
}

function readU32(bytes, cursor, end) {
  let value = 0;
  for (let index = 0; index < 5; index++) {
    if (cursor === end) throw new Error("truncated wasm section length");
    const byte = bytes[cursor++];
    if (index === 4 && (byte & 0xf0) !== 0)
      throw new Error("overflowed wasm section length");
    value += (byte & 127) * 2 ** (index * 7);
    if ((byte & 128) === 0) return { value, cursor };
  }
  throw new Error("unterminated wasm section length");
}

function hasNameSection(bytes) {
  if (bytes.length < header.length || header.some((byte, index) => bytes[index] !== byte))
    throw new Error("invalid wasm module header");
  let cursor = header.length;
  let found = false;
  while (cursor < bytes.length) {
    const id = bytes[cursor++];
    const size = readU32(bytes, cursor, bytes.length);
    cursor = size.cursor;
    const end = cursor + size.value;
    if (end > bytes.length) throw new Error("truncated wasm section");
    if (id === 0) {
      const name = readU32(bytes, cursor, end);
      if (name.value > end - name.cursor) throw new Error("truncated wasm custom name");
      if (name.value === 4 && bytes[name.cursor] === 110 && bytes[name.cursor + 1] === 97 &&
          bytes[name.cursor + 2] === 109 && bytes[name.cursor + 3] === 101)
        found = true;
    }
    cursor = end;
  }
  return found;
}

export function appendResidentModuleName(bytes, { unitId, pc, tier }) {
  if (!(bytes instanceof Uint8Array)) throw new TypeError("expected wasm bytes");
  if (typeof unitId !== "bigint" || unitId < 1n || unitId > 0xffffffffffffffffn)
    throw new RangeError("invalid resident unit id");
  if (!Number.isInteger(pc) || pc < 0 || pc > 0xffffffff)
    throw new RangeError("invalid resident entry pc");
  if (tier !== 0 && tier !== 1) throw new RangeError("invalid resident tier");
  const originalBytes = bytes.length;
  if (hasNameSection(bytes))
    return { bytes, moduleName: null, appended: false, originalBytes, compiledBytes: originalBytes, addedBytes: 0 };
  const moduleName = `ring3.u${unitId}.pc${pc.toString(16).padStart(8, "0")}.t${tier}`;
  const name = encoder.encode(moduleName);
  const subsection = [...uleb(name.length), ...name];
  const payload = [4, 110, 97, 109, 101, 0, ...uleb(subsection.length), ...subsection];
  const suffix = new Uint8Array([0, ...uleb(payload.length), ...payload]);
  const named = new Uint8Array(originalBytes + suffix.length);
  named.set(bytes);
  named.set(suffix, originalBytes);
  return { bytes: named, moduleName, appended: true, originalBytes, compiledBytes: named.length, addedBytes: suffix.length };
}
