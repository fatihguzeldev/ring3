import {builder} from '../../../../../support/pe32/image.mjs';

export function image() {
  const b = builder([
    ['kernel32.dll', ['GlobalAlloc', 'GlobalFree', 'GlobalLock', 'GlobalUnlock', 'GlobalSize', 'GetLastError', 'ExitProcess']],
    ['user32.dll', ['OpenClipboard', 'CloseClipboard', 'EmptyClipboard', 'SetClipboardData', 'GetClipboardData',
      'EnumClipboardFormats', 'CountClipboardFormats', 'GetPriorityClipboardFormat', 'SetSysColors', 'GetSysColor']],
  ]);
  for (const name of ['global', 'pointer', 'ansi']) b.allocate(name);
  const priority = b.data('priority', [13, 1]);
  const indices = b.data('indices', [15]);
  const colors = b.data('colors', [0x332211]);
  const words = [];
  const call = (name, id, args, expected, output, hook) => {
    b.external(name, id, args, expected, output, hook);
    words.push(args.length);
  };
  call('GlobalAlloc', 0xb0016, [2, 8], 'nonzero', 'global');
  call('GlobalLock', 0xb0019, [b.ref('global')], 'nonzero', 'pointer');
  b.writeLocked(b.addresses.pointer, [0x00df0041, 0]);
  call('GlobalUnlock', 0xb001a, [b.ref('global')], 0, undefined, 'utf16');
  call('OpenClipboard', 0x200b0, [0], 1);
  call('EmptyClipboard', 0x200b2, [], 1);
  call('SetClipboardData', 0x200b4, [13, b.ref('global')], {ref: 'global'});
  call('CloseClipboard', 0x200b1, [], 1);
  call('OpenClipboard', 0x200b0, [0], 1);
  call('GetClipboardData', 0x200b3, [13], {ref: 'global'});
  call('GetClipboardData', 0x200b3, [1], 'nonzero', 'ansi');
  call('GlobalLock', 0xb0019, [b.ref('ansi')], 'nonzero', undefined, 'ansi');
  call('GetClipboardData', 0x200b3, [1], {ref: 'ansi'});
  call('EnumClipboardFormats', 0x200b7, [0], 13);
  call('EnumClipboardFormats', 0x200b7, [13], 16);
  call('CountClipboardFormats', 0x200b6, [], 4);
  call('GetPriorityClipboardFormat', 0x200bf, [priority, 2], 13);
  call('GlobalFree', 0xb0018, [b.ref('global')], {ref: 'global'});
  call('GetLastError', 0x10001, [], 6);
  call('EmptyClipboard', 0x200b2, [], 1);
  call('GlobalSize', 0xb001b, [b.ref('global')], 0);
  call('GlobalSize', 0xb001b, [b.ref('ansi')], 0);
  call('GetLastError', 0x10001, [], 6);
  call('CloseClipboard', 0x200b1, [], 1);
  call('SetSysColors', 0x200cd, [1, indices, colors], 1);
  call('GetSysColor', 0x1a0086, [15], 0x332211);
  call('ExitProcess', 0x10003, [0], 'exit');
  const guest = b.seal();
  let start = guest.entry;
  guest.segments = guest.calls.map((call, index) => {
    const segment = {...call, start, limit: guest.entry + call.end, words: words[index], thread: 1};
    start = segment.limit;
    return segment;
  });
  return guest;
}
