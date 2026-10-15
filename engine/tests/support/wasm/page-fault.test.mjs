import assert from 'node:assert/strict';
import test from 'node:test';
import {pageFaultExit, pageFaultPacket} from './page-fault.mjs';

test('declared read, write and execute page faults have literal complete wire records', () => {
  const read = {errorCode: 4, address: 0x12345678, access: 1, length: 4};
  assert.equal(pageFaultExit({retired: 2, ...read}).toString('hex'),
    '523345580c0001002800000000000000170000000200000004000000785634120100000004000000');
  assert.equal(pageFaultPacket(read).toString('hex'),
    '52334d480f0001002800000000000000090000000000000004000000785634120100000004000000');
  const write = {errorCode: 7, address: 0xffffffff, access: 2, length: 2};
  assert.equal(pageFaultExit({retired: 0, ...write}).toString('hex'),
    '523345580c0001002800000000000000170000000000000007000000ffffffff0200000002000000');
  assert.equal(pageFaultPacket(write).toString('hex'),
    '52334d480f0001002800000000000000090000000000000007000000ffffffff0200000002000000');
  const execute = {errorCode: 21, address: 0, access: 3, length: 15};
  assert.equal(pageFaultExit({retired: 1, ...execute}).toString('hex'),
    '523345580d000100280000000000000017000000010000001500000000000000030000000f000000');
  assert.equal(pageFaultPacket(execute).toString('hex'),
    '52334d480f000100280000000000000009000000000000001500000000000000030000000f000000');
});

test('all declared user fault code pairs are distinct and accepted', () => {
  for (const [access, errorCode] of [[1, 4], [1, 5], [2, 6], [2, 7], [3, 20], [3, 21]]) {
    const fault = {errorCode, address: 0x8000, access, length: access === 3 ? 15 : 832};
    assert.equal(pageFaultPacket(fault).readUInt32LE(24), errorCode);
    assert.equal(pageFaultExit({retired: 0, ...fault}).readUInt32LE(32), access);
  }
});

test('invalid expected geometry cannot silently encode a different fault', () => {
  const fault = {errorCode: 4, address: 0x8000, access: 1, length: 4};
  for (const update of [{errorCode: 6}, {errorCode: 0}, {access: 0}, {length: 0},
    {length: -1}, {length: 833}, {length: 0x100000000}, {address: -1}, {address: 0x100000000},
    {errorCode: 20, access: 3, length: 16}]) {
    assert.throws(() => pageFaultPacket({...fault, ...update}));
    assert.throws(() => pageFaultExit({retired: 0, ...fault, ...update}));
  }
  for (const retired of [-1, 0x100000000, 1.5]) assert.throws(() => pageFaultExit({retired, ...fault}));
});
