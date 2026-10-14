import assert from 'node:assert/strict';
import {PC, FLAGS, TRANSFER, word, seed, vector, call, bound, assertCapture} from './oracle.mjs';
import {segments, fxImage, scratch, sourceImage, entryImage, finishArguments} from './images.mjs';

// one context per access seam; scalar cases cover replacement and resident lifecycles.
export const genericRows = [
  {label: 'LOCK ADD checks architectural Write before RMW read/output', family: 'integer', code: 'f08305fe5f000001ebfe',
    resident: true, entries: true, pages: [0x5000, 0x6000], rights: [[0x5000, 3], [0x6000, 1]],
    helper: 'cpu_integer', args: [0x400, 0x5ffe], width: 4, linear: 0x6000, error: 7, access: 2},
  {label: 'FLD memory PF precedes x87 metadata and stack publication', family: 'x87', code: 'd90500600000ebfe',
    pages: [0x6000], rights: [[0x6000, null]], helper: 'x87_op', args: [3, 0x6000, FLAGS, PC, 0x105, 0x401],
    width: 4, linear: 0x6000, error: 4, access: 1},
  {label: 'FSTP Write PF preserves nonempty ST0 and tag', family: 'x87-store', code: 'd91d00600000ebfe',
    resident: true, pages: [0x6000], rights: [[0x6000, 1]], helper: 'x87_op', args: [0x10004, 0x6000, FLAGS, PC, 0x11d, 0x401],
    width: 4, linear: 0x6000, error: 7, access: 2},
  {label: 'MOVDQU cross-page load leaves scratch and XMM unchanged on PF', family: 'load', code: 'f30f6f05f85f0000ebfe',
    entries: true, pages: [0x5000, 0x6000], rights: [[0x5000, 3], [0x6000, null]],
    helper: 'cpu_read', args: [0x5ff8, 16, 1], width: 16, linear: 0x6000, error: 4, access: 1},
  {label: 'MOVDQU cross-page store stages source but writes no early bytes', family: 'store', code: 'f30f7f05f85f0000ebfe',
    resident: true, entries: true, pages: [0x5000, 0x6000], rights: [[0x5000, 3], [0x6000, 1]],
    helper: 'cpu_write', args: [0x5ff8, 16, 1], width: 16, linear: 0x6000, error: 7, access: 2},
  {label: 'FXSAVE tail-only PF belongs to its full 512-byte checked envelope', family: 'fxsave', code: '0fae05305e0000ebfe',
    entries: true, pages: [0x5000, 0x6000], rights: [[0x5000, 3], [0x6000, null]],
    helper: 'cpu_fxsave', args: [0x5e30], prepare: [0x13400b, 0x1918, 0, 0, 0, FLAGS], width: 512, linear: 0x6000, error: 6, access: 2},
  {label: 'FXSAVE success writes 464 bytes and preserves the 48-byte software tail', family: 'fxsave', code: '0fae05305e0000ebfe',
    resident: true, pages: [0x5000, 0x6000], rights: [[0x5000, 3], [0x6000, 3]],
    helper: 'cpu_fxsave', args: [0x5e30], prepare: [0x13400b, 0x1918, 0, 0, 0, FLAGS], width: 512, instructionLength: 7, success: true},
  {label: 'XSAVE request0 checks only header read/write and preserves skipped components', family: 'xsave', code: '0fae23ebfe',
    resident: true, entries: true, pages: [0x6000], rights: [[0x6000, 1]], registers: {0: 0, 2: 0, 3: 0x5e00}, zeroHeader: 0x6000,
    helper: 'cpu_xsave_avx', args: [3, 0x5e00, 0, 0], width: 832, linear: 0x6000, error: 7, access: 2},
  {label: 'XSAVE YMM late PF prevents an earlier selected header write', family: 'xsave', code: '0fae23ebfe',
    pages: [0x5000, 0x6000], rights: [[0x5000, 3], [0x6000, 1]], registers: {0: 4, 2: 0, 3: 0x5dc0}, zeroHeader: 0x5fc0,
    helper: 'cpu_xsave_avx', args: [3, 0x5dc0, 4, 0], width: 832, linear: 0x6000, error: 7, access: 2},
  {label: 'MASKMOV checks selected high bytes and skips the absent low page', family: 'mask', code: '660ff7c1ebfe',
    resident: true, entries: true, pages: [0x6000], rights: [[0x6000, 1]], registers: {7: 0x5ff8},
    helper: 'cpu_mask_write', args: [0x5ff8, 16, 0xff00], prepare: [0x2a40d, 0x908, 0, 0, 0, FLAGS],
    width: 16, linear: 0x6000, error: 7, access: 2},
  {label: 'B16 based PUSHA stages high-to-low modular slots before one transaction', family: 'stack', code: '60ebfe',
    resident: true, pages: [0xfffff000, 0xf000], rights: [[0xfffff000, 3], [0xf000, null]],
    ssBase: 0xfffffff0, ssCache: 0xf030ffff, registers: {4: 0xdead0010},
    helper: 'stack_execute', args: [0], width: 4, linear: 0xffec, error: 6, access: 2},
  {label: 'POPA skips ESP slot and reports the first actually read missing slot', family: 'stack', code: '61ebfe',
    resident: true, entries: true, pages: [0x5000, 0x6000], rights: [[0x5000, 3], [0x6000, null]], registers: {4: 0x5ff4},
    helper: 'stack_execute', args: [2], width: 4, linear: 0x6004, error: 4, access: 1},
  {label: 'ENTER ordered frame read PF prevents the already staged first push', family: 'stack', code: 'c8100002ebfe',
    entries: true, pages: [0x5000, 0x6000], rights: [[0x5000, null], [0x6000, 3]], registers: {4: 0x6010, 5: 0x5008},
    helper: 'stack_execute', args: [0x02001004], width: 4, linear: 0x5004, error: 4, access: 1},

];

function name(c, row) {
  if (!c.resident || row.helper === 'cpu_read') return row.helper;
  return row.helper === 'x87_op' ? 'x87_resident_op' : row.helper + '_resident';
}
function argumentsFor(c, material, row) {return row.helper === 'cpu_read' ? row.args : bound(c, material, row.args);}
function configure(h, c, row) {
  c.capturesExceptions = true;
  for (const [page, permission] of row.rights) {
    if (permission === null) {
      h.host(c, 'unmap', [page, 1]); c.pages = c.pages.filter(mapped => mapped !== page);
    } else h.host(c, 'protect', [page, 1, permission]);
  }
}
function inputImage(h, row) {
  const b = seed(h, {});
  for (const [index, value] of Object.entries(row.registers ?? {})) b.writeUInt32LE(value >>> 0, 16 + 4 * +index);
  if (row.family === 'x87-store') {
    b.writeUInt16LE(0xfffc, 4236 + 20); b.set(Buffer.from('0000000000000080ff3f', 'hex'), 4236 + 40);
  }
  if (row.family === 'mask') {
    const mask = Buffer.concat([b.subarray(4412, 4428), b.subarray(4644, 4660)]);
    mask.fill(0, 0, 8); mask.fill(0x80, 8, 16); vector(b, 1, mask);
  }
  return segments(b, row);
}

export function runGeneric({h, compile, observe, cr2, observed, writeRAM}) {
  for (const row of genericRows) {
    observed.contexts++;
    const pages = [...new Set([PC, 0x2000, 0x7000, ...row.pages])];
    const c = h.open(!!row.resident, !!row.entries, row.label, pages.map(page => [page, page === PC || page === 0x2000 ? 7 : 3]));
    c.diagnosticRAM = true;
    for (const page of pages) h.upload(c, page, Buffer.alloc(4096, 0x6b));
    h.upload(c, PC, Buffer.from(row.code, 'hex'));
    if (row.zeroHeader !== undefined) h.upload(c, row.zeroHeader, Buffer.alloc(24));
    configure(h, c, row); cr2(c, row.label + ': initial CR2 Unknown', undefined);
    const input = inputImage(h, row); h.put(c, 0, input);
    const prepare = 'simd_execute' + (c.resident ? '_resident' : ''), helpers = [name(c, row), 'finish_exception_detection'];
    if (row.prepare) helpers.unshift(prepare);
    const material = compile(c, row.label + ': source', [[PC, row.code.length / 2]], helpers);
    const originalRAM = h.ram(c); let actual = material;
    if (material) {
      const module = new WebAssembly.Module(new Uint8Array(c.memory.buffer, material.pointer, material.length)), checks = [], captures = [];
      const ring3 = Object.fromEntries(WebAssembly.Module.imports(module).filter(i => i.kind === 'function').map(i => [i.name, (...args) => {
        if (i.name === 'finish_exception_detection') {
          const got = h.arena(c), expected = sourceImage(input, row); assertCapture(got, expected, checks, 'complete source PF stop before real finish');
          for (const [page, bytes] of h.ram(c)) if (!bytes.equals(originalRAM.get(page))) checks.push('source PF altered RAM before exception frame publication');
          observed.captures++; captures.push({kind: 'pre-finish', actual: got, expected});
        }
        const status = c.api[i.name](...args);
        if (i.name === name(c, row) || row.prepare && i.name === prepare) {
          const got = h.arena(c), expected = scratch(input, row, i.name === prepare ? 'prepare' : 'final');
          assertCapture(got, expected, checks, 'real helper exact packet/scratch and no premature register/FP/vector state');
          captures.push({kind: i.name === prepare ? 'prepare-helper' : 'real-helper', actual: got, expected, returned_status: status});
        }
        c.calls.push(call(i.name, args, status)); return status;
      }]));
      actual = {...material, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run, checks, captures};
    }
    const ram = h.ram(c), trace = [];
    if (row.prepare && material) trace.push(call(prepare, bound(c, material, row.prepare)));
    if (material) trace.push(call(name(c, row), argumentsFor(c, material, row), row.success ? 0 : 23));
    if (!row.success) trace.push(call('finish_exception_detection', finishArguments(row)));
    else writeRAM(ram, 0x5e30, fxImage(input).subarray(0, 464));
    observe(c, row.label, actual, input, entryImage(input, row), {helpers: trace, wantRAM: ram});
    cr2(c, row.label + ': only actual PF updates CR2', row.success ? undefined : row.linear); h.host(c, 'close');
  }
  assert.equal(genericRows.filter(r => r.success).length, 1);
  return {contexts: genericRows.length, compiles: genericRows.length, invocations: genericRows.length,
    pf: genericRows.filter(r => !r.success).length, note: 'application integer, x87, SIMD, xstate, masked and user-stack access seams; exact helper and complete source-state captures'};
}
