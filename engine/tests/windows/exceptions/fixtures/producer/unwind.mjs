import assert from 'node:assert/strict';
import {builder} from '../../../../support/pe32/image.mjs';

const word = value => {
  const bytes = Buffer.alloc(4);
  bytes.writeUInt32LE(value >>> 0);
  return [...bytes];
};

function assembler(base) {
  const bytes = [], labels = new Map(), references = [];
  return {
    emit(...values) { bytes.push(...values); },
    label(name) { assert.ok(!labels.has(name)); labels.set(name, base + bytes.length); },
    address(name) { references.push({offset: bytes.length, name}); bytes.push(0, 0, 0, 0); },
    branch(name) { bytes.push(0x0f, 0x85); references.push({offset: bytes.length, name, relative: true}); bytes.push(0, 0, 0, 0); },
    store(address, value) { bytes.push(0xc7, 0x05, ...word(address), ...word(value)); },
    save(address) { bytes.push(0xa3, ...word(address)); },
    finish() {
      for (const reference of references) {
        assert.ok(labels.has(reference.name), reference.name);
        const target = labels.get(reference.name);
        bytes.splice(reference.offset, 4, ...word(reference.relative ? target - base - reference.offset - 4 : target));
      }
      return Buffer.from(bytes);
    },
  };
}

export function unwindImage() {
  const names = ['RaiseException', 'RtlUnwind', 'RtlCaptureContext', 'RtlRestoreContext', 'ExitProcess'];
  const ids = [0x120007, 0x120008, 0x120009, 0x12000a, 0x10003];
  const b = builder([['kernel32.dll', names]]);
  for (const name of ['mode', 'nestedFlags', 'outerFlags', 'nestedCalls', 'outerCalls', 'unwindResult', 'restoreResult', 'autoFlags', 'autoHandled', 'parentAfter', 'parentContext', 'autoUnwindAfter', 'autoUnwindResult']) b.allocate(name);
  b.allocate('context', Array(179).fill(0));
  const a = b.addresses, frames = [0x7f003f00, 0x7f003f10, 0x7f003f20];
  const procedures = [0x402000, 0x402100, 0x402200];
  const iat = name => 0x406400 + names.indexOf(name) * 4;
  const main = assembler(0x401000);
  const call = (name, args) => {
    for (const value of [...args].reverse()) main.emit(0x68, ...word(value));
    main.emit(0xff, 0x15, ...word(iat(name)));
  };
  frames.forEach((frame, index) => {
    main.store(frame, frames[index + 1] ?? 0xffffffff);
    main.store(frame + 4, procedures[index]);
  });
  main.emit(0xbc, ...word(frames[0] - 16));
  main.emit(0x64, 0xc7, 0x05, ...word(0), ...word(frames[0]));
  main.store(a.mode, 1);
  call('RaiseException', [0xe0530001, 0, 0, 0]);
  main.store(a.mode, 3);
  call('RaiseException', [0xe0530301, 0, 0, 0]);
  main.store(a.mode, 2);
  call('RtlUnwind', [frames[2], 0, 0, 0x76543210]);
  main.save(a.unwindResult);
  main.emit(0x64, 0xc7, 0x05, ...word(0), ...word(frames[0]));
  main.store(a.mode, 4);
  call('RtlUnwind', [frames[2], 0, 0, 0x11235813]);
  main.save(a.autoUnwindResult);
  call('RtlCaptureContext', [a.context]);
  main.store(a.context + 176, 0x24681357);
  main.emit(0xc7, 0x05, ...word(a.context + 184));
  main.address('restored');
  // i386 ignores the optional record; this inaccessible pointer must not be read as an x64 jump buffer.
  call('RtlRestoreContext', [a.context, 0xffffffff]);
  main.emit(0xcc);
  main.label('restored');
  main.save(a.restoreResult);
  main.emit(0x64, 0xc7, 0x05, ...word(0), ...word(0xffffffff));
  call('ExitProcess', [0]);
  main.emit(0xcc);
  b.emit(...main.finish());
  const guest = b.seal();
  const handlers = [];
  const first = assembler(procedures[0]);
  const handlerCall = (name, args) => {
    for (const value of [...args].reverse()) first.emit(0x68, ...word(value));
    first.emit(0xff, 0x15, ...word(iat(name)));
  };
  first.emit(0x83, 0x3d, ...word(a.mode), 3);
  first.branch('not-nested');
  first.emit(0x8b, 0x44, 0x24, 4, 0x81, 0x38, ...word(0xe0530302));
  first.branch('raise-child');
  first.emit(0x8b, 0x40, 4);
  first.save(a.autoFlags);
  first.emit(0xff, 0x05, ...word(a.autoHandled), 0x31, 0xc0, 0xc3);
  first.label('raise-child');
  handlerCall('RaiseException', [0xe0530302, 0, 0, 0]);
  first.emit(0xff, 0x05, ...word(a.parentAfter), 0xb8, ...word(1), 0xc3);
  first.label('not-nested');
  first.emit(0x83, 0x3d, ...word(a.mode), 4);
  first.branch('direct-disposition');
  first.emit(0x8b, 0x44, 0x24, 12);
  first.save(a.parentContext);
  handlerCall('RtlUnwind', [frames[2], 0, 0, 0xabcdef01]);
  first.emit(0xff, 0x05, ...word(a.autoUnwindAfter));
  first.emit(0x68, ...word(0), 0xff, 0x35, ...word(a.parentContext));
  first.emit(0xff, 0x15, ...word(iat('RtlRestoreContext')), 0xcc);
  first.label('direct-disposition');
  first.emit(0x8b, 0x44, 0x24, 16, 0xc7, 0x00, ...word(frames[1]));
  first.emit(0x83, 0x3d, ...word(a.mode), 1);
  first.branch('collision');
  first.emit(0xb8, ...word(2), 0xc3);
  first.label('collision');
  first.emit(0xb8, ...word(3), 0xc3);
  handlers.push(first.finish());
  for (const [index, flags, count, result] of [[1, a.nestedFlags, a.nestedCalls, 1], [2, a.outerFlags, a.outerCalls, 0]]) {
    const handler = assembler(procedures[index]);
    handler.emit(0xff, 0x05, ...word(count));
    handler.emit(0x8b, 0x44, 0x24, 4, 0x8b, 0x40, 4);
    handler.save(flags);
    handler.emit(0xb8, ...word(result), 0xc3);
    handlers.push(handler.finish());
  }
  handlers.forEach((body, index) => {
    assert.ok(body.length < 256);
    guest.bytes.set(body, 0x200 + procedures[index] - guest.base - 0x1000);
  });
  const expectedCalls = [ids[0], ids[0], ids[0], ids[1], ids[1], ids[1], ids[3], ids[2], ids[3], ids[4]];
  return {...guest, procedures, frames, ids, names, expectedCalls};
}
