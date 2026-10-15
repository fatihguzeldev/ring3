import {builder} from '../../../../support/pe32/image.mjs';

export function image() {
  const symbols = [
    'CreateFileA', 'ReadFile', 'WriteFile', 'SetFilePointer', 'GetFileSize',
    'CreateDirectoryA', 'DeleteFileA', 'MoveFileA', 'FindFirstFileA',
    'FindNextFileA', 'FindClose', 'GetLastError', 'CloseHandle',
    'DuplicateHandle', 'CreateFileMappingA', 'MapViewOfFile',
    'FlushViewOfFile', 'UnmapViewOfFile', 'CreateEventA', 'WaitForSingleObject',
    'GetOverlappedResult', 'CancelIoEx', 'CopyFileExA', 'ExitProcess',
  ];
  const b = builder([['kernel32.dll', symbols]], {
    callbacks: [{words: 13, result: 0, record: 'copyProgress'}],
  });
  const {allocate, data, string, ref, external} = b;
  const gate = {
    CreateFileA: 0xa0001,
    ReadFile: 0xa0003,
    WriteFile: 0xa0004,
    SetFilePointer: 0xa0005,
    GetFileSize: 0xa0007,
    CreateDirectoryA: 0xa0015,
    DeleteFileA: 0xa0022,
    MoveFileA: 0xa0028,
    FindFirstFileA: 0xa001b,
    FindNextFileA: 0xa001f,
    FindClose: 0xa0021,
    CreateFileMappingA: 0xa0080,
    MapViewOfFile: 0xa0084,
    FlushViewOfFile: 0xa0087,
    UnmapViewOfFile: 0xa0086,
    GetOverlappedResult: 0xa0088,
    CancelIoEx: 0xa008b,
    CopyFileExA: 0xa0026,
    GetLastError: 0x10001,
    CloseHandle: 0x10018,
    DuplicateHandle: 0x10019,
    CreateEventA: 0x10011,
    WaitForSingleObject: 0x1001a,
    ExitProcess: 0x10003,
  };
  const call = (name, args, expected = 1, out, hook) =>
    external(name, gate[name], args, expected, out, hook);

  for (const name of [
    'asset', 'mapping', 'view', 'alias', 'copy', 'save', 'duplicate',
    'async', 'event', 'count', 'readWord', 'copyWord', 'large', 'largeHigh',
  ]) allocate(name);
  allocate('buffer', Array(8).fill(0));
  allocate('find', Array(80).fill(0));
  allocate('overlapped', Array(5).fill(0));
  const assetPath = string('assetPath', 'c:/ASSETS/data.bin');
  const saves = string('saves', 'C:\\saves');
  const savePath = string('savePath', 'C:\\saves\\slot.dat');
  const movedPath = string('movedPath', 'C:\\saves\\renamed.dat');
  const pattern = string('pattern', 'C:\\saves\\*.dat');
  const copiedPath = string('copiedPath', 'C:\\saves\\copied.dat');
  const largePath = string('largePath', 'C:\\assets\\large.bin');
  const saved = data('saved', [0x12345678, 0x90abcdef]);

  call('CreateDirectoryA', [saves, 0]);
  call('CreateFileA', [assetPath, 0xc0000000, 7, 0, 3, 0, 0], 'handle', 'asset');
  call('ReadFile', [ref('asset'), b.addresses.buffer, 8, b.addresses.count, 0], 1, undefined, 'asset-read');
  b.copy(b.addresses.readWord, b.addresses.buffer);
  call('GetFileSize', [ref('asset'), 0], 8);
  call('CreateFileA', [largePath, 0x80000000, 7, 0, 3, 0, 0], 'handle', 'large');
  call('GetFileSize', [ref('large'), b.addresses.largeHigh], 0, undefined, 'large-size');
  call('ReadFile', [ref('large'), b.addresses.buffer, 4, b.addresses.count, 0], 1, undefined, 'large-read');
  call('CloseHandle', [ref('large')]);

  call('CreateFileMappingA', [ref('asset'), 0, 4, 0, 0, 0], 'handle', 'mapping');
  call('MapViewOfFile', [ref('mapping'), 2, 0, 0, 8], 'pointer', 'view');
  call('MapViewOfFile', [ref('mapping'), 4, 0, 0, 8], 'pointer', 'alias');
  call('MapViewOfFile', [ref('mapping'), 1, 0, 0, 8], 'pointer', 'copy');
  b.writeLocked(b.addresses.view, [0x11223344]);
  b.readLocked(b.addresses.alias, b.addresses.readWord);
  b.readLocked(b.addresses.copy, b.addresses.copyWord);
  b.writeLocked(b.addresses.copy, [0x55667788]);
  call('FlushViewOfFile', [ref('view'), 4], 1, undefined, 'mapped-coherence');
  call('CloseHandle', [ref('mapping')]);
  call('CloseHandle', [ref('asset')]);
  call('UnmapViewOfFile', [ref('view')]);
  call('UnmapViewOfFile', [ref('alias')]);
  call('UnmapViewOfFile', [ref('copy')]);

  call('CreateFileA', [savePath, 0xc0000000, 7, 0, 2, 0, 0], 'handle', 'save');
  call('WriteFile', [ref('save'), saved, 8, b.addresses.count, 0]);
  call('DuplicateHandle', [0xffffffff, ref('save'), 0xffffffff, b.addresses.duplicate, 0, 0, 2]);
  call('SetFilePointer', [ref('duplicate'), 0, 0, 0], 0);
  call('ReadFile', [ref('save'), b.addresses.buffer, 8, b.addresses.count, 0], 1, undefined, 'save-read');
  call('CloseHandle', [ref('save')]);
  call('CloseHandle', [ref('duplicate')]);
  call('MoveFileA', [savePath, movedPath]);
  call('FindFirstFileA', [pattern, b.addresses.find], 'handle', 'save');
  call('FindNextFileA', [ref('save'), b.addresses.find], 0);
  call('GetLastError', [], 18);
  call('FindClose', [ref('save')]);
  call('CloseHandle', [ref('save')], 0);
  call('GetLastError', [], 6);
  call('CopyFileExA', [movedPath, copiedPath, b.callbacks[0].entry, 0xfeedbeef, 0, 1], 1, undefined, 'copy-progress');

  call('CreateFileA', [assetPath, 0x80000000, 7, 0, 3, 0x40000000, 0], 'handle', 'async');
  call('CreateEventA', [0, 1, 1, 0], 'handle', 'event');
  b.copy(b.addresses.overlapped + 16, b.addresses.event);
  call('ReadFile', [ref('async'), b.addresses.buffer, 8, b.addresses.count, b.addresses.overlapped], 0);
  call('GetLastError', [], 997);
  call('WaitForSingleObject', [ref('event'), 0], 0x102);
  call('GetOverlappedResult', [ref('async'), b.addresses.overlapped, b.addresses.count, 1], 1, undefined, 'async-progress');
  call('WaitForSingleObject', [ref('event'), 0], 0);
  call('WaitForSingleObject', [ref('async'), 0], 0);
  call('ReadFile', [ref('async'), b.addresses.buffer, 8, b.addresses.count, b.addresses.overlapped], 0);
  call('CancelIoEx', [ref('async'), b.addresses.overlapped]);
  call('GetOverlappedResult', [ref('async'), b.addresses.overlapped, b.addresses.count, 1], 0, undefined, 'cancel-progress');
  call('GetLastError', [], 995);
  call('CloseHandle', [ref('async')]);
  call('CloseHandle', [ref('event')]);
  allocate('exitScratch');
  allocate('exitAsset');
  allocate('exitMapping');
  allocate('exitView');
  const scratchPath = string('scratchPath', 'C:\\saves\\scratch.tmp');
  call('CreateFileA', [scratchPath, 0xc0010000, 7, 0, 2, 0x04000080, 0], 'handle', 'exitScratch');
  call('WriteFile', [ref('exitScratch'), saved, 8, b.addresses.count, 0]);
  call('CreateFileA', [assetPath, 0xc0000000, 7, 0, 3, 0, 0], 'handle', 'exitAsset');
  call('CreateFileMappingA', [ref('exitAsset'), 0, 4, 0, 0, 0], 'handle', 'exitMapping');
  call('MapViewOfFile', [ref('exitMapping'), 2, 0, 0, 8], 'pointer', 'exitView');
  b.writeLocked(b.addresses.exitView, [0x44332211]);
  call('ExitProcess', [0], 'exit');
  return b.seal();
}
