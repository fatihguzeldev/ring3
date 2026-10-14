import {savedRAM} from './images.mjs';

export function work({h, exit, registers, observed, open, compile, observe, wantEntered,
  taskHelpers, call, cloneRAM, writeRAM}) {
  const sourceRegisters = [...registers];
  sourceRegisters[1] = 1; sourceRegisters[6] = 0x9000; sourceRegisters[7] = 0x9001;
  const {c, input, row} = open(false, false, {label: 'completed REP MOVSB work precedes task IRET',
    code: 'f3a4cf', nextPC: 0x1003, sourceFlags: 0x254ad7, sourceRegisters, repeat: true});
  const material = compile(c, row.label, row.code, false, ['cpu_read8', 'cpu_store8']);
  const want = wantEntered(input, row); exit(want, 3, 2, 6); want.writeUInt32LE(2, 4604);
  // The outgoing save observes the completed string's registers, not its entry snapshot.
  const savedRegisters = [...row.oldRegisters];
  savedRegisters[1] = 0; savedRegisters[6] = 0x9001; savedRegisters[7] = 0x9002;
  const wantRAM = savedRAM(cloneRAM, writeRAM, h.ram(c), row.current, row.code, savedRegisters,
    {nextPC: 0x1003, flags: 0x240ad7});
  writeRAM(wantRAM, 0x9001, Buffer.from([0x7e]));
  observe(c, 'one REP element and completed IRET retain two work units and exact outgoing progress',
    material, input, want, {budget: 2, wantRAM,
      helpers: [call('cpu_read8', [0x9000]), call('cpu_store8', [0x9001, 0x7e]), ...taskHelpers(c, material, row)]});
  h.host(c, 'close');
  return {contexts: 1, compiles: 1, generated: 1, requests: 0};
}
