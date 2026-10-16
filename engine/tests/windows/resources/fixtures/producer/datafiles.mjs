export const datafileSymbols = [
  'LoadLibraryExA', 'FreeLibrary', 'GetModuleHandleA', 'GetProcAddress', 'VirtualQuery', 'VirtualProtect',
];
export function datafiles(b) {
  const {allocate, external, ref} = b;
  for (const name of ['fileOne', 'fileTwo', 'imageOnly', 'fileResource', 'imageResource', 'dataPointer', 'oldProtection']) allocate(name);
  allocate('virtualInfo', Array(7).fill(0));
  const path = b.string('datafileName', 'data-resource.dll');
  const procedure = b.string('datafileProcedure', 'UnusedEntry');
  const a = b.addresses;
  external('LoadLibraryExA', 0x1000c, [path, 0, 2], 'pointer', 'fileOne', 'datafile-first');
  external('GetProcAddress', 0x1000e, [ref('fileOne'), procedure], 0);
  external('GetLastError', 0x10001, [], 6);
  external('FindResourceW', 0x130002, [ref('fileOne'), 7, 10], 'pointer', 'fileResource');
  external('LoadResource', 0x130005, [ref('fileOne'), ref('fileResource')], 'pointer', 'dataPointer');
  external('LockResource', 0x130006, [ref('dataPointer')], 'pointer', undefined, 'datafile-payload');
  external('VirtualQuery', 0xb001e, [ref('dataPointer'), a.virtualInfo, 28], 28, undefined, 'datafile-query');
  external('VirtualProtect', 0x10007, [ref('dataPointer'), 4, 4, a.oldProtection], 0);
  external('GetLastError', 0x10001, [], 87);
  external('LoadLibraryExA', 0x1000c, [path, 0, 2], 'pointer', 'fileTwo', 'datafile-second');
  external('FreeLibrary', 0x1000f, [ref('fileOne')], 1);
  external('SizeofResource', 0x130007, [ref('fileTwo'), ref('fileResource')], 0);
  external('GetLastError', 0x10001, [], 6);
  external('LockResource', 0x130006, [ref('dataPointer')], 0);
  external('GetLastError', 0x10001, [], 6);
  external('LoadLibraryExA', 0x1000c, [path, 0, 0x20], 'pointer', 'imageOnly', 'datafile-image');
  external('FindResourceW', 0x130002, [ref('imageOnly'), 7, 10], 'pointer', 'imageResource');
  external('LoadResource', 0x130005, [ref('imageOnly'), ref('imageResource')], 'pointer', 'dataPointer');
  external('LockResource', 0x130006, [ref('dataPointer')], 'pointer', undefined, 'image-payload');
  external('VirtualQuery', 0xb001e, [ref('dataPointer'), a.virtualInfo, 28], 28, undefined, 'image-query');
  external('VirtualProtect', 0x10007, [ref('dataPointer'), 4, 1, a.oldProtection], 1, undefined, 'datafile-noaccess');
  external('VirtualProtect', 0x10007, [ref('dataPointer'), 4, 2, a.oldProtection], 1, undefined, 'datafile-readonly');
  external('FreeLibrary', 0x1000f, [ref('imageOnly')], 1);
  external('FreeLibrary', 0x1000f, [ref('fileTwo')], 1);
  external('LoadLibraryExA', 0x1000c, [path, 0, 0x22], 'pointer', 'imageOnly', 'datafile-image');
  external('FindResourceW', 0x130002, [ref('imageOnly'), 7, 10], 'pointer', 'imageResource');
  external('LoadResource', 0x130005, [ref('imageOnly'), ref('imageResource')], 'pointer', 'dataPointer');
  external('LockResource', 0x130006, [ref('dataPointer')], 'pointer', undefined, 'image-payload');
  external('FreeLibrary', 0x1000f, [ref('imageOnly')], 1);
  external('GetModuleHandleA', 0x10004, [path], 0);
}

export function libraryImage(guest) {
  const bytes = Buffer.from(guest.bytes);
  bytes.writeUInt16LE(0x2102, 0x96);
  bytes.writeUInt32LE(0x500000, 0x98 + 28);
  bytes.fill(0, 0x4200 + 0x100, 0x4200 + 0x10d);
  bytes.write('missing.dll\0', 0x4200 + 0x100);
  const rva = guest.addresses.resourceDirectory - 0x400000 + 0x300;
  const raw = 0x4200 + rva - 0x6000;
  bytes.writeUInt32LE(rva, 0x98 + 168);
  bytes.writeUInt32LE(24, 0x98 + 172);
  for (const [index, value] of [0x500000 + rva + 0x40, 0x500000 + rva + 0x44,
    0x500000 + rva + 0x50, 0x500000 + rva + 0x60, 0, 0].entries()) {
    bytes.writeUInt32LE(value, raw + index * 4);
  }
  bytes.writeUInt32LE(0xdeadbeef, raw + 0x40);
  bytes.writeUInt32LE(0xaaaaaaaa, raw + 0x50);
  bytes.writeUInt32LE(0x501000, raw + 0x60);
  bytes.writeUInt32LE(0, raw + 0x64);
  return bytes;
}
