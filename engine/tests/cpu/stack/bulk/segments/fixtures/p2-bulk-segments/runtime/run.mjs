import {harness, u32} from '../../../../../support/segments.mjs';

const rows = [], normal = limit => u32(0xf2300000 + limit), expand = limit => u32(0xf2700000 + limit);
for (const width of [2, 4]) {
  const prefix = width === 2 ? [0x66] : [], ss = [7, 0, 0, 0, 0, width];
  const push = {width, action: 'push', code: [...prefix, 0x60]};
  const pop = {width, action: 'pop', code: [...prefix, 0x61]};
  const lastPush = 0x8080 - 8 * width, lastRead = 0x8020 + 8 * width - 1;
  rows.push(
    {...push, name: `push${width}/flat`},
    {...push, name: `push${width}/last-write-rejected`, cache: expand(lastPush), fault: ss},
    {...push, name: `push${width}/last-write-exact`, cache: expand(lastPush - 1)},
    {...push, name: `push${width}/wrap-between-slots`, sp: 4 * width},
    {...push, name: `push${width}/individual-span-overflow`, sp: 1, fault: ss},
    {...push, name: `push${width}/later-page-write-failure`, sp: 0x8000 + 2 * width,
      deny: [0x7000, 1], fault: [1, 0, 2, 0x8000 - width, 2, width]},
    {...push, name: `push${width}/code-store-retirement`, sp: 0x1080, smc: true},
    {...pop, name: `pop${width}/flat`, sp: 0x8020},
    {...pop, name: `pop${width}/last-read-full-width`, sp: 0x8020, cache: normal(lastRead - 1), fault: ss},
    {...pop, name: `pop${width}/last-read-exact`, sp: 0x8020, cache: normal(lastRead)},
    {...pop, name: `pop${width}/wrap-between-slots`, sp: u32(0x100000000 - 4 * width)},
    {...pop, name: `pop${width}/individual-span-overflow`, sp: u32(0x100000001 - width), fault: ss},
    {...pop, name: `pop${width}/later-page-read-failure`, sp: 0x8000 - 2 * width,
      deny: [0x8000, 2], fault: [1, 0, 2, 0x8000, 1, width]},
    {...pop, name: `pop${width}/skipped-esp-slot-cannot-report-a-read-fault`,
      synthetic: {packet: [1, 0, 1, 0x8080 + 3 * width, 1, width]}},
    {...push, name: `push${width}/segment-policy-before-earlier-page`, cache: expand(lastPush),
      deny: [0x8000, 1], fault: ss},
    {...pop, name: `pop${width}/segment-policy-before-earlier-page`, sp: 0x8020, cache: normal(lastRead - 1),
      deny: [0x8000, 2], fault: ss},
    {...push, name: `push${width}/budget-before-ss`, cache: expand(lastPush), fault: ss, budget: 0},
    {...pop, name: `pop${width}/cancel-before-ss`, sp: 0x8020, cache: normal(lastRead - 1), fault: ss, cancel: 1},
    {...push, name: `push${width}/valid-ss-packet`, synthetic: {packet: ss, reason: 15}},
    {...pop, name: `pop${width}/ss-packet-rejects-width`, synthetic: {packet: [7, 0, 0, 0, 0, width + 1]}},
    {...push, name: `push${width}/ss-packet-rejects-detail`, synthetic: {packet: [7, 0, 1, 0, 0, width]}},
    {...push, name: `push${width}/ss-packet-rejects-changed-code-status`, synthetic: {packet: ss, status: 11}},
  );
}
function oracle(registers, ram, row) {
  const result = [...registers], width = row.width;
  const write = (address, value) => {for (let byte = 0; byte < width; byte++) ram.set(u32(address + byte), Number((BigInt(value) >> BigInt(byte * 8)) & 255n));};
  const read = address => {let value = 0n; for (let byte = 0; byte < width; byte++) value += BigInt(ram.get(u32(address + byte))) << BigInt(byte * 8); return Number(value);};
  if (row.action === 'push') {
    for (let index = 0; index < 8; index++) write(u32(BigInt(registers[4]) - BigInt((index + 1) * width)), registers[index]);
    result[4] = u32(BigInt(registers[4]) - BigInt(8 * width));
  } else {
    for (const [slot, register] of [[0, 7], [1, 6], [2, 5], [4, 3], [5, 2], [6, 1], [7, 0]]) {
      const value = read(u32(BigInt(registers[4]) + BigInt(slot * width)));
      result[register] = width === 2 ? u32(BigInt(registers[register] & 0xffff0000) + BigInt(value)) : value;
    }
    result[4] = u32(BigInt(registers[4]) + BigInt(8 * width));
  }
  return result;
}
const proof = harness('bulk', [
  'engine/tests/cpu/stack/bulk/segments/wasm.rs',
  'engine/tests/cpu/stack/bulk/segments/native.rs',
  'engine/tests/cpu/stack/bulk/segments/fixtures/p2-bulk-segments/runtime/run.mjs',
]);
proof.run(rows, oracle); proof.finish();
