import assert from 'node:assert/strict';
import {savedRAM} from './images.mjs';

export function protocol({h, record, exit, observed, open, compile, observe, wantEntered,
  taskHelpers, call, cloneRAM, writeRAM}) {
  const cases = [
    {label: 'committed GP requires error code zero', fields: [0, 11, 0x48, 0, 0, 0]},
    {label: 'committed SS requires a nonnull selector', fields: [0, 15, 0, 0, 0, 0]},
    {label: 'precommit phase rejects GP', fields: [4, 11, 0, 0, 0, 0]},
    {label: 'precommit NP requires GDT selector', fields: [4, 14, 0x4c, 0, 0, 0]},
    {label: 'raw task write fault cannot report a whole TSS span', fields: [1, 0, 1, 0x5000, 2, 104]},
    {label: 'unexpected transport precedes stale packet header', badHeader: true, transport: 9, detail: 3},
    {label: 'posthelper State size is checked before retirement', state: [8, 52]},
    {label: 'posthelper State metadata is checked before retirement', state: [12, 2]},
  ];
  for (const selected of cases) {
    const {c, input, row} = open(false, false, {label: selected.label});
    const material = compile(c, selected.label, row.code, false);
    const want = wantEntered(input, row);
    const wantRAM = savedRAM(cloneRAM, writeRAM, h.ram(c), row.current, row.code, row.oldRegisters);
    function alter(bytes) {
      if (selected.fields) bytes.set(record('R3MH', 40, selected.fields, 14), 100);
      if (selected.badHeader) bytes.writeUInt16LE(13, 104);
      if (selected.state) bytes.writeUInt32LE(selected.state[1], selected.state[0]);
    }
    alter(want);
    exit(want, 7, 0, 6);
    want.writeUInt32LE(selected.detail ?? 2, 80);
    if (material) {
      const module = new WebAssembly.Module(new Uint8Array(c.memory.buffer, material.pointer, material.length));
      const imports = Object.fromEntries(WebAssembly.Module.imports(module)
        .filter(item => item.kind === 'function').map(item => [item.name, (...args) => {
          const status = c.api[item.name](...args);
          c.calls.push(call(item.name, args, status));
          if (item.name === 'cpu_task_return') {
            const bytes = h.arena(c);
            assert.deepEqual(bytes.subarray(100, 140), record('R3MH', 40, [], 14));
            assert.equal(bytes.readUInt32LE(48), 0x2000);
            alter(bytes);
            h.put(c, 0, bytes);
            return selected.transport ?? status;
          }
          return status;
        }]));
      const wrapped = {...material, run: new WebAssembly.Instance(module,
        {env: {memory: c.memory}, ring3: imports}).exports.run};
      observe(c, selected.label + ' preserves committed incoming state and outgoing RAM save',
        wrapped, input, want, {wantRAM, helpers: taskHelpers(c, material, row)});
    } else {
      h.skip(c, selected.label, 'task transition module');
    }
    h.host(c, 'close');
  }
  return {contexts: cases.length, compiles: cases.length, generated: cases.length};
}
