import assert from 'node:assert/strict';

function unsigned(value, label) {
  assert.ok(Number.isInteger(value) && value >= 0 && value <= 0xffffffff, label);
  return value;
}

function geometry({errorCode, address, access, length}) {
  unsigned(address, 'declared fault address');
  unsigned(length, 'declared access length');
  assert.ok(length > 0 && length <= (access === 3 ? 15 : 832), 'canonical faulting access length');
  const allowed = access === 1 ? [4, 5] : access === 2 ? [6, 7] : access === 3 ? [20, 21] : [];
  assert.ok(allowed.includes(errorCode), 'declared user fault code agrees with access intent');
  return [errorCode, address, access, length];
}

function record(magic, version, fields) {
  const bytes = Buffer.alloc(40);
  bytes.write(magic); bytes.writeUInt16LE(version, 4); bytes.writeUInt16LE(1, 6);
  bytes.writeUInt32LE(40, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value, 16 + 4 * index));
  return bytes;
}

export function pageFaultExit({retired, ...fault}) {
  unsigned(retired, 'declared retired instructions');
  const fields = geometry(fault);
  return record('R3EX', fault.access === 3 ? 13 : 12, [23, retired, ...fields]);
}

export function pageFaultPacket(fault) {
  return record('R3MH', 15, [9, 0, ...geometry(fault)]);
}
