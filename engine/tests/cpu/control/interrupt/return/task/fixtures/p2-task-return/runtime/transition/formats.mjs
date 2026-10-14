import {incomingRegisters, savedRAM, savedWordRAM} from './images.mjs';

export function formats({h, open, compile, observe, wantEntered, taskHelpers, cloneRAM, writeRAM}) {
  // Existing dword/dword tests cover all four bindings. These distinguish the independent formats
  // and CF/66CF without multiplying that unchanged route proof.
  const rows = [
    {currentWord: true, targetWord: true, code: 'cf', resident: false, entries: false},
    {currentWord: true, targetWord: true, code: '66cf', resident: true, entries: true},
    {currentWord: true, targetWord: false, code: 'cf', resident: true, entries: false},
    {currentWord: false, targetWord: true, code: '66cf', resident: false, entries: true},
  ];
  for (const selected of rows) {
    const {resident, entries, ...rowInput} = selected;
    if (rowInput.targetWord) {
      // Word-TSS upper GPR/FLAGS zeroing and null FS/GS are explicit Ring3 profile choices.
      rowInput.general = incomingRegisters.map(value => value & 0xffff); rowInput.flags = 0x0ed7;
    }
    rowInput.label = 'task formats ' + (rowInput.currentWord ? 'word' : 'dword') + '→' +
      (rowInput.targetWord ? 'word' : 'dword') + '/' + rowInput.code;
    const {c, input, row} = open(resident, entries, rowInput), material = compile(c, row.label, row.code, false);
    const want = wantEntered(input, row);
    const save = row.currentWord ? savedWordRAM : savedRAM;
    const wantRAM = save(cloneRAM, writeRAM, h.ram(c), row.current, row.code, row.oldRegisters);
    observe(c, 'real ' + row.code + ' saves current format and loads independent incoming format exactly',
      material, input, want, {wantRAM, helpers: taskHelpers(c, material, row)});
    h.host(c, 'close');
  }
  return {contexts: rows.length, compiles: rows.length, generated: rows.length, requests: 0};
}
