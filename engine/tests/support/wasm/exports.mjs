import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {join} from 'node:path';

// the closed declaration tables own the engine export set; fixture API subsets retain their own arity checks.
export function assertEngineExports(module, root) {
  const ownerPath = 'engine/src/abi/wasm/exports/mod.rs';
  const ownerText = readFileSync(join(root, ownerPath), 'utf8');
  const ownerList = ownerText.match(/\n\}\s*\n((?:mod [a-z0-9_]+;\s*\n)+)$/)?.[1];
  assert.ok(ownerList, 'closed ABI export owner list');
  const owners = [...ownerList.matchAll(/^mod (\w+);$/gm)]
    .map(([, name]) => `engine/src/abi/wasm/exports/${name}.rs`);
  const functions = owners.flatMap(path => {
    const text = readFileSync(join(root, path), 'utf8');
    const body = text.match(/^export_process_calls! \{\s*([\s\S]*?)\s*\}\s*$/)?.[1];
    assert.notEqual(body, undefined, `${path}: closed ABI declaration table`);
    const pattern = /([a-z0-9_]+)\(([^()]*)\);/g;
    const declarations = [...body.matchAll(pattern)];
    assert.equal(body.replace(pattern, '').trim(), '', `${path}: every declaration parsed`);
    assert.ok(declarations.length > 0, `${path}: nonempty export owner`);
    return declarations.map(([, name, parameters]) => {
      const argumentsList = parameters.split(',').map(value => value.trim()).filter(Boolean);
      for (const argument of argumentsList) {
        assert.match(argument, /^[a-z0-9_]+:\s*u32$/, `${path}/${name}: ABI parameter`);
      }
      return {name: `ring3_abi_v1_${name}`, kind: 'function', arity: argumentsList.length};
    });
  });
  assert.equal(new Set(functions.map(row => row.name)).size, functions.length, 'unique declared engine exports');
  const byName = (left, right) => left.name < right.name ? -1 : left.name > right.name ? 1 : 0;
  const expected = [{name: 'memory', kind: 'memory'}, ...functions.map(({name, kind}) => ({name, kind}))];
  assert.deepEqual(WebAssembly.Module.imports(module), [], 'engine has no JS imports');
  assert.deepEqual(WebAssembly.Module.exports(module).sort(byName), expected.sort(byName), 'exact declared engine exports');
  return {functions, sourcePaths: ['engine/tests/support/wasm/exports.mjs', ownerPath, ...owners]};
}
