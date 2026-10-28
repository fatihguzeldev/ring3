import {spawnSync} from 'node:child_process';
import {copyFileSync, mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {dirname, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const output = resolve(root, process.argv[2] ?? 'artifacts/v86-experiment/ring3_engine.wasm');
const args = ['rustc', '--locked', '-p', 'ring3-engine', '--lib', '--release',
  '--target', 'wasm32-unknown-unknown', '--features', 'external-cpu', '--',
  '-C', 'link-arg=--import-memory',
  '-C', 'link-arg=--export-memory',
  '-C', 'link-arg=--global-base=536870912',
  '-C', 'link-arg=--initial-memory=805306368',
  '-C', 'link-arg=--export=__stack_pointer',
  '-C', 'link-arg=--export=__heap_base',
  '-C', 'link-arg=--export=__data_end'];
const result = spawnSync('cargo', args, {cwd: root, stdio: 'inherit'});
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);
const built = resolve(root, 'target/wasm32-unknown-unknown/release/ring3_engine.wasm');
const bytes = readFileSync(built), module = new WebAssembly.Module(bytes);
const imports = WebAssembly.Module.imports(module);
if (!imports.some(item => item.module === 'env' && item.name === 'memory' && item.kind === 'memory'))
  throw new Error('experimental engine must import the v86 memory');
mkdirSync(dirname(output), {recursive: true});
copyFileSync(built, output);
const manifest = {
  command: ['cargo', ...args],
  memory: {sharedWith: 'v86', globalBase: 536870912, minimumBytes: 805306368},
  sha256: createHash('sha256').update(bytes).digest('hex'),
  bytes: bytes.length,
  imports,
};
writeFileSync(output + '.json', JSON.stringify(manifest, null, 2) + '\n');
console.log(JSON.stringify({output, ...manifest}));
