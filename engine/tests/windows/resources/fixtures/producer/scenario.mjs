import {builder} from '../../../../support/pe32/image.mjs';
import {datafiles, datafileSymbols} from './datafiles.mjs';

export function image() {
  const b = builder([
    ['kernel32.dll', ['FindResourceA', 'FindResourceW', 'FindResourceExW',
      'LoadResource', 'LockResource', 'SizeofResource', 'FreeResource',
      'EnumResourceTypesW', 'EnumResourceTypesExW', 'EnumResourceLanguagesW',
      'GetLastError', 'ExitProcess', ...datafileSymbols]],
    ['user32.dll', ['LoadStringA', 'LoadStringW']],
  ], {callbacks: [
    {words: 3, result: 1, record: 'typeCallback'},
    {words: 5, result: 1, record: 'languageCallback'},
  ]});
  const {allocate, data, external, ref} = b;
  allocate('resource'); allocate('loaded'); allocate('named');
  allocate('stringPointer'); allocate('stringBuffer', Array(8).fill(0xcccccccc));
  allocate('copiedWord');
  const type = data('typeName', [0x00590054, 0x00450050, 0]);
  const name = data('resourceName', [0x0041004e, 0x0045004d, 0]);
  const decimal = b.string('decimalName', '#7');
  const a = b.addresses;
  external('FindResourceW', 0x130002, [0, 7, 10], 'pointer', 'resource');
  external('SizeofResource', 0x130007, [0, ref('resource')], 4);
  external('LoadResource', 0x130005, [0, ref('resource')], 'pointer', 'loaded');
  external('LockResource', 0x130006, [ref('loaded')], 'pointer', undefined, 'payload');
  b.readLocked(a.loaded, a.copiedWord);
  external('FreeResource', 0x130008, [ref('loaded')], 0, undefined, 'copied');
  external('LockResource', 0x130006, [ref('loaded')], 'pointer', undefined, 'payload');
  external('FindResourceA', 0x130001, [0, decimal, 10], 'pointer', undefined, 'same-resource');
  external('FindResourceW', 0x130002, [0, name, type], 'pointer', 'named');
  external('SizeofResource', 0x130007, [0, ref('named')], 4);
  external('FindResourceExW', 0x130004, [0, 10, 7, 0x407], 0);
  external('GetLastError', 0x10001, [], 1815);
  external('LoadStringW', 0x130016, [0, 0, a.stringBuffer, 4], 3, undefined, 'wide-string');
  external('LoadStringW', 0x130016, [0, 0, a.stringPointer, 0], 5, undefined, 'string-pointer');
  external('LoadStringA', 0x130015, [0, 0, a.stringBuffer, 32], 5, undefined, 'ansi-string');
  external('EnumResourceTypesW', 0x13000a, [0, b.callbacks[0].entry, 0x12345678], 1, undefined, 'types');
  external('EnumResourceLanguagesW', 0x13000e, [0, 10, 7, b.callbacks[1].entry, 0x87654321], 1, undefined, 'languages');
  external('EnumResourceTypesExW', 0x130010, [0, b.callbacks[0].entry, 0, 2, 0], 0);
  external('GetLastError', 0x10001, [], 1813);
  external('FindResourceW', 0x130002, [0, 999, 10], 0);
  external('GetLastError', 0x10001, [], 1814);
  datafiles(b);
  external('ExitProcess', 0x10003, [0], 'exit');

  const resource = Buffer.alloc(512);
  const half = (at, value) => resource.writeUInt16LE(value, at);
  const word = (at, value) => resource.writeUInt32LE(value, at);
  half(12, 1); half(14, 2);
  for (const [at, name, target] of [
    [16, 0x800001c0, 0x80000050], [24, 6, 0x80000080], [32, 10, 0x80000050],
  ]) { word(at, name); word(at + 4, target); }
  half(0x5c, 1); half(0x5e, 1);
  word(0x60, 0x800001d0); word(0x64, 0x800000a0);
  word(0x68, 7); word(0x6c, 0x800000a0);
  half(0x8e, 1); word(0x90, 1); word(0x94, 0x800000c0);
  for (const [at, target] of [[0xa0, 0xe0], [0xc0, 0xf0]]) {
    half(at + 14, 1); word(at + 16, 0x409); word(at + 20, target);
  }
  half(0x100, 5); Buffer.from('hello', 'utf16le').copy(resource, 0x102);
  resource.set([0x11, 0x22, 0x33, 0x44], 0x180);
  for (const [at, text] of [[0x1c0, 'TYPE'], [0x1d0, 'NAME']]) {
    half(at, text.length); Buffer.from(text, 'utf16le').copy(resource, at + 2);
  }
  const directory = data('resourceDirectory', Array(128).fill(0));
  const rva = directory - 0x400000;
  word(0xe0, rva + 0x180); word(0xe4, 4);
  word(0xf0, rva + 0x100); word(0xf4, 42); word(0xf8, 1200);
  const guest = b.seal();
  resource.copy(guest.bytes, 0x4200 + rva - 0x6000);
  guest.bytes.writeUInt32LE(rva, 0x98 + 112);
  guest.bytes.writeUInt32LE(resource.length, 0x98 + 116);
  return guest;
}
