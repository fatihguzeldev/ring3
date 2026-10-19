import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {join} from 'node:path';

const detectionOwner = 'engine/src/abi/wasm/exports/detection.rs';
const cpuOwner = 'engine/src/abi/wasm/exports/cpu.rs';
const tokens = value => value.match(/[a-zA-Z_][a-zA-Z_0-9]*|"(?:\\.|[^"\\])*"|[^\s]/g);
const cpuPair = `#[allow(unsafe_code, clippy::too_many_arguments)]
#[unsafe(export_name = "ring3_abi_v1_x87_resident_pair")]
pub extern "C" fn x87_resident_pair(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    first_pc: u32,
    address0: u32,
    address1: u32,
    flags: u32,
    max_steps: u32,
) -> i64 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::x87_resident_pair(
            key_low, key_high, id_low, id_high, first_pc, address0, address1, flags, max_steps,
        )
    }
}`;
const cpuGroup = `#[allow(unsafe_code, clippy::too_many_arguments)]
#[unsafe(export_name = "ring3_abi_v1_x87_resident_group")]
pub extern "C" fn x87_resident_group(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    ordinal: u32,
    address0: u32,
    address1: u32,
    address2: u32,
    address3: u32,
    address4: u32,
    address5: u32,
    address6: u32,
    address7: u32,
    flags: u32,
    max_steps: u32,
) -> i64 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::x87_resident_group(
            key_low, key_high, id_low, id_high, ordinal, address0, address1, address2, address3,
            address4, address5, address6, address7, flags, max_steps,
        )
    }
}`;
const detectionMacro = `macro_rules! export_detection_guards {
    ($($name:ident($($argument:ident: $kind:ty),*);)*) => {
        $(
            #[allow(unsafe_code, clippy::too_many_arguments)]
            #[unsafe(export_name = concat!("ring3_abi_v1_", stringify!($name)))]
            pub extern "C" fn $name($($argument: $kind),*) -> i64 {
                {
                    #![forbid(unsafe_code)]
                    crate::process::wasm::$name($($argument),*)
                }
            }
        )*
    };
}`;
const detectionParameters = new Map([
  ['guard_detect', ['low: u32', 'high: u32', 'generation: u32', 'state: u32', 'exit: u32', 'cancel: u32']],
  ['guard_detect_resident', ['low: u32', 'high: u32', 'id_low: u32', 'id_high: u32', 'state: u32', 'exit: u32', 'cancel: u32']],
  ['finish_exception_detection', ['epoch: i64', 'reason: u32', 'detail: u32', 'address: u32', 'access: u32', 'length: u32', 'debug_origin: u32', 'restart_pc: u32', 'restart_retired: u32', 'restart_flags: u32', 'restart_shadow: u32', 'restart_work: u32']],
]);

function declarations(body, path, checkParameters) {
  const pattern = /([a-z0-9_]+)\(([^()]*)\);/g;
  const rows = [...body.matchAll(pattern)];
  assert.equal(body.replace(pattern, '').trim(), '', `${path}: every declaration parsed`);
  assert.ok(rows.length > 0, `${path}: nonempty export table`);
  return rows.map(([, name, parameters]) => {
    const argumentsList = parameters.split(',').map(value => value.trim()).filter(Boolean);
    checkParameters(name, argumentsList, parameters);
    return {name: `ring3_abi_v1_${name}`, kind: 'function', arity: argumentsList.length};
  });
}

function detectionDeclarations(text, path) {
  const tables = text.match(/^([\s\S]*?)\nexport_detection_guards! \{\s*([^{}]*)\s*\}\s*export_process_calls! \{\s*([^{}]*)\s*\}\s*$/);
  assert.ok(tables, `${path}: closed detection declaration tables`);
  assert.deepEqual(tokens(tables[1]), tokens(detectionMacro), `${path}: declared i64 guard macro`);
  const check = (name, argumentsList, parameters) => {
    assert.ok(detectionParameters.has(name), `${path}/${name}: declared detection export`);
    if (name !== 'finish_exception_detection') {
      assert.ok(parameters.split(',').every(value => value.trim()), `${path}/${name}: guard argument separators`);
    }
    assert.deepEqual(argumentsList, detectionParameters.get(name), `${path}/${name}: detection parameters`);
  };
  const guards = declarations(tables[2], path, check);
  const finish = declarations(tables[3], path, check);
  assert.deepEqual(guards.map(row => row.name), ['ring3_abi_v1_guard_detect', 'ring3_abi_v1_guard_detect_resident'], `${path}: exact guard table`);
  assert.deepEqual(finish.map(row => row.name), ['ring3_abi_v1_finish_exception_detection'], `${path}: exact finish table`);
  return [...guards, ...finish];
}

// the closed declaration tables own the engine export set; fixture API subsets retain their own arity checks.
export function assertEngineExports(module, root) {
  assert.deepEqual(WebAssembly.Module.imports(module), [], 'engine has no JS imports');
  const ownerPath = 'engine/src/abi/wasm/exports/mod.rs';
  const ownerText = readFileSync(join(root, ownerPath), 'utf8');
  const ownerList = ownerText.match(/\n\}\s*\n((?:(?:mod [a-z0-9_]+;|#\[cfg\(feature = "observability"\)\]\s*\nmod observability;)\s*\n)+)$/)?.[1];
  assert.ok(ownerList, 'closed ABI export owner list');
  const conditional = '#[cfg(feature = "observability")]\nmod observability;';
  assert.ok(ownerList.split(conditional).length <= 2, 'unique conditional observability owner');
  const owners = [...ownerList.replace(conditional, '').matchAll(/^mod (\w+);$/gm)]
    .map(([, name]) => `engine/src/abi/wasm/exports/${name}.rs`);
  const sourcePaths = [];
  function declarationsAt(path) {
    assert.ok(!sourcePaths.includes(path), `${path}: unique export owner`);
    sourcePaths.push(path);
    let text = readFileSync(join(root, path), 'utf8');
    if (path === detectionOwner) return detectionDeclarations(text, path);
    const extra = [];
    if (path === cpuOwner) {
      const parts = text.match(/^(export_process_calls!\s*\{[^{}]*\})\s*([\s\S]*)$/);
      assert.ok(parts, `${path}: closed CPU declaration table`);
      assert.deepEqual(tokens(parts[2]), tokens(cpuPair + '\n' + cpuGroup), `${path}: declared i64 pair/group exports`);
      text = parts[1];
      extra.push({name: 'ring3_abi_v1_x87_resident_pair', kind: 'function', arity: 9});
      extra.push({name: 'ring3_abi_v1_x87_resident_group', kind: 'function', arity: 15});
    }
    const table = text.match(/^((?:mod [a-z0-9_]+;\s*\n)*)\s*export_process_calls! \{\s*([^{}]*)\s*\}\s*$/);
    const body = table?.[2];
    assert.notEqual(body, undefined, `${path}: closed ABI declaration table`);
    const functions = declarations(body, path, (name, argumentsList) => {
      if (path === cpuOwner && name === 'cpu_read32_view') {
        assert.deepEqual(argumentsList, ['epoch: i64'], `${path}/${name}: view parameters`);
        return;
      }
      for (const argument of argumentsList) {
        assert.match(argument, /^[a-z0-9_]+:\s*u32$/, `${path}/${name}: ABI parameter`);
      }
    });
    const children = [...table[1].matchAll(/^mod (\w+);$/gm)]
      .flatMap(([, name]) => declarationsAt(`${path.slice(0, -3)}/${name}.rs`));
    return [...functions, ...children, ...extra];
  }
  const functions = owners.flatMap(declarationsAt);
  assert.equal(new Set(functions.map(row => row.name)).size, functions.length, 'unique declared engine exports');
  const byName = (left, right) => left.name < right.name ? -1 : left.name > right.name ? 1 : 0;
  const expected = [{name: 'memory', kind: 'memory'}, ...functions.map(({name, kind}) => ({name, kind}))];
  assert.deepEqual(WebAssembly.Module.exports(module).sort(byName), expected.sort(byName), 'exact declared engine exports');
  return {functions, sourcePaths: ['engine/tests/support/wasm/exports.mjs', ownerPath, ...sourcePaths]};
}
