export const endpointSymbols = [
  'CreatePipe', 'GetFileType', 'CreateNamedPipeA', 'ConnectNamedPipe',
  'PeekNamedPipe', 'CreateIoCompletionPort', 'GetQueuedCompletionStatus',
  'PostQueuedCompletionStatus',
];
export const endpointGates = {
  CreatePipe: 0xa00a0,
  GetFileType: 0xa000b,
  CreateNamedPipeA: 0xa00a1,
  ConnectNamedPipe: 0xa00a3,
  PeekNamedPipe: 0xa00a5,
  CreateIoCompletionPort: 0xa0090,
  GetQueuedCompletionStatus: 0xa0092,
  PostQueuedCompletionStatus: 0xa0091,
};

export function appendEndpoints(b, call) {
  const {allocate, data, string, ref} = b;
  for (const name of [
    'pipeRead', 'pipeWrite', 'pipeAlias', 'pipeServer', 'pipeClient',
    'completionPort', 'nullDevice', 'packetCount', 'packetKey', 'packetOverlapped',
  ]) allocate(name);
  allocate('pipeOverlap', Array(5).fill(0));
  allocate('endpointBuffer', Array(2).fill(0));
  const payload = data('endpointData', [0x64636261, 0x65]);
  const nullName = string('nullName', 'NUL.txt');
  const consoleName = string('consoleName', 'CONOUT$');
  const pipeName = string('pipeName', '\\\\.\\pipe\\authored-ring3');
  const count = b.addresses.count;
  const buffer = b.addresses.endpointBuffer;
  const overlap = b.addresses.pipeOverlap;
  const dequeue = (expected, hook) => call('GetQueuedCompletionStatus', [
    ref('completionPort'), b.addresses.packetCount, b.addresses.packetKey,
    b.addresses.packetOverlapped, 0,
  ], expected, undefined, hook);

  call('CreatePipe', [b.addresses.pipeRead, b.addresses.pipeWrite, 0, 32]);
  call('DuplicateHandle', [0xffffffff, ref('pipeRead'), 0xffffffff, b.addresses.pipeAlias, 0, 0, 2]);
  call('CloseHandle', [ref('pipeRead')]);
  call('WriteFile', [ref('pipeWrite'), payload, 5, count, 0]);
  call('ReadFile', [ref('pipeAlias'), buffer, 5, count, 0], 1, undefined, 'anonymous-read');
  call('GetFileType', [ref('pipeAlias')], 3);
  call('GetFileType', [ref('pipeWrite')], 3);
  call('CloseHandle', [ref('pipeAlias')]);
  call('CloseHandle', [ref('pipeWrite')]);

  call('CreateFileA', [nullName, 0xc0000000, 0, 0, 2, 0, 0], 'handle', 'nullDevice');
  call('GetFileType', [ref('nullDevice')], 2);
  call('WriteFile', [ref('nullDevice'), payload, 8, count, 0], 1, undefined, 'null-write');
  call('ReadFile', [ref('nullDevice'), buffer, 5, count, 0], 1, undefined, 'null-eof');
  call('GetFileSize', [ref('nullDevice'), 0], 0);
  call('CloseHandle', [ref('nullDevice')]);
  call('CreateFileA', [consoleName, 0x40000000, 0, 0, 3, 0, 0], 0xffffffff);
  call('GetLastError', [], 2);

  call('CreateNamedPipeA', [pipeName, 0x40000003, 6, 1, 16, 16, 50, 0], 'handle', 'pipeServer');
  call('CreateIoCompletionPort', [ref('pipeServer'), 0, 0x53594e43, 1], 'handle', 'completionPort');
  call('ConnectNamedPipe', [ref('pipeServer'), overlap], 0);
  call('GetLastError', [], 997);
  call('CreateFileA', [pipeName, 0xc0000000, 0, 0, 3, 0, 0], 'handle', 'pipeClient');
  call('GetOverlappedResult', [ref('pipeServer'), overlap, count, 1], 1);
  dequeue(1, 'pipe-connect-packet');
  call('WriteFile', [ref('pipeClient'), payload, 5, count, 0]);
  call('ReadFile', [ref('pipeServer'), buffer, 2, 0, overlap], 0);
  call('GetOverlappedResult', [ref('pipeServer'), overlap, count, 1], 0, undefined, 'pipe-partial-read');
  dequeue(0, 'pipe-partial-packet');
  call('GetLastError', [], 234);
  call('PeekNamedPipe', [ref('pipeServer'), buffer, 3, count, b.addresses.packetCount, b.addresses.packetKey], 1, undefined, 'pipe-peek');
  call('ReadFile', [ref('pipeServer'), buffer, 3, 0, overlap], 0);
  call('GetOverlappedResult', [ref('pipeServer'), overlap, count, 1], 1, undefined, 'pipe-remaining-read');
  dequeue(1, 'pipe-remaining-packet');
  call('CloseHandle', [ref('pipeClient')]);
  call('ReadFile', [ref('pipeServer'), buffer, 1, 0, overlap], 0);
  call('GetOverlappedResult', [ref('pipeServer'), overlap, count, 1], 0);
  call('GetLastError', [], 109);
  dequeue(0, 'pipe-broken-packet');
  call('GetLastError', [], 109);
  call('PostQueuedCompletionStatus', [ref('completionPort'), 19, 0xabc, 0x7777]);
  dequeue(1, 'posted-completion');
  call('CloseHandle', [ref('pipeServer')]);
  call('CloseHandle', [ref('completionPort')]);
}
