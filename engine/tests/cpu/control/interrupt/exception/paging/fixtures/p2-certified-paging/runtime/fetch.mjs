import {PC, FLAGS, record, seed, call, bound, assertCapture} from './oracle.mjs';

export const fetchRows = [
  {label: 'source-free unmapped fetch carries Execute fault20 and R3EX13', pc: PC, linear: PC, length: 1, error: 20},
  {label: 'consumed prefix retains identity before cross-page Execute fault20', resident: true, pc: 0x1fff, linear: 0x2000, length: 2, prefix: true, error: 20},
  {label: 'logical CS span wins over captured cross-page Execute denial', resident: true, pc: 0x1fff, length: 2, prefix: true, cs: true},
  {label: 'source-free mapping repair returns NeedCode without retirement', pc: PC, length: 1, repair: true},
  {label: 'mapped nonexecutable page produces protection Execute fault21', pc: PC, linear: PC, length: 1, denied: true, error: 21},
];

function configure(h, c, row) {
  c.capturesExceptions = true;
  if (row.denied) h.host(c, 'protect', [PC, 1, 3]);
  else {
    const page = row.prefix ? 0x2000 : PC;
    h.host(c, 'unmap', [page, 1]); c.pages = c.pages.filter(mapped => mapped !== page);
  }
}
function inputImage(h, row) {
  const b = seed(h, {}); b.writeUInt32LE(row.pc, 48);
  if (row.cs) {
    b.writeUInt16LE(2, 4528);
    [0xf33fffff, 0xf2b01fff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0]
      .forEach((v, i) => b.writeUInt32LE(v >>> 0, 4524 + [28, 32, 36, 40, 44, 56][i]));
  }
  return b;
}
function packet(row) {
  if (row.repair) return record('R3MH', 40, [0, 0, 0, 0, 0, row.length], 7);
  return record('R3MH', 40, [9, 0, row.error, row.linear, 3, row.length], 15);
}
function stage(input, row) {const b = Buffer.from(input); if (!row.cs) b.set(packet(row), 100); return b;}
function source(input, row) {
  const b = stage(input, row), reason = row.cs ? 11 : row.repair ? 3 : 23;
  b.set(record('R3EX', 40, [reason, 0, row.cs || row.repair ? 0 : row.error,
    row.cs || row.repair ? 0 : row.linear, row.cs || row.repair ? 0 : 3, row.cs || row.repair ? 0 : row.length], reason === 23 ? 13 : 6), 56);
  return b;
}
function entered(input, row) {return source(input, row);}
function finish(row) {
  return [2n, row.cs ? 11 : 23, row.cs ? 0 : row.error,
    row.cs ? 0 : row.linear, row.cs ? 0 : 3, row.cs ? 0 : row.length, 0, 0, 0, 0, 0, 0];
}

export function runFetch({h, compile, observe, cr2, observed, writeRAM}) {
  for (const row of fetchRows) {
    observed.contexts++;
    const c = h.open(!!row.resident, true, row.label, [[PC, 7], [0x2000, 7], [0x3000, 7], [0x7000, 3]]); c.diagnosticRAM = true;
    for (const page of [PC, 0x2000, 0x3000, 0x7000]) h.upload(c, page, Buffer.alloc(4096, 0x6b));
    h.upload(c, row.pc, Buffer.from(row.prefix ? '66b80df0ebfe' : 'b80df0ad0bebfe', 'hex'));
    configure(h, c, row); cr2(c, row.label + ': initial CR2 Unknown', undefined);
    // strict entry compilation stays strict; a real ordinary code miss authorizes stopped capture.
    const branch = Buffer.alloc(5); branch[0] = 0xe9; branch.writeUInt32LE((row.pc - 0x3005) >>> 0, 1);
    h.upload(c, 0x3000, branch);
    const start = seed(h, {}); start.writeUInt32LE(0x3000, 48); h.put(c, 0, start);
    const starter = compile(c, row.label + ': admitted JMP starter', [[0x3000, 5]], ['finish_exception_detection']);
    const miss = Buffer.from(start); miss.writeUInt32LE(row.pc, 48); miss.writeUInt32LE(FLAGS, 52); miss.writeUInt32LE(0, 12);
    miss.set(record('R3EX', 40, [3, 1, 0, 0, 0, 0], 1), 56);
    const missed = observe(c, row.label + ': real JMP reaches uncaptured entry', starter, start, miss, {budget: 2, helpers: []});
    if (!missed?.passed) {h.skip(c, row.label + ': captured terminal', 'real NeedCode starter'); h.skip(c, row.label, 'stopped source admission'); h.host(c, 'close'); continue;}
    // a fresh host-supplied invocation input independently tests pre-begin RF/shadow after capture.
    const captureInput = seed(h, {}); captureInput.writeUInt32LE(row.pc, 48);
    captureInput.set(record('R3EX', 40, [3, 1, 0, 0, 0, 0], 1), 56); h.put(c, 0, captureInput);
    const helper = 'cpu_fetch_fault' + (c.resident ? '_resident' : ''), helpers = [helper, 'finish_exception_detection'];
    const material = compile(c, row.label + ': captured terminal', [], helpers, {stopped: true});
    if (row.repair) {
      h.host(c, 'map', [PC, 1, 7]); c.pages.push(PC); c.pages.sort((a, b) => a - b);
      h.upload(c, PC, Buffer.from('b80df0ad0bebfe', 'hex'));
    }
    const input = inputImage(h, row); let actual = material;
    if (material) {
      const module = new WebAssembly.Module(new Uint8Array(c.memory.buffer, material.pointer, material.length)), checks = [], captures = [];
      const ring3 = Object.fromEntries(WebAssembly.Module.imports(module).filter(i => i.kind === 'function').map(i => [i.name, (...args) => {
        if (i.name === 'finish_exception_detection') {
          const got = h.arena(c), expected = source(input, row); assertCapture(got, expected, checks, 'complete pre-begin fetch terminal before real finish');
          observed.captures++; captures.push({kind: 'pre-finish', actual: got, expected});
        }
        const status = c.api[i.name](...args);
        if (i.name === helper) {
          const got = h.arena(c), expected = stage(input, row); assertCapture(got, expected, checks, 'live native Execute preflight and packet only');
          captures.push({kind: 'real-fetch-helper', actual: got, expected, returned_status: status});
        }
        c.calls.push(call(i.name, args, status)); return status;
      }]));
      actual = {...material, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3}).exports.run, checks, captures};
    }
    const ram = h.ram(c), trace = [];
    if (material && !row.cs) trace.push(call(helper, bound(c, material, [row.pc, row.length]), row.repair ? 0 : 23));
    if (!row.repair) trace.push(call('finish_exception_detection', finish(row)));
    const result = observe(c, row.label, actual, input, entered(input, row), {helpers: trace, wantRAM: ram});
    cr2(c, row.label + ': actual Execute fault only', row.cs || row.repair ? undefined : row.linear);
    if (row.repair) {
      if (!result?.passed) h.skip(c, 'repaired fetch material executes the real non-idempotent MOV', 'NeedCode repair completed');
      else {
        const retry = entered(input, row); h.put(c, 0, retry);
        const repaired = compile(c, 'repaired source capture consumes actual MOV bytes', [[PC, 7]], ['finish_exception_detection']);
        const want = Buffer.from(retry); want.writeUInt32LE(0x0badf00d, 16); want.writeUInt32LE(PC + 5, 48);
        want.writeUInt32LE(FLAGS, 52); want.writeUInt32LE(0, 12); want.set(record('R3EX', 40, [1, 1, 0, 0, 0, 0], 1), 56);
        observe(c, 'repaired fetch material executes the real non-idempotent MOV', repaired, retry, want, {helpers: []});
      }
    }
    h.host(c, 'close');
  }
  return {contexts: fetchRows.length, compiles: 2 * fetchRows.length + 1, invocations: 2 * fetchRows.length + 1,
    note: 'real JMP/NeedCode starters authorize stopped capture; live application execute permissions produce exact R3EX13/error20 or21; logical CS first, repair preserves pre-begin RF/shadow'};
}
