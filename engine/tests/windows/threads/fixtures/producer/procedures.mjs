import assert from 'node:assert/strict';

const word = value => {
  const bytes = Buffer.alloc(4);
  bytes.writeUInt32LE(value >>> 0);
  return [...bytes];
};

function routine(b, entry, thread) {
  const code = [], calls = [], imports = [];
  const save = address => code.push(0xa3, ...word(address));
  const stack = (index, address) => {
    code.push(0x8b, 0x44, 0x24, 4 + index * 4);
    save(address);
  };
  const fs = (offset, address) => {
    code.push(0x64, 0xa1, ...word(offset));
    save(address);
  };
  const external = (name, id, args, expected, output, hook) => {
    for (const value of [...args].reverse()) {
      code.push(...(typeof value === 'object'
        ? [0xff, 0x35, ...word(value.address)] : [0x68, ...word(value)]));
    }
    code.push(0xff, 0x15);
    imports.push({offset: code.length, name});
    code.push(0, 0, 0, 0);
    const status = b.allocate(`${thread}-${calls.length}-status`, [0xcccccccc]);
    calls.push({name, id, end: code.length, status, expected, words: args.length, hook});
    save(status);
    if (output) save(output);
  };
  return {
    entry, thread, code, calls, imports, stack, fs, external,
    write(address, value) {code.push(0xc7, 0x05, ...word(address), ...word(value));},
    increment(address) {code.push(0xff, 0x05, ...word(address));},
    finish(result, words, id, hook) {
      code.push(0xb8, ...word(result), 0xc2, words * 4, 0);
      calls.push({name: hook, id, end: code.length, words: 0, hook, private: true});
    },
  };
}

function importsOf(bytes) {
  const imports = new Map(), file = rva => 0x4200 + rva - 0x6000;
  for (let descriptor = file(bytes.readUInt32LE(0x98 + 104));
    bytes.readUInt32LE(descriptor + 12); descriptor += 20) {
    const original = bytes.readUInt32LE(descriptor), iat = bytes.readUInt32LE(descriptor + 16);
    for (let index = 0; bytes.readUInt32LE(file(original + index * 4)); index++) {
      const at = file(bytes.readUInt32LE(file(original + index * 4))) + 2;
      const end = bytes.indexOf(0, at);
      imports.set(bytes.subarray(at, end).toString(), 0x400000 + iat + index * 4);
    }
  }
  return imports;
}

export function procedures(b) {
  const a = b.addresses, child = routine(b, 0x403800, 2);
  child.stack(0, a.childParameter);
  child.fs(0x18, a.childTeb);
  child.fs(0x24, a.childTidFromFs);
  child.fs(0x30, a.childPeb);
  child.external('TlsGetValue', 0xf000b, [b.ref('tlsIndex')], 0, a.childTlsInitial);
  child.external('TlsSetValue', 0xf000c, [b.ref('tlsIndex'), 0x2222], 1);
  child.external('EnterCriticalSection', 0x100014, [a.critical], 0, undefined, 'child-blocked');
  child.write(a.childEntered, 1);
  child.external('TlsGetValue', 0xf000b, [b.ref('tlsIndex')], 0x2222, a.childTlsFinal);
  child.external('LeaveCriticalSection', 0x100015, [a.critical], 0);
  child.finish(37, 1, 0xfffff, 'child-return');

  const apc = routine(b, 0x403c00, 1);
  apc.stack(0, a.apcArgument);
  apc.fs(0x24, a.apcTid);
  apc.increment(a.apcCount);
  apc.finish(0, 1, 0xffffe, 'apc-return');

  const timer = routine(b, 0x403d00, 1);
  for (let index = 0; index < 3; index++) timer.stack(index, a.timerArguments + index * 4);
  timer.fs(0x24, a.timerTid);
  timer.increment(a.timerCount);
  timer.finish(0, 3, 0xffffe, 'timer-return');
  const routines = [child, apc, timer];
  assert.ok(child.code.length < 0x400 && apc.code.length < 0x100 && timer.code.length < 0x100);
  return {
    child, apc, timer,
    install(guest) {
      const iats = importsOf(guest.bytes), segments = [];
      for (const item of routines) {
        for (const patch of item.imports) {
          assert.ok(iats.has(patch.name), patch.name);
          item.code.splice(patch.offset, 4, ...word(iats.get(patch.name)));
        }
        guest.bytes.set(item.code, 0x200 + item.entry - 0x401000);
        let start = item.entry;
        for (const call of item.calls) {
          segments.push({...call, start, limit: item.entry + call.end, thread: item.thread});
          start = item.entry + call.end;
        }
      }
      return segments;
    },
  };
}
