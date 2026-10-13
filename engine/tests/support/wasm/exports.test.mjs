import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import test from 'node:test';
import {assertEngineExports} from './exports.mjs';

function workspace(body, child, check) {
  const root = mkdtempSync(join(tmpdir(), 'ring3-export-tables-'));
  const dir = join(root, 'engine/src/abi/wasm/exports');
  mkdirSync(join(dir, 'memory'), {recursive: true});
  writeFileSync(join(dir, 'mod.rs'), 'macro_rules! export_process_calls {\n}\nmod memory;\n');
  writeFileSync(join(dir, 'memory.rs'), body);
  writeFileSync(join(dir, 'memory/cpu_write.rs'), child);
  try { check(root); } finally { rmSync(root, {recursive: true, force: true}); }
}

function moduleWith(names) {
  const section = (id, bytes) => [id, bytes.length, ...bytes];
  const name = value => [value.length, ...Buffer.from(value)];
  return new WebAssembly.Module(Uint8Array.from([
    0, 97, 115, 109, 1, 0, 0, 0,
    ...section(1, [1, 0x60, 0, 0]),
    ...section(3, [names.length, ...names.map(() => 0)]),
    ...section(5, [1, 0, 1]),
    ...section(7, [names.length + 1, ...name('memory'), 2, 0,
      ...names.flatMap((value, index) => [...name(value), 0, index])]),
    ...section(10, [names.length, ...names.flatMap(() => [2, 0, 0x0b])]),
  ]));
}

const parent = 'mod cpu_write;\n\nexport_process_calls! { read32(address: u32); }\n';
const child = 'export_process_calls! { cpu_store32(address: u32, value: u32); }\n';
const module = moduleWith(['ring3_abi_v1_read32', 'ring3_abi_v1_cpu_store32']);

test('declared child tables retain exact exports, arities and source pins', () => {
  workspace(parent, child, root => {
    const result = assertEngineExports(module, root);
    assert.deepEqual(result.functions.map(row => [row.name, row.arity]), [
      ['ring3_abi_v1_read32', 1], ['ring3_abi_v1_cpu_store32', 2],
    ]);
    assert.ok(result.sourcePaths.includes('engine/src/abi/wasm/exports/memory/cpu_write.rs'));
    assert.throws(() => assertEngineExports(moduleWith(['ring3_abi_v1_read32']), root), /exact declared engine exports/);
  });
});

test('unknown source tokens remain a closed-table error', () => {
  workspace(parent + 'fn hidden() {}\n', child, root => {
    assert.throws(() => assertEngineExports(module, root), /closed ABI declaration table/);
  });
});

test('duplicate child declarations and exported names remain errors', () => {
  workspace('mod cpu_write;\n' + parent, child, root => {
    assert.throws(() => assertEngineExports(module, root), /unique export owner/);
  });
  workspace(parent, 'export_process_calls! { read32(address: u32); }\n', root => {
    assert.throws(() => assertEngineExports(module, root), /unique declared engine exports/);
  });
});
