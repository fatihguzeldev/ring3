import assert from 'node:assert/strict';
import {builder} from '../../../../support/pe32/image.mjs';

const word = value => {const bytes = Buffer.alloc(4); bytes.writeUInt32LE(value >>> 0); return [...bytes];};
const queryEntry = 0x404d00, addEntry = 0x404e00, releaseEntry = 0x404f00;
const invokeEntries = [0x404a00, 0x404b00, 0x404c00];

export function image() {
  const b = builder([
    ['kernel32.dll', ['ExitProcess']],
    ['oleaut32.dll', ['VariantCopy', 'VariantCopyInd', 'VariantClear', 'VariantChangeTypeEx', 'SysAllocStringLen', 'SafeArrayCreateVector', 'SafeArrayGetElement', 'SafeArrayPutElement', 'SafeArrayCopy', 'SafeArrayRedim', 'SafeArrayDestroy']],
  ], {callbacks: [{words: 1}, {words: 1}]});
  const table = b.data('table', [queryEntry, addEntry, releaseEntry]);
  const first = b.allocate('first', [table, 1, 0, 0, 0, 0]);
  const old = b.allocate('old', [table, 1, 0, 0, 0, 0]);
  const rejecting = b.allocate('rejecting', [table, 1, 0, 0, 0, 1]);
  const borrowed = b.data('borrowed', [first]);
  b.allocate('source', [13, 0, first, 0]);
  b.allocate('destination', [9, 0, old, 0]);
  b.allocate('second', [0, 0, 0, 0]);
  b.allocate('indirect', [0x400d, 0, borrowed, 0]);
  b.allocate('rejectingSource', [13, 0, rejecting, 0]);
  for (const [index, name] of ['numeric', 'failure', 'conversion'].entries()) {
    const table = b.data(`${name}Table`, [queryEntry, addEntry, releaseEntry, 0, 0, 0, invokeEntries[index]]);
    const object = b.allocate(name, [table, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    b.allocate(`${name}Source`, [9, 0, object, 0]);
  }
  const arrayObject = b.allocate('arrayObject', [table, 1, 0, 0, 0, 0]);
  b.allocate('arrayOwner', [13, 0, arrayObject, 0]);
  for (const name of ['array', 'arrayCopy', 'element', 'variantArray', 'variantCopy']) b.allocate(name);
  b.allocate('arrayReturned', [13, 0, 0, 0]);
  b.allocate('stringSource', [8, 0, 0, 0]);
  b.allocate('variantReturned', [0xa5a5a5a5, 0xa5a5a5a5, 0xa5a5a5a5, 0xa5a5a5a5]);
  const indexZero = b.data('indexZero', [0]), indexOne = b.data('indexOne', [1]);
  const reducedBound = b.data('reducedBound', [1, 7]), embedded = b.data('embedded', [0x61, 0x7a]);
  const rows = [];
  const call = (name, id, args, expected = 0, hook) => {
    b.external(name, id, args, expected, undefined, hook); rows.push(args.length);
  };
  const a = b.addresses;
  call('VariantCopy', 0x20001b, [a.destination, a.source], 0, 'copy');
  call('VariantClear', 0x20001a, [a.destination], 0, 'clear-copy');
  call('VariantChangeTypeEx', 0x20001e, [a.destination, a.source, 0x407, 0, 9], 0, 'query');
  call('VariantChangeTypeEx', 0x20001e, [a.second, a.destination, 0x407, 0, 9], 0, 'same-type');
  call('VariantClear', 0x20001a, [a.second]);
  call('VariantClear', 0x20001a, [a.destination]);
  b.write(a.destination, 3); b.write(a.destination + 8, 37);
  call('VariantChangeTypeEx', 0x20001e, [a.destination, a.rejectingSource, 0x407, 0, 9], 0x80004002, 'query-failure');
  call('VariantCopyInd', 0x20001c, [a.destination, a.indirect], 0, 'indirect');
  call('VariantClear', 0x20001a, [a.destination]);
  call('VariantCopy', 0x20001b, [a.destination, a.indirect], 0, 'borrowed');
  call('VariantClear', 0x20001a, [a.destination], 0, 'clear-borrowed');
  call('VariantClear', 0x20001a, [a.rejectingSource]);
  call('VariantClear', 0x20001a, [a.source]);
  call('VariantChangeTypeEx', 0x20001e, [a.destination, a.numericSource, 0x407, 0, 3], 0, 'invoke-numeric');
  call('VariantClear', 0x20001a, [a.destination]);
  call('VariantChangeTypeEx', 0x20001e, [a.destination, a.failureSource, 0x407, 0, 3], 0x80020005, 'invoke-failure');
  call('VariantChangeTypeEx', 0x20001e, [a.destination, a.conversionSource, 0x407, 0, 3], 0x80020005, 'invoke-conversion');
  call('VariantClear', 0x20001a, [a.numericSource]);
  call('VariantClear', 0x20001a, [a.failureSource]);
  call('VariantClear', 0x20001a, [a.conversionSource]);
  const arrayCall = (name, id, args, expected, hook, output) => {
    b.external(name, id, args, expected, output, hook); rows.push(args.length);
  };
  arrayCall('SafeArrayCreateVector', 0x20000a, [13, 0, 2], 'pointer', 'array-created', 'array');
  call('SafeArrayPutElement', 0x200015, [b.ref('array'), indexZero, arrayObject], 0, 'array-put-zero');
  call('SafeArrayPutElement', 0x200015, [b.ref('array'), indexOne, arrayObject], 0, 'array-put-one');
  call('SafeArrayGetElement', 0x200014, [b.ref('array'), indexZero, a.element], 0, 'array-get');
  b.copy(a.arrayReturned + 8, a.element);
  call('VariantClear', 0x20001a, [a.arrayReturned], 0, 'array-release-get');
  call('SafeArrayCopy', 0x200016, [b.ref('array'), a.arrayCopy], 0, 'array-copy');
  call('SafeArrayRedim', 0x200017, [b.ref('array'), reducedBound], 0, 'array-redim');
  call('SafeArrayDestroy', 0x20000b, [b.ref('arrayCopy')], 0, 'array-destroy-copy');
  call('SafeArrayDestroy', 0x20000b, [b.ref('array')], 0, 'array-destroy');
  call('VariantClear', 0x20001a, [a.arrayOwner], 0, 'array-owner-release');
  arrayCall('SysAllocStringLen', 0x200002, [embedded, 3], 'pointer', 'variant-string-created');
  b.save(a.stringSource + 8);
  arrayCall('SafeArrayCreateVector', 0x20000a, [12, 0, 1], 'pointer', 'variant-array-created', 'variantArray');
  call('SafeArrayPutElement', 0x200015, [b.ref('variantArray'), indexZero, a.stringSource], 0, 'variant-array-put');
  call('VariantClear', 0x20001a, [a.stringSource], 0, 'variant-source-clear');
  call('SafeArrayGetElement', 0x200014, [b.ref('variantArray'), indexZero, a.variantReturned], 0, 'variant-array-get');
  call('SafeArrayCopy', 0x200016, [b.ref('variantArray'), a.variantCopy], 0, 'variant-array-copy');
  call('SafeArrayDestroy', 0x20000b, [b.ref('variantArray')], 0, 'variant-array-destroy');
  call('SafeArrayDestroy', 0x20000b, [b.ref('variantCopy')], 0, 'variant-copy-destroy');
  call('VariantClear', 0x20001a, [a.variantReturned], 0, 'variant-result-clear');
  call('ExitProcess', 0x10003, [0], 'exit');
  const guest = b.seal();
  assert.ok(guest.calls.at(-1).end < 0x3d00, 'authored main does not overlap interface methods');
  const add = [0x8b, 0x4c, 0x24, 4, 0xff, 0x41, 4, 0xff, 0x41, 8, 0x8b, 0x41, 4, 0xc2, 4, 0];
  const release = [0x8b, 0x4c, 0x24, 4, 0xff, 0x41, 12, 0xff, 0x49, 4, 0x8b, 0x41, 4, 0xc2, 4, 0];
  const query = [], labels = new Map(), jumps = [];
  const emit = (...bytes) => query.push(...bytes);
  const label = name => labels.set(name, query.length);
  const jump = (opcode, target) => {emit(opcode, 0); jumps.push({at: query.length - 1, target});};
  emit(0x8b, 0x4c, 0x24, 4, 0xff, 0x41, 16, 0x8b, 0x54, 0x24, 8);
  emit(0x81, 0x3a, ...word(0)); jump(0x74, 'identity');
  emit(0x81, 0x3a, ...word(0x00020400)); jump(0x75, 'failure');
  emit(0x83, 0x79, 20, 0); jump(0x75, 'failure');
  label('identity');
  for (const [offset, value] of [[4, 0], [8, 0xc0], [12, 0x46000000]]) {
    emit(0x81, 0x7a, offset, ...word(value)); jump(0x75, 'failure');
  }
  emit(0x8b, 0x54, 0x24, 12, 0x89, 0x0a, 0xff, 0x41, 4, 0xff, 0x41, 8);
  emit(0xb8, ...word(0), 0xc2, 12, 0);
  label('failure');
  emit(0x8b, 0x54, 0x24, 12, 0xc7, 0x02, ...word(0), 0xb8, ...word(0x80004002), 0xc2, 12, 0);
  for (const {at, target} of jumps) {
    const offset = labels.get(target) - at - 1;
    assert.ok(offset >= -128 && offset <= 127); query[at] = offset & 255;
  }
  const invocation = (unknown, status) => {
    const bytes = [], emit = (...values) => bytes.push(...values);
    emit(0x8b, 0x4c, 0x24, 4, 0xff, 0x41, 24);
    for (const [argument, field] of [[8, 28], [16, 32], [20, 36], [28, 52], [32, 56], [36, 60]]) {
      emit(0x8b, 0x44, 0x24, argument, 0x89, 0x41, field);
    }
    emit(0x8b, 0x54, 0x24, 24, 0x8b, 0x42, 8, 0x89, 0x41, 40, 0x8b, 0x42, 12, 0x89, 0x41, 44);
    emit(0x8b, 0x54, 0x24, 12, 0x8b, 0x02, 0x0b, 0x42, 4, 0x0b, 0x42, 8, 0x0b, 0x42, 12, 0x89, 0x41, 48);
    emit(0x8b, 0x54, 0x24, 28, 0xc7, 0x02, ...word(unknown ? 13 : 3));
    emit(0xc7, 0x42, 4, ...word(0), 0xc7, 0x42, 12, ...word(0));
    if (unknown) {
      emit(0x89, 0x4a, 8, 0xff, 0x41, 4, 0xff, 0x41, 8);
    } else {
      emit(0xc7, 0x42, 8, ...word(37));
    }
    emit(0xb8, ...word(status), 0xc2, 36, 0);
    return bytes;
  };
  const procedures = [[queryEntry, query, 3], [addEntry, add, 1], [releaseEntry, release, 1],
    [invokeEntries[0], invocation(false, 0), 9],
    [invokeEntries[1], invocation(true, 0x80020006), 9],
    [invokeEntries[2], invocation(true, 0), 9],
  ];
  for (const [entry, bytes] of procedures) {
    assert.ok(bytes.length <= 0xe0);
    guest.bytes.set(bytes, 0x200 + entry - 0x401000);
  }
  const boundaries = [...new Set([0, query.length, ...labels.values(), ...jumps.map(jump => jump.at + 1)])].sort((a, b) => a - b);
  const queryBlocks = boundaries.slice(0, -1).map((offset, index) => ({entry: queryEntry + offset, length: boundaries[index + 1] - offset}));
  assert.equal(queryBlocks.length, 8, 'authored QI control flow fits the existing descriptor bound');
  guest.procedures = procedures.map(([entry, bytes, words]) => ({entry, length: bytes.length, words,
    blocks: entry === queryEntry ? queryBlocks : [{entry, length: bytes.length}],
    result: entry === invokeEntries[1] ? 0x80020006 : invokeEntries.includes(entry) ? 0 : undefined,
  }));
  let start = guest.entry;
  guest.segments = guest.calls.map((call, index) => {
    const segment = {...call, start, limit: guest.entry + call.end, words: rows[index], thread: 1};
    start = segment.limit; return segment;
  });
  return guest;
}
