import {builder} from '../../../../support/pe32/image.mjs';
import {procedures} from './procedures.mjs';

const stackWords = {
  GetCurrentThreadId: 0, TlsAlloc: 0, TlsSetValue: 2, TlsGetValue: 1, TlsFree: 1,
  InitializeCriticalSectionAndSpinCount: 2, EnterCriticalSection: 1, LeaveCriticalSection: 1,
  DeleteCriticalSection: 1, CreateThread: 6, GetExitCodeThread: 2, ResumeThread: 1,
  WaitForSingleObject: 2, CreateWaitableTimerW: 3, SetWaitableTimer: 6, CancelWaitableTimer: 1,
  QueueUserAPC: 3, SleepEx: 2, GetThreadTimes: 5, CloseHandle: 1, ExitProcess: 1,
};

export function image() {
  const symbols = ['GetCurrentThreadId', 'TlsAlloc', 'TlsSetValue', 'TlsGetValue', 'TlsFree',
    'InitializeCriticalSectionAndSpinCount', 'EnterCriticalSection', 'LeaveCriticalSection',
    'DeleteCriticalSection', 'CreateThread', 'GetExitCodeThread', 'ResumeThread',
    'WaitForSingleObject', 'CreateWaitableTimerW', 'SetWaitableTimer', 'CancelWaitableTimer',
    'QueueUserAPC', 'SleepEx', 'GetThreadTimes', 'CloseHandle', 'ExitProcess'];
  const b = builder([['kernel32.dll', symbols]]), {allocate, external, ref} = b;
  for (const name of ['mainTid', 'mainTeb', 'mainPeb', 'tlsIndex', 'mainTls', 'childHandle',
    'childTid', 'childParameter', 'childTeb', 'childTidFromFs', 'childPeb', 'childTlsInitial',
    'childTlsFinal', 'childEntered', 'childExit', 'timerHandle', 'apcArgument', 'apcTid',
    'apcCount', 'timerTid', 'timerCount']) allocate(name);
  allocate('critical', Array(6).fill(0));
  allocate('due', [0xffffd8f0, 0xffffffff]);
  allocate('timerArguments', [0, 0, 0]);
  allocate('threadTimes', Array(8).fill(0));
  const a = b.addresses, functions = procedures(b);
  const fs = offset => {
    const bytes = Buffer.alloc(4);
    bytes.writeUInt32LE(offset);
    b.emit(0x64, 0xa1, ...bytes);
  };
  fs(0x18); b.save(a.mainTeb);
  fs(0x30); b.save(a.mainPeb);
  external('GetCurrentThreadId', 0xc0002, [], 1, 'mainTid');
  external('TlsAlloc', 0xf0009, [], 0, 'tlsIndex');
  external('TlsSetValue', 0xf000c, [ref('tlsIndex'), 0x1111], 1);
  external('InitializeCriticalSectionAndSpinCount', 0x100012, [a.critical, 0], 1);
  external('EnterCriticalSection', 0x100014, [a.critical], 0);
  external('EnterCriticalSection', 0x100014, [a.critical], 0, undefined, 'recursive-owner');
  external('CreateThread', 0xf0001, [0, 0, functions.child.entry, 0x13572468, 4, a.childTid], 'pointer', 'childHandle', 'suspended-child');
  external('GetExitCodeThread', 0xf0004, [ref('childHandle'), a.childExit], 1, undefined, 'still-active');
  external('ResumeThread', 0xf0006, [ref('childHandle')], 1, undefined, 'schedule-child');
  external('LeaveCriticalSection', 0x100015, [a.critical], 0, undefined, 'still-contended');
  external('LeaveCriticalSection', 0x100015, [a.critical], 0);
  external('WaitForSingleObject', 0x1001a, [ref('childHandle'), 0xffffffff], 0, undefined, 'wait-child');
  external('GetExitCodeThread', 0xf0004, [ref('childHandle'), a.childExit], 1, undefined, 'child-exited');
  external('TlsGetValue', 0xf000b, [ref('tlsIndex')], 0x1111, 'mainTls');
  external('CreateWaitableTimerW', 0x10000c, [0, 0, 0], 'pointer', 'timerHandle');
  external('SetWaitableTimer', 0x10000f, [ref('timerHandle'), a.due, 0, functions.timer.entry, 0x76543210, 0], 1);
  external('QueueUserAPC', 0x100018, [functions.apc.entry, 0xfffffffe, 0xabcdef01], 1);
  external('SleepEx', 0x1001f, [0, 1], 0xc0, undefined, 'wait-apc');
  external('SleepEx', 0x1001f, [0xffffffff, 1], 0xc0, undefined, 'wait-timer');
  external('WaitForSingleObject', 0x1001a, [ref('timerHandle'), 0], 0);
  external('CancelWaitableTimer', 0x100010, [ref('timerHandle')], 1);
  external('GetThreadTimes', 0xf0010, [ref('childHandle'), a.threadTimes, a.threadTimes + 8, a.threadTimes + 16, a.threadTimes + 24], 1, undefined, 'thread-times');
  external('CloseHandle', 0x10018, [ref('timerHandle')], 1);
  external('CloseHandle', 0x10018, [ref('childHandle')], 1);
  external('DeleteCriticalSection', 0x100013, [a.critical], 0);
  external('TlsFree', 0xf000a, [ref('tlsIndex')], 1);
  external('ExitProcess', 0x10003, [0], 'exit');
  const guest = b.seal(), segments = [];
  if (guest.entry + guest.calls.at(-1).end >= functions.child.entry) throw new Error('main code overlaps child');
  let start = guest.entry;
  for (const call of guest.calls) {
    segments.push({...call, start, limit: guest.entry + call.end, thread: 1,
      words: stackWords[call.name]});
    start = guest.entry + call.end;
  }
  guest.segments = [...segments, ...functions.install(guest)];
  guest.procedures = functions;
  return guest;
}
