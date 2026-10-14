import {open, host, packet, PC} from '../../../../far/support/transport.mjs';

// independent literal observations for the fixed protected32 process environment.
const OLD_FLAGS = 0x250ed7;

export function checkDefaultQueries(engine, {compile, input, exit, check}) {
  // a fresh process exposes complete user-readable virtual system observations;
  // these instructions never need guest-memory GDT/IDT/TSS backing.
  for (const resident of [false, true]) for (const entries of [false, true]) {
    for (const spec of [
      {name: 'SLDT', hex: '0f00c0', eax: 0},
      {name: 'STR', hex: '0f00c8', eax: 0x28},
      {name: 'SMSW', hex: '0f01e0', eax: 0x12340033},
      {name: 'SGDT', hex: '0f010500500000', bytes: 'ff070000ffff'},
      {name: 'SIDT', hex: '0f010d00500000', bytes: 'ff070010ffff'},
    ]) {
      const c = open(engine, resident, entries), code = Buffer.from(spec.hex, 'hex'); compile(c, code);
      const b = input(c), want = Buffer.from(b), calls = [], writes = [];
      want.writeUInt32LE(PC + code.length, 48); want.writeUInt32LE(OLD_FLAGS & ~0x10000, 52);
      if (spec.eax !== undefined) want.writeUInt32LE(spec.eax, 16);
      if (spec.bytes) {
        const bytes = Buffer.from(spec.bytes, 'hex'), helper = resident ? 'cpu_write_resident' : 'cpu_write';
        want.set(bytes, 140); want.set(packet('R3MH', 40, [0, 0, 0, 0, 0, 6], 7), 100);
        calls.push([helper, [...c.binding, 0x5000, 6, 1]]); writes.push({address: 0x5000, bytes});
      }
      exit(want, 1, 1, 0, 0, 0, 0, spec.bytes || entries ? 6 : 1);
      check(c, `default-process-${spec.name}-without-system-table-backing`, b, want, calls, {writes});
      host(c, 'close');
    }
  }
}
