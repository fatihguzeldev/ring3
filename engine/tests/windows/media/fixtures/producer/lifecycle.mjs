import assert from 'node:assert/strict';
import {builder} from '../../../../support/pe32/image.mjs';

export const WARMUPS = 32;
export const CYCLES = 256;
export const TOTAL = WARMUPS + CYCLES;
export const SMC = 0x50000000;

const word = value => {
  const bytes = Buffer.alloc(4);
  bytes.writeUInt32LE(value >>> 0);
  return [...bytes];
};

export function lifecycleImage() {
  const kernel = [
    'CreateEventA', 'WaitForSingleObject', 'CloseHandle', 'ExitProcess',
    'VirtualAlloc', 'VirtualFree', 'FlushInstructionCache', 'CreateThread',
    'GetExitCodeThread', 'GetCurrentThreadId', 'QueueUserAPC', 'SleepEx',
    'CreateFileA', 'SetFilePointer', 'ReadFile', 'WriteFile',
  ];
  const b = builder([
    ['kernel32.dll', kernel],
    ['user32.dll', ['RegisterClassA', 'CreateWindowExA', 'DestroyWindow']],
    ['dsound.dll', ['DirectSoundCreate8', 'DirectSoundCaptureCreate8']],
  ], {
    callbacks: [
      {words: 4, result: 1, record: 'windowProc'},
      {words: 1, result: 0, record: 'apc'},
    ],
  });
  const {allocate, data, string, ref} = b;
  for (const name of [
    'window', 'event', 'sound', 'capture', 'buffer', 'notify', 'captureBuffer',
    'captureNotify', 'pointer1', 'length1', 'pointer2', 'length2', 'captureWord',
    'childHandle', 'childTid', 'childParameter', 'childTeb', 'childTidFromFs',
    'childExit', 'childCount', 'mainTeb', 'apcTid', 'apcTeb', 'round', 'file',
    'fileCount', 'fileWord', 'smcPointer', 'smcBefore', 'smcUnflushed', 'smcAfter',
  ]) allocate(name);
  const a = b.addresses;
  const filePath = string('filePath', 'C:\\lifecycle.bin');
  const className = string('className', 'Ring3Lifecycle');
  const title = string('title', 'authored lifecycle');
  const wndclass = data('wndclass', [0, b.callbacks[0].entry, 0, 0, 0x400000, 0, 0, 0, 0, className]);
  const format = data('format', [0x10001, 24000, 24000, 0x80001, 0]);
  const playback = data('playbackDescription', [36, 0x101e0, 4, 0, format, 0, 0, 0, 0]);
  const capture = data('captureDescription', [28, 0, 4, 0, format, 0, 0]);
  const iidNotify = data('iidNotify', [0xb0210783, 0x11d089cd, 0xa00008af, 0x16cd25c9]);
  const positions = data('positions', [2, 0]);
  const childEntry = 0x403800;
  const notes = [];
  let phase = 'setup';
  function call(name, id, args, expected = 0, output, hook) {
    b.external(name, id, args, expected, output, hook);
    notes.push({phase, arguments: args});
  }
  function com(name, object, tag, slot, args = [], expected = 0, hook) {
    b.com(name, object, tag, slot, args, expected, hook);
    notes.push({phase, arguments: [{address: a[object]}, ...args]});
  }
  b.emit(0x64, 0xa1, ...word(0x18));
  b.save(a.mainTeb);
  call('RegisterClassA', 0x20001, [wndclass], 'atom');
  call('CreateWindowExA', 0x20007,
    [0, className, title, 0x10000000, 0, 0, 64, 64, 0, 0, 0x400000, 0], 'pointer', 'window');
  call('CreateEventA', 0x10011, [0, 0, 0, 0], 'pointer', 'event');
  call('DirectSoundCreate8', 0x60002, [0, a.sound, 0]);
  com('CooperativeLevel', 'sound', 0x70200, 6, [ref('window'), 2]);
  call('DirectSoundCaptureCreate8', 0x60006, [0, a.capture, 0]);
  call('VirtualAlloc', 0x10005, [SMC, 4096, 0x3000, 0x40], SMC, 'smcPointer');
  call('CreateFileA', 0xa0001, [filePath, 0xc0000000, 7, 0, 2, 0, 0], 'pointer', 'file');
  call('CloseHandle', 0x10018, [ref('file')], 1);
  b.copy(positions + 4, a.event);
  const loopMarker = Buffer.from([0x90, 0x90, 0x90, 0x90, 0x90]);
  b.emit(...loopMarker);

  phase = 'cycle';
  com('CreatePlayback', 'sound', 0x70200, 3, [playback, a.buffer, 0]);
  com('PlaybackNotify', 'buffer', 0x70400, 0, [iidNotify, a.notify]);
  com('PlaybackNotification', 'notify', 0x70500, 3, [1, positions]);
  com('PlaybackLock', 'buffer', 0x70400, 11,
    [0, 4, a.pointer1, a.length1, a.pointer2, a.length2, 0]);
  b.writeLocked(a.pointer1, [0x4080c080]);
  com('PlaybackUnlock', 'buffer', 0x70400, 19,
    [ref('pointer1'), ref('length1'), ref('pointer2'), ref('length2')]);
  com('PlaybackStart', 'buffer', 0x70400, 12, [0, 0, 1]);
  call('WaitForSingleObject', 0x1001a, [ref('event'), 0xffffffff], 0, undefined, 'audio');
  com('PlaybackStop', 'buffer', 0x70400, 18);
  com('PlaybackNotifyRelease', 'notify', 0x70500, 2);
  com('PlaybackRelease', 'buffer', 0x70400, 2);

  com('CreateCapture', 'capture', 0x70800, 3, [capture, a.captureBuffer, 0]);
  com('CaptureNotify', 'captureBuffer', 0x70a00, 0, [iidNotify, a.captureNotify]);
  com('CaptureNotification', 'captureNotify', 0x70500, 3, [1, positions]);
  com('CaptureStart', 'captureBuffer', 0x70a00, 9, [1]);
  call('WaitForSingleObject', 0x1001a, [ref('event'), 0xffffffff], 0, undefined, 'capture');
  com('CaptureLock', 'captureBuffer', 0x70a00, 8,
    [0, 4, a.pointer1, a.length1, a.pointer2, a.length2, 0]);
  b.readLocked(a.pointer1, a.captureWord);
  com('CaptureUnlock', 'captureBuffer', 0x70a00, 11,
    [ref('pointer1'), ref('length1'), ref('pointer2'), ref('length2')]);
  com('CaptureStop', 'captureBuffer', 0x70a00, 10);
  com('CaptureNotifyRelease', 'captureNotify', 0x70500, 2);
  com('CaptureRelease', 'captureBuffer', 0x70a00, 2);

  call('CreateThread', 0xf0001, [0, 0, childEntry, ref('round'), 0, a.childTid], 'pointer', 'childHandle');
  call('WaitForSingleObject', 0x1001a, [ref('childHandle'), 0xffffffff], 0, undefined, 'join');
  call('GetExitCodeThread', 0xf0004, [ref('childHandle'), a.childExit], 1);
  call('CloseHandle', 0x10018, [ref('childHandle')], 1);
  call('QueueUserAPC', 0x100018, [b.callbacks[1].entry, 0xfffffffe, ref('round')], 1);
  call('SleepEx', 0x1001f, [0, 1], 0xc0, undefined, 'apc');

  call('CreateFileA', 0xa0001, [filePath, 0xc0000000, 7, 0, 3, 0, 0], 'pointer', 'file');
  call('SetFilePointer', 0xa0005, [ref('file'), 0, 0, 2], 'offset');
  call('WriteFile', 0xa0004, [ref('file'), a.round, 4, a.fileCount, 0], 1);
  call('SetFilePointer', 0xa0005, [ref('file'), 0xfffffffc, 0, 1], 'offset');
  call('ReadFile', 0xa0003, [ref('file'), a.fileWord, 4, a.fileCount, 0], 1);
  call('CloseHandle', 0x10018, [ref('file')], 1);

  b.writeLocked(a.smcPointer, [0x000000b8, 0x0000c300]);
  b.emit(0xa1, ...word(a.round), 0x35, ...word(0x13570000),
    0x8b, 0x0d, ...word(a.smcPointer), 0x89, 0x41, 1);
  call('FlushInstructionCache', 0x10008, [0xffffffff, ref('smcPointer'), 6], 1, undefined, 'smc-before');
  b.emit(0xff, 0x15, ...word(a.smcPointer));
  b.save(a.smcBefore);
  b.emit(0xa1, ...word(a.round), 0x35, ...word(0x5a000000),
    0x8b, 0x0d, ...word(a.smcPointer), 0x89, 0x41, 1);
  b.emit(0xff, 0x15, ...word(a.smcPointer));
  b.save(a.smcUnflushed);
  call('FlushInstructionCache', 0x10008, [0xffffffff, ref('smcPointer'), 6], 1, undefined, 'smc-after');
  b.emit(0xff, 0x15, ...word(a.smcPointer));
  b.save(a.smcAfter);
  call('GetCurrentThreadId', 0xc0002, [], 1, undefined, 'checkpoint');
  b.emit(0xff, 0x05, ...word(a.round), 0x81, 0x3d, ...word(a.round), ...word(TOTAL),
    0x0f, 0x85, 0, 0, 0, 0);

  phase = 'cleanup';
  call('CloseHandle', 0x10018, [ref('event')], 1);
  com('CaptureDeviceRelease', 'capture', 0x70800, 2);
  com('SoundDeviceRelease', 'sound', 0x70200, 2);
  call('DestroyWindow', 0x20009, [ref('window')], 1);
  call('VirtualFree', 0x10006, [ref('smcPointer'), 0, 0x8000], 1);
  call('ExitProcess', 0x10003, [0], 'exit');
  const guest = b.seal();
  guest.calls = guest.calls.map((call, index) => ({...call, ...notes[index]}));
  assert.equal(guest.calls.length, notes.length);
  const loopAt = guest.bytes.indexOf(loopMarker, 0x200);
  assert.ok(loopAt >= 0x200 && loopAt < 0x1000);
  const checkpoint = guest.calls.find(call => call.hook === 'checkpoint');
  const jumpAt = 0x200 + checkpoint.end + 5 + 6 + 10;
  assert.equal(guest.bytes.readUInt16LE(jumpAt), 0x850f);
  guest.bytes.writeInt32LE(loopAt - (jumpAt + 6), jumpAt + 2);
  assert.ok(guest.entry + guest.calls.at(-1).end < childEntry);
  const smcCall = Buffer.from([0xff, 0x15, ...word(a.smcPointer)]);
  const mainCode = guest.bytes.subarray(0x200, 0x200 + guest.calls.at(-1).end);
  guest.smcReturns = [];
  for (let at = 0; (at = mainCode.indexOf(smcCall, at)) !== -1; at += smcCall.length) {
    guest.smcReturns.push(guest.entry + at + smcCall.length);
  }
  assert.equal(guest.smcReturns.length, 3);

  function routine(entry, body) {
    guest.bytes.set(body, 0x200 + entry - guest.entry);
    return {entry, length: body.length};
  }
  guest.child = routine(childEntry, [
    0x8b, 0x44, 0x24, 4, 0xa3, ...word(a.childParameter),
    0x64, 0xa1, ...word(0x18), 0xa3, ...word(a.childTeb),
    0x64, 0xa1, ...word(0x24), 0xa3, ...word(a.childTidFromFs),
    0xff, 0x05, ...word(a.childCount),
    0x8b, 0x44, 0x24, 4, 0x35, ...word(0x13572468), 0xc2, 4, 0,
  ]);
  guest.apc = routine(b.callbacks[1].entry, [
    0x8b, 0x44, 0x24, 4, 0xa3, ...word(a.apcArguments),
    0x64, 0xa1, ...word(0x18), 0xa3, ...word(a.apcTeb),
    0x64, 0xa1, ...word(0x24), 0xa3, ...word(a.apcTid),
    0xff, 0x05, ...word(a.apcCount), 0xb8, ...word(0), 0xc2, 4, 0,
  ]);
  return guest;
}
