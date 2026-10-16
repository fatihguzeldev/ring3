import {builder} from '../../../../support/pe32/image.mjs';

function wide(builder, name, value) {
  const bytes = Buffer.from(value + '\0', 'utf16le');
  const padded = Buffer.alloc(Math.ceil(bytes.length / 4) * 4);
  bytes.copy(padded);
  return builder.data(name, Array.from({length: padded.length / 4}, (_, index) => padded.readUInt32LE(index * 4)));
}

export function image() {
  const symbols = [
    'RegCreateKeyExA', 'RegOpenKeyExA', 'RegSetValueExA', 'RegSetValueExW',
    'RegQueryValueExA', 'RegQueryValueExW', 'RegGetValueW', 'RegEnumValueW',
    'RegQueryInfoKeyW', 'RegCloseKey', 'RegOverridePredefKey', 'RegCopyTreeA',
    'RegDeleteValueA', 'RegSetKeyValueA', 'RegEnumKeyExA', 'RegDeleteTreeA',
    'RegNotifyChangeKeyValue', 'RegFlushKey', 'RegDeleteKeyA',
  ];
  const b = builder([
    ['advapi32.dll', symbols],
    ['kernel32.dll', ['CreateEventA', 'WaitForSingleObject', 'ResetEvent', 'CloseHandle', 'ExitProcess']],
  ]);
  const gate = {
    RegCreateKeyExA: 0x110004, RegOpenKeyExA: 0x110002,
    RegSetValueExA: 0x110008, RegSetValueExW: 0x110009,
    RegQueryValueExA: 0x110006, RegQueryValueExW: 0x110007,
    RegGetValueW: 0x110015, RegEnumValueW: 0x110011,
    RegQueryInfoKeyW: 0x110013, RegCloseKey: 0x110001,
    RegOverridePredefKey: 0x11002b, RegCopyTreeA: 0x110027,
    RegDeleteValueA: 0x11000c, RegSetKeyValueA: 0x110031,
    RegEnumKeyExA: 0x11000e, RegDeleteTreeA: 0x110018,
    RegNotifyChangeKeyValue: 0x11002e, RegFlushKey: 0x11001a,
    RegDeleteKeyA: 0x11000a, CreateEventA: 0x10011,
    WaitForSingleObject: 0x1001a, ResetEvent: 0x10016,
    CloseHandle: 0x10018, ExitProcess: 0x10003,
  };
  const call = (name, args, expected = 0, hook, output) =>
    b.external(name, gate[name], args, expected, output, hook);
  for (const name of ['key', 'readonly', 'copy', 'replacement', 'event', 'disposition', 'size', 'type', 'nameSize', 'values']) b.allocate(name);
  b.allocate('buffer', Array(32).fill(0xcccccccc));
  b.allocate('enumName', Array(16).fill(0xcccccccc));
  const a = b.addresses;
  const path = b.string('path', 'Software\\Ring3\\Game');
  const copyPath = b.string('copyPath', 'Software\\Ring3\\Copy');
  const child = b.string('child', 'Child');
  const childW = wide(b, 'childW', 'Child');
  const name = b.string('name', 'Name');
  const nameW = wide(b, 'nameW', 'Name');
  const termW = wide(b, 'termW', 'Term');
  const text = b.string('text', 'ok');
  const binary = b.data('binary', [0x78563412]);
  const unterminated = b.data('unterminated', [0x00000041]);
  const final = b.data('final', [9]);
  const seedPathW = wide(b, 'seedPathW', 'Software\\Ring3\\Seed');
  const bootW = wide(b, 'bootW', 'Boot');

  b.write(a.size, 32);
  call('RegGetValueW', [0x80000001, seedPathW, bootW, 16, a.type, a.buffer, a.size], 0, 'seeded');
  call('RegCreateKeyExA', [0x80000001, path, 0, 0, 0, 0xf003f, 0, a.key, a.disposition], 0, 'created');
  call('RegSetValueExA', [b.ref('key'), name, 0, 1, text, 3]);
  b.write(a.size, 32);
  call('RegQueryValueExW', [b.ref('key'), nameW, 0, a.type, a.buffer, a.size], 0, 'unicode');
  b.write(a.size, 2);
  b.write(a.buffer, 0xcccccccc);
  call('RegQueryValueExA', [b.ref('key'), name, 0, a.type, a.buffer, a.size], 234, 'more-data');
  call('RegSetValueExW', [b.ref('key'), termW, 0, 1, unterminated, 2]);
  b.write(a.size, 32);
  call('RegGetValueW', [b.ref('key'), 0, termW, 2, a.type, a.buffer, a.size], 0, 'terminated');
  b.write(a.size, 32);
  b.write(a.nameSize, 32);
  call('RegEnumValueW', [b.ref('key'), 0, a.enumName, a.nameSize, 0, a.type, a.buffer, a.size], 0, 'enumerated-value');
  call('RegQueryInfoKeyW', [b.ref('key'), 0, 0, 0, 0, 0, 0, a.values, 0, 0, 0, 0], 0, 'info');
  call('RegOpenKeyExA', [b.ref('key'), 0, 0, 1, a.readonly]);
  call('RegSetValueExA', [b.ref('readonly'), name, 0, 3, binary, 4], 5);
  call('RegCloseKey', [b.ref('readonly')]);
  call('RegOverridePredefKey', [0x80000000, b.ref('key')]);
  b.write(a.size, 32);
  call('RegQueryValueExA', [0x80000000, name, 0, a.type, a.buffer, a.size], 0, 'overridden');
  call('RegOverridePredefKey', [0x80000000, 0]);
  call('RegCreateKeyExA', [0x80000001, copyPath, 0, 0, 0, 0xf003f, 0, a.copy, 0]);
  call('RegCopyTreeA', [b.ref('key'), 0, b.ref('copy')]);
  b.write(a.size, 32);
  call('RegQueryValueExA', [b.ref('copy'), name, 0, a.type, a.buffer, a.size], 0, 'copied');
  call('RegDeleteValueA', [b.ref('key'), name]);
  call('RegQueryValueExA', [b.ref('key'), name, 0, a.type, a.buffer, a.size], 2);
  call('RegSetKeyValueA', [b.ref('key'), child, 0, 4, binary, 4]);
  b.write(a.size, 32);
  call('RegGetValueW', [b.ref('key'), childW, 0, 16, a.type, a.buffer, a.size], 0, 'child-value');
  b.write(a.nameSize, 32);
  call('RegEnumKeyExA', [b.ref('key'), 0, a.enumName, a.nameSize, 0, 0, 0, 0], 0, 'enumerated-key');
  call('RegDeleteTreeA', [b.ref('key'), child]);
  call('RegOpenKeyExA', [b.ref('key'), child, 0, 1, a.readonly], 2);
  call('CreateEventA', [0, 1, 0, 0], 'handle', undefined, 'event');
  call('RegNotifyChangeKeyValue', [b.ref('key'), 0, 4, b.ref('event'), 1]);
  call('WaitForSingleObject', [b.ref('event'), 0], 258);
  call('RegSetValueExA', [b.ref('key'), name, 0, 3, binary, 4]);
  call('WaitForSingleObject', [b.ref('event'), 0], 0, 'notified');
  call('ResetEvent', [b.ref('event')], 1);
  call('RegSetValueExA', [b.ref('key'), name, 0, 3, binary, 4]);
  call('WaitForSingleObject', [b.ref('event'), 0], 258, 'one-shot');
  call('RegFlushKey', [b.ref('key')], 0, 'flush');
  call('RegDeleteKeyA', [0x80000001, path]);
  call('RegQueryValueExA', [b.ref('key'), name, 0, a.type, a.buffer, a.size], 1018);
  call('RegCreateKeyExA', [0x80000001, path, 0, 0, 0, 0xf003f, 0, a.replacement, a.disposition], 0, 'recreated');
  call('RegSetValueExA', [b.ref('replacement'), name, 0, 3, final, 4]);
  call('RegQueryValueExA', [b.ref('key'), name, 0, a.type, a.buffer, a.size], 1018);
  call('RegCloseKey', [b.ref('key')]);
  call('RegCloseKey', [b.ref('copy')]);
  call('RegCloseKey', [b.ref('replacement')]);
  call('CloseHandle', [b.ref('event')], 1);
  call('ExitProcess', [0], 'exit');
  return b.seal();
}
