import assert from 'node:assert/strict';
import {builder} from '../../../../support/pe32/image.mjs';

const word = value => {
  const bytes = Buffer.alloc(4);
  bytes.writeUInt32LE(value >>> 0);
  return [...bytes];
};
const words = {
  GetCurrentThreadId: 0, Sleep: 1, CreateEventW: 4, WaitForSingleObject: 2,
  CloseHandle: 1, ExitProcess: 1, timeSetEvent: 5, timeKillEvent: 1,
  SetTimer: 4, KillTimer: 2, PeekMessageA: 5, DispatchMessageA: 1,
  CreateActCtxW: 1, ActivateActCtx: 2, DeactivateActCtx: 2, GetCurrentActCtx: 1, ReleaseActCtx: 1,
};

export function image() {
  const b = builder([
    ['kernel32.dll', ['GetCurrentThreadId', 'Sleep', 'CreateEventW', 'WaitForSingleObject', 'CloseHandle', 'ExitProcess', 'CreateActCtxW', 'ActivateActCtx', 'DeactivateActCtx', 'GetCurrentActCtx', 'ReleaseActCtx']],
    ['winmm.dll', ['timeSetEvent', 'timeKillEvent']],
    ['user32.dll', ['SetTimer', 'KillTimer', 'PeekMessageA', 'DispatchMessageA']],
  ], {callbacks: [{words: 4, result: 0xfefefefe, record: 'window'}, {words: 5, result: 0, record: 'multimedia'}]});
  for (const name of ['mainTid', 'windowTid', 'windowTeb', 'multimediaTid', 'multimediaTeb',
    'multimediaSleepStatus', 'mainTeb', 'multimediaTimer', 'event', 'eventTimer', 'windowTimer',
    'warmTimer', 'activation', 'cookie', 'mainContext', 'windowContext', 'multimediaContext']) b.allocate(name);
  b.allocate('message', Array(7).fill(0));
  const specification = b.data('activationSpec', [32, 0x88, 0, 0, 0, 2, 0, 0x400000]);
  const manifest = Buffer.from(`<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0"><assemblyIdentity name="Ring3.Timer.Context" type="win32" version="1.0.0.0"/></assembly>`);
  const resource = Buffer.alloc((0x100 + manifest.length + 3) & ~3);
  for (const offset of [14, 0x2e, 0x4e]) resource.writeUInt16LE(1, offset);
  for (const [offset, value] of [[16, 24], [20, 0x80000020], [0x30, 2], [0x34, 0x80000040], [0x50, 0x409], [0x54, 0x60]]) resource.writeUInt32LE(value, offset);
  const directory = b.data('resourceDirectory', Array(resource.length / 4).fill(0));
  resource.writeUInt32LE(directory - 0x400000 + 0x100, 0x60);
  resource.writeUInt32LE(manifest.length, 0x64);
  manifest.copy(resource, 0x100);
  const a = b.addresses;
  const fs = (offset, address) => b.emit(0x64, 0xa1, ...word(offset), 0xa3, ...word(address));
  fs(0x18, a.mainTeb);
  b.external('GetCurrentThreadId', 0xc0002, [], 1, 'mainTid');
  b.external('timeSetEvent', 0xe0006, [1000, 0, b.callbacks[1].entry, 0, 0], 'pointer', 'warmTimer');
  b.external('timeKillEvent', 0xe0007, [b.ref('warmTimer')], 0);
  b.external('CreateActCtxW', 0x180002, [specification], 'pointer', 'activation');
  b.external('ActivateActCtx', 0x180005, [b.ref('activation'), a.cookie], 1);
  b.external('timeSetEvent', 0xe0006, [10, 0, b.callbacks[1].entry, 0x12345678, 0x101], 'pointer', 'multimediaTimer');
  b.external('DeactivateActCtx', 0x180006, [0, b.ref('cookie')], 1);
  b.external('GetCurrentActCtx', 0x180007, [a.mainContext], 1, undefined, 'main-context-cleared');
  b.external('Sleep', 0x1001e, [10], 0, undefined, 'wait-first-multimedia');
  b.external('timeKillEvent', 0xe0007, [b.ref('multimediaTimer')], 0, undefined, 'synchronous-kill');
  b.external('timeKillEvent', 0xe0007, [b.ref('multimediaTimer')], 11);
  b.external('timeSetEvent', 0xe0006, [10, 0, 0, 0, 0x30], 0);
  b.external('CreateEventW', 0x10012, [0, 0, 0, 0], 'pointer', 'event');
  b.external('timeSetEvent', 0xe0006, [5, 0, b.ref('event'), 0, 0x10], 'pointer', 'eventTimer');
  b.external('WaitForSingleObject', 0x1001a, [b.ref('event'), 0xffffffff], 0, undefined, 'event-timer');
  b.external('timeKillEvent', 0xe0007, [b.ref('eventTimer')], 11);
  b.external('ActivateActCtx', 0x180005, [b.ref('activation'), a.cookie], 1);
  b.external('SetTimer', 0x2005a, [0, 77, 1, b.callbacks[0].entry], 'pointer', 'windowTimer');
  b.external('DeactivateActCtx', 0x180006, [0, b.ref('cookie')], 1);
  b.external('GetCurrentActCtx', 0x180007, [a.mainContext], 1, undefined, 'main-context-cleared');
  b.external('Sleep', 0x1001e, [10], 0, undefined, 'window-due');
  b.external('PeekMessageA', 0x20015, [a.message, 0, 0x113, 0x113, 0], 1, undefined, 'peek-timer');
  b.external('KillTimer', 0x2005b, [0, b.ref('windowTimer')], 1);
  b.external('PeekMessageA', 0x20015, [a.message, 0, 0x113, 0x113, 1], 1);
  b.external('DispatchMessageA', 0x2001a, [a.message], 0, undefined, 'dispatch-timer');
  b.external('PeekMessageA', 0x20015, [a.message, 0, 0x113, 0x113, 1], 0);
  b.external('ReleaseActCtx', 0x180004, [b.ref('activation')], 'void');
  b.external('CloseHandle', 0x10018, [b.ref('event')], 1);
  b.external('ExitProcess', 0x10003, [0], 'exit');
  const guest = b.seal();
  const resourceRva = directory - 0x400000;
  resource.copy(guest.bytes, 0x4200 + resourceRva - 0x6000);
  guest.bytes.writeUInt32LE(resourceRva, 0x98 + 112);
  guest.bytes.writeUInt32LE(resource.length, 0x98 + 116);
  const sleepIat = 0x406404;
  const file = address => 0x200 + address - 0x401000;
  const callback = (definition, tid, teb, context, sleep) => {
    const old = [...guest.bytes.subarray(file(definition.entry), file(definition.entry) + definition.length)];
    const code = old.slice(0, -8);
    code.push(0x64, 0xa1, ...word(0x24), 0xa3, ...word(tid));
    code.push(0x64, 0xa1, ...word(0x18), 0xa3, ...word(teb));
    const currentIat = 0x406424, releaseIat = 0x406428;
    code.push(0x68, ...word(context), 0xff, 0x15, ...word(currentIat));
    const contextEnd = code.length;
    code.push(0xff, 0x35, ...word(context), 0xff, 0x15, ...word(releaseIat));
    const releaseEnd = code.length;
    let callEnd;
    if (sleep) {
      code.push(0x68, ...word(5), 0xff, 0x15, ...word(sleepIat));
      callEnd = code.length;
      code.push(0xa3, ...word(a.multimediaSleepStatus));
    }
    code.push(...old.slice(-8));
    assert.ok(code.length < 0xe0);
    guest.bytes.set(code, file(definition.entry));
    return {...definition, length: code.length, callEnd, contextEnd, releaseEnd};
  };
  const window = callback(guest.callbacks[0], a.windowTid, a.windowTeb, a.windowContext, false);
  const multimedia = callback(guest.callbacks[1], a.multimediaTid, a.multimediaTeb, a.multimediaContext, true);
  let start = guest.entry;
  guest.segments = guest.calls.map(call => {
    const segment = {...call, start, limit: guest.entry + call.end, words: words[call.name], thread: 1};
    start = segment.limit;
    return segment;
  });
  guest.segments.push(
    {name: 'multimedia-context', start: multimedia.entry, limit: multimedia.entry + multimedia.contextEnd,
      id: 0x180007, words: 1, thread: 2, expected: 1},
    {name: 'multimedia-context-release', start: multimedia.entry + multimedia.contextEnd, limit: multimedia.entry + multimedia.releaseEnd,
      id: 0x180004, words: 1, thread: 2, expected: 'void'},
    {name: 'multimedia-sleep', start: multimedia.entry + multimedia.releaseEnd, limit: multimedia.entry + multimedia.callEnd,
      id: 0x1001e, words: 1, thread: 2, expected: 0, hook: 'multimedia-sleep'},
    {name: 'multimedia-return', start: multimedia.entry + multimedia.callEnd, limit: multimedia.entry + multimedia.length,
      id: 0xffffe, words: 0, thread: 2, private: true, expected: 0, hook: 'multimedia-return'},
  );
  return {...guest, windowCallback: window, multimediaCallback: multimedia};
}
