import {builder} from '../../../../support/pe32/image.mjs';

export function image() {
  const b = builder([
    ['kernel32.dll', ['LocalFree', 'ExitProcess', 'FormatMessageA', 'FormatMessageW', 'GetLastError']],
    ['user32.dll', ['GetDC', 'ReleaseDC']],
    ['gdi32.dll', ['SetPixel', 'GetPixel', 'SetTextColor', 'TextOutW', 'GetTextExtentPoint32W', 'CreateDIBSection']],
    ['oleaut32.dll', ['SysAllocStringLen', 'SysStringLen', 'SysStringByteLen', 'SysFreeString']],
    ['shlwapi.dll', ['StrCmpNIA']],
    ['shell32.dll', ['CommandLineToArgvW']],
  ]);
  for (const name of ['dc', 'bstr', 'argv', 'argc', 'dib', 'bits', 'message']) b.allocate(name);
  b.allocate('extent', [0, 0]);
  b.allocate('message_buffer', Array(16).fill(0));
  const utf16 = (name, value) => {
    const bytes = Buffer.from(value + '\0', 'utf16le');
    const padded = Buffer.alloc((bytes.length + 3) & ~3); bytes.copy(padded);
    return b.data(name, Array.from({length: padded.length / 4}, (_, i) => padded.readUInt32LE(i * 4)));
  };
  const text = utf16('text', 'A');
  const embedded = utf16('embedded', 'a\0b');
  const command = utf16('command', 'program "a b" "" x');
  const message = utf16('message_source', 'hello %1!u!');
  const short = utf16('short_source', '1234');
  const ansi = b.data('ansi_source', [0xe9636163, 0]);
  const formatArgs = b.data('format_arguments', [7]);
  const shortOutput = b.data('short_output', [0x23451234, 0x45673456]);
  const info = b.data('info', [40, 2, 0xfffffffe, 1 | (32 << 16), 0, 0, 0, 0, 0, 0]);
  const rows = [];
  const call = (name, id, args, expected, output, hook) => {
    b.external(name, id, args, expected, output, hook); rows.push(args.length);
  };
  call('GetDC', 0x1a005a, [0], 'nonzero', 'dc');
  call('SetPixel', 0x1a0024, [b.ref('dc'), 1, 2, 0x112233], 0x112233, undefined, 'frame');
  call('GetPixel', 0x1a0023, [b.ref('dc'), 1, 2], 0x112233);
  call('SetTextColor', 0x1a0029, [b.ref('dc'), 0xffffff], 0);
  call('GetTextExtentPoint32W', 0x1a0070, [b.ref('dc'), text, 1, b.addresses.extent], 1);
  call('TextOutW', 0x1a007a, [b.ref('dc'), 2, 1, text, 1], 1, undefined, 'text');
  call('CreateDIBSection', 0x1a0016, [0, info, 0, b.addresses.bits, 0, 0], 'nonzero', 'dib');
  b.writeLocked(b.addresses.bits, [0x00aabbcc]);
  call('ReleaseDC', 0x1a005c, [0, b.ref('dc')], 1);
  call('SysAllocStringLen', 0x200002, [embedded, 3], 'nonzero', 'bstr');
  call('SysStringLen', 0x200007, [b.ref('bstr')], 3);
  call('SysStringByteLen', 0x200008, [b.ref('bstr')], 6);
  call('SysFreeString', 0x200006, [b.ref('bstr')], 0);
  call('StrCmpNIA', 0x1c002b, [0xfffffff0, 0xffffffe0, 0], 0);
  call('CommandLineToArgvW', 0x1d0005, [command, b.addresses.argc], 'nonzero', 'argv');
  call('LocalFree', 0xb0010, [b.ref('argv')], 0);
  call('FormatMessageW', 0x130019, [0x2500, message, 0, 0, b.addresses.message, 16, formatArgs], 7, undefined, 'message');
  call('LocalFree', 0xb0010, [b.ref('message')], 0);
  call('FormatMessageA', 0x130018, [0x400, ansi, 0, 0, b.addresses.message_buffer, 64, 0], 4, undefined, 'ansi-message');
  call('FormatMessageW', 0x130019, [0x400, short, 0, 0, shortOutput, 3, 0], 0, undefined, 'short-message');
  call('GetLastError', 0x10001, [], 122);
  call('FormatMessageW', 0x130019, [0x1200, 0xfffffff0, 5, 0, b.addresses.message_buffer, 32, 0], 19);
  call('GetLastError', 0x10001, [], 122);
  call('ExitProcess', 0x10003, [0], 'exit');
  const guest = b.seal();
  let start = guest.entry;
  guest.segments = guest.calls.map((call, i) => {
    const segment = {...call, start, limit: guest.entry + call.end, words: rows[i], thread: 1};
    start = segment.limit; return segment;
  });
  return guest;
}
