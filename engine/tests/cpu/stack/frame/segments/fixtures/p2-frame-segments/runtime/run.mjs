import {harness, u32} from '../../../../../support/segments.mjs';

const rows = [], expand = limit => u32(0xf2700000 + limit), normal = limit => u32(0xf2300000 + limit);
for (const width of [2, 4]) {
  const ss = [7, 0, 0, 0, 0, width];
  const enter = (level, allocation = 0) => ({width, level: level & 31, allocation,
    code: [...(width === 2 ? [0x66] : []), 0xc8, allocation & 255, allocation >> 8, level]});
  for (const level of [0, 1, 3, 31, 35]) rows.push({...enter(level), name: `enter${width}/overlay-level${level}`});
  rows.push(
    {...enter(3), name: `enter${width}/nested-read-rejected`, bp: 0x6000 + width, cache: expand(0x6000), fault: ss},
    {...enter(3), name: `enter${width}/last-frame-push-rejected`, cache: expand(0x8080 - 4 * width), fault: ss},
    {...enter(3), name: `enter${width}/all-pushes-exact`, cache: expand(0x8080 - 4 * width - 1)},
    {...enter(3), name: `enter${width}/wrap-and-overlay`, sp: 0, bp: 0},
    {...enter(0), name: `enter${width}/individual-span-overflow`, sp: 1, fault: ss},
    {...enter(3), name: `enter${width}/full-original-ebp-segment`, bp: 0x10000 + width, cache: normal(0x8fff), fault: ss},
    {...enter(0, 1), name: `enter${width}/final-allocation-span-overflow`, sp: width, fault: ss},
    {...enter(0, 16), name: `enter${width}/final-allocation-wrap`, sp: width},
    {...enter(0), name: `enter${width}/saved-bp-parent`, bp: 0xaabb8080},
    {...enter(3), name: `enter${width}/nested-read-page-permission`, bp: 0x6000 + width,
      deny: [0x6000, 2], fault: [1, 0, 2, 0x6000, 1, width]},
    {...enter(0, 0x1000), name: `enter${width}/final-allocation-page-probe`,
      deny: [0x7000, 1], fault: [1, 0, 2, 0x7080 - width, 2, width]},
    {...enter(3), name: `enter${width}/code-store-retirement`, sp: 0x1080, bp: 0x1080, smc: true},
    {...enter(3), name: `enter${width}/valid-ss-packet`, synthetic: {packet: ss, reason: 15}},
    {...enter(3), name: `enter${width}/ss-packet-rejects-address`, synthetic: {packet: [7, 0, 0, 1, 0, width]}},
    {...enter(3), name: `enter${width}/ss-packet-rejects-access`, synthetic: {packet: [7, 0, 0, 0, 2, width]}},
    {...enter(3), name: `enter${width}/ss-packet-rejects-value`, synthetic: {packet: [7, 1, 0, 0, 0, width]}},
  );
  for (const level of [0, 1, 3]) {
    const allocation = 16, count = level === 0 ? 1 : level + 1, last = 0x8080 - count * width - allocation;
    rows.push(
      {...enter(level, allocation), name: `enter${width}/final-allocation-level${level}-rejected`, cache: expand(last), fault: ss},
      {...enter(level, allocation), name: `enter${width}/final-allocation-level${level}-exact`, cache: expand(last - 1)},
    );
  }
  if (width === 2) rows.push({...enter(0, 16), name: 'enter2/allocation-borrows-through-esp-bit16', sp: 0x10008, bp: 0xaabb8080});
  rows.push(
    {...enter(0, 16), name: `enter${width}/segment-policy-before-earlier-page`,
      cache: expand(0x8080 - width - 16), deny: [0x8000, 1], fault: ss},
    {...enter(0, 16), name: `enter${width}/budget-before-final-ss`, cache: expand(0x8080 - width - 16), fault: ss, budget: 0},
    {...enter(0, 16), name: `enter${width}/cancel-before-final-ss`, cache: expand(0x8080 - width - 16), fault: ss, cancel: 1},
  );
}
function oracle(registers, ram, row) {
  const result = [...registers], width = row.width;
  const push = value => {
    result[4] = u32(BigInt(result[4]) - BigInt(width));
    for (let byte = 0; byte < width; byte++) ram.set(u32(result[4] + byte), Number((BigInt(value) >> BigInt(byte * 8)) & 255n));
  };
  const read = address => {let value = 0n; for (let byte = 0; byte < width; byte++) value += BigInt(ram.get(u32(address + byte))) << BigInt(byte * 8); return Number(value);};
  push(registers[5]); const frame = result[4];
  for (let index = 1; index < row.level; index++) push(read(u32(BigInt(registers[5]) - BigInt(index * width))));
  if (row.level) push(frame);
  result[5] = width === 2 ? u32(BigInt(registers[5] & 0xffff0000) + BigInt(frame & 0xffff)) : frame;
  result[4] = u32(BigInt(result[4]) - BigInt(row.allocation));
  return result;
}
const proof = harness('frame', [
  'engine/tests/cpu/stack/frame/segments/wasm.rs',
  'engine/tests/cpu/stack/frame/segments/native.rs',
  'engine/tests/cpu/stack/frame/segments/fixtures/p2-frame-segments/runtime/run.mjs',
]);
proof.run(rows, oracle); proof.finish();
