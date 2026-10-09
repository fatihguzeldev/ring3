import assert from 'node:assert/strict';
import {mkdirSync, writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {initialHex} from '../resident-phases/plan.mjs';
import {sorted} from '../p2-pe32-sort/oracle.mjs';

export const SIZE = 4364;
export const CODE = 0x1000;
export const DATA = 0x3000;
export const BUDGET = 1283;
export const CODE_HEX = '31c0b90001000089fec1c005330683c6044975f5';
export const CUTS = [0, 2, 7, 9, 12, 14, 17, 18, 20];

function record(magic, length, fields, version = 1) {
  const bytes = Buffer.alloc(length);
  bytes.write(magic); bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(length, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4));
  return bytes;
}

export function seedHex() {
  const bytes = Buffer.alloc(140);
  bytes.set(record('R3ST', 56, [0x11223344, 0x22334455, 0x33445566, 0x44556677,
    0x55667788, 0x66778899, 0x778899aa, DATA, CODE, 2]));
  bytes.set(record('R3EX', 40, [3, 0, 0, 0, 0, 0], 3), 56);
  bytes.set(record('R3MH', 40, [0, 0xdecafbad, 0, 0, 0, 0]), 100);
  return bytes.toString('hex');
}

export function dataHex() {
  const bytes = Buffer.alloc(1024);
  sorted().forEach((value, index) => bytes.writeUInt32LE(value, index * 4));
  return bytes.toString('hex');
}

export function makePlan() {
  let nextAction = 0, nextContext = 0;
  const processes = [];
  for (let index = 0; index < 3; index++) {
    const contexts = [], actions = [];
    const add = (context, kind, fields = {}) => actions.push({id: nextAction++, context: context?.id ?? null, kind, ...fields});
    const input = (context, offset, hex) => add(context, 'input', {offset, hex});
    const host = (context, name, args) => add(context, 'host', {name, args});
    const seed = context => input(context, 0, seedHex());
    const generated = (context, channel, budget, phase, fields = {}) => add(context, 'call', {
      channel, budget, phase, timed: false, expected_status: 0, expected_retired: budget, ...fields,
    });
    function install(context, generation, dispatcher = false) {
      const request = Buffer.alloc(context.blocks * 8);
      const cuts = context.blocks === 1 ? [0, 20] : CUTS;
      for (let i = 0; i < context.blocks; i++) {
        request.writeUInt32LE(CODE + cuts[i], i * 8);
        request.writeUInt32LE(cuts[i + 1] - cuts[i], i * 8 + 4);
      }
      input(context, 140, request.toString('hex'));
      host(context, 'compile_resident', [context.blocks]);
      add(context, 'module', {target: 'child', generation, file: `${context.id}-child-${generation}.wasm`});
      add(context, 'table', {slot: 0});
      host(context, 'acknowledge_resident_installation', ['key_low', 'key_high', 'unit_low', 'unit_high', 0]);
      if (dispatcher) {
        host(context, 'dispatcher_module', ['key_low', 'key_high']);
        add(context, 'module', {target: 'dispatcher', generation: 0, file: `${context.id}-dispatcher.wasm`});
      }
    }
    function open(blocks, role) {
      const context = {id: ++nextContext, blocks, role, pages: 16, key: [0x10000 + nextContext, 0x43484b53]};
      contexts.push(context); add(context, 'open');
      input(context, 0, initialHex());
      host(context, 'map', [CODE, 1, 7]);
      input(context, 140, CODE_HEX); host(context, 'upload', [CODE, 20]);
      if (role !== 'stale') host(context, 'protect', [CODE, 1, 5]);
      host(context, 'map', [DATA, 1, 3]);
      input(context, 140, dataHex()); host(context, 'upload', [DATA, 1024]);
      host(context, 'protect', [DATA, 1, 1]);
      install(context, 0, true); seed(context);
      add(context, 'data', {address: DATA, count: 1024, phase: 'initial', file: `${context.id}-data-initial.bin`});
      return context;
    }
    const warm = [open(1, 'warm'), open(8, 'warm')];
    for (const context of warm) add(context, 'guard', {count: 1024, phase: 'warm', timed: false});
    const arms = [
      {context: warm[0], channel: 'direct'}, {context: warm[1], channel: 'direct'},
      {context: warm[0], channel: 'dispatch'}, {context: warm[1], channel: 'dispatch'},
    ];
    function quartet(round, timed) {
      for (let position = 0; position < 4; position++) {
        const arm = arms[(round + index + position) % 4];
        seed(arm.context);
        generated(arm.context, arm.channel, BUDGET, timed ? 'sample' : 'warm', {
          timed, round, position,
        });
      }
    }
    for (let round = 0; round < 100; round++) quartet(round, false);
    for (let round = 0; round < 32; round++) {
      add(null, 'gc', {round});
      const guards = () => {
        for (let position = 0; position < 2; position++) add(warm[(round + index + position) % 2], 'guard', {
          count: 1024, phase: 'sample', timed: true, round, position,
        });
      };
      if (round % 2 === 0) guards();
      quartet(round, true);
      if (round % 2 === 1) guards();
    }
    for (const context of warm) {
      seed(context);
      for (const budget of [3, 1, 1, 3]) generated(context, 'direct', budget, 'partial');
      host(context, 'protect', [DATA, 1, 0]); seed(context);
      generated(context, 'dispatch', BUDGET, 'fault', {expected_retired: 4});
      generated(context, 'dispatch', BUDGET, 'retry', {expected_retired: 0});
      host(context, 'protect', [DATA, 1, 1]);
      generated(context, 'dispatch', 1279, 'repair');
      add(context, 'data', {address: DATA, count: 1024, phase: 'final', file: `${context.id}-data-final.bin`});
      add(context, 'close');
    }
    if (index === 2) for (const blocks of [1, 8]) {
      const context = open(blocks, 'stale');
      seed(context); host(context, 'write8', [CODE, 0x31]);
      for (const channel of ['direct', 'dispatch']) generated(context, channel, BUDGET, 'stale', {
        expected_status: 4, expected_retired: 0,
      });
      add(context, 'table', {slot: 0, clear: true});
      host(context, 'retire_stale_resident', ['key_low', 'key_high', 'unit_low', 'unit_high']);
      install(context, 1); seed(context);
      generated(context, 'dispatch', BUDGET, 'replacement');
      add(context, 'data', {address: DATA, count: 1024, phase: 'final', file: `${context.id}-data-final.bin`});
      add(context, 'close');
    }
    processes.push({index, contexts, actions});
  }
  const actions = processes.flatMap(p => p.actions), contexts = processes.flatMap(p => p.contexts);
  const calls = actions.filter(a => a.kind === 'call');
  const counts = {
    processes: 3, contexts: contexts.length,
    generated_calls: calls.length, positive_calls: calls.filter(a => a.expected_retired > 0).length,
    zero_retirement_calls: calls.filter(a => a.expected_retired === 0).length,
    expected_instructions: calls.reduce((n, a) => n + a.expected_retired, 0),
    guard_calls: actions.filter(a => a.kind === 'guard').reduce((n, a) => n + a.count, 0),
    diagnostic_read32_calls: actions.filter(a => a.kind === 'data').reduce((n, a) => n + a.count, 0),
    child_modules: actions.filter(a => a.kind === 'module' && a.target === 'child').length,
    dispatcher_modules: actions.filter(a => a.kind === 'module' && a.target === 'dispatcher').length,
    timed_samples: actions.filter(a => ['call', 'guard'].includes(a.kind) && a.timed).length,
    explicit_gc_calls: actions.filter(a => a.kind === 'gc').length,
    actions: actions.length,
    frames: actions.reduce((n, a) => n + (a.kind === 'gc' ? 0 : ['open', 'close'].includes(a.kind) ? 1 : 2), 0),
    host_api_calls: actions.filter(a => a.kind === 'host' || ['open', 'close'].includes(a.kind)).length + contexts.length,
    module_files: actions.filter(a => a.kind === 'module').length,
    data_files: actions.filter(a => a.kind === 'data').length,
    // plan, result, manifest, arena, journal and checkpoints per process; code/data inputs are in plan.
    process_physical_files: actions.filter(a => ['module', 'data'].includes(a.kind)).length + 18,
    journal_rows: actions.length + actions.reduce((n, a) => n + (a.kind === 'gc' ? 0 : ['open', 'close'].includes(a.kind) ? 1 : 2), 0)
      + actions.filter(a => ['module', 'data'].includes(a.kind)).length,
    checkpoint_rows: actions.filter(a => ['call', 'guard', 'data', 'close'].includes(a.kind)).length + 3,
  };
  assert.equal(counts.generated_calls, 1632); assert.equal(counts.positive_calls, 1622);
  assert.equal(counts.zero_retirement_calls, 10); assert.equal(counts.expected_instructions, 2042584);
  assert.equal(counts.guard_calls, 202752); assert.equal(counts.contexts, 8);
  assert.equal(counts.diagnostic_read32_calls, 16384); assert.equal(counts.child_modules, 10);
  assert.equal(counts.dispatcher_modules, 8); assert.equal(counts.timed_samples, 576);
  return {schema_version: 1, layout: {arena_bytes: SIZE, code: CODE, data: DATA, budget: BUDGET},
    code_hex: CODE_HEX, data_hex: dataHex(), cuts: CUTS, counts, processes};
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  assert.equal(process.argv[2], '--plan');
  const output = resolve(process.argv[3]); mkdirSync(output, {recursive: true});
  const plan = makePlan();
  writeFileSync(resolve(output, 'plan.json'), JSON.stringify(plan, null, 2) + '\n', {flag: 'wx'});
  console.log(JSON.stringify(plan.counts));
}
