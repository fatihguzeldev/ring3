export function callbackRecord(bytes, returnPc, returnId) {
  if (!(bytes instanceof Uint8Array) || bytes.byteLength !== 64) throw new Error('invalid callback record length');
  const d = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (new TextDecoder().decode(bytes.subarray(0, 4)) !== 'R3CB' || d.getUint16(4, true) !== 2 ||
      d.getUint16(6, true) !== 1 || d.getUint32(8, true) !== 64 || d.getUint32(12, true) !== 0 ||
      d.getUint32(60, true) !== 0) throw new Error('invalid callback record header');
  const token = d.getUint32(16, true), outer = d.getUint32(20, true), words = d.getUint32(48, true);
  if (!outer || token <= outer || d.getUint32(24, true) !== 1 || d.getUint32(28, true) > 1 ||
      d.getUint32(32, true) === returnPc || d.getUint32(40, true) !== returnPc || d.getUint32(44, true) !== returnId ||
      d.getUint32(56, true) !== 0 || words > 16 || d.getUint32(52, true) !== 0)
    throw new Error('invalid callback frame');
  return {token, returnPc, returnId, returnEsp: (d.getUint32(36, true) + 4 + words * 4) >>> 0};
}
