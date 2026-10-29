import { ENTRY, EXIT, DATA, STACK, ESP, oracle, expectedFpRecord } from "./compare-programs.mjs";

export function check(value, message) { if (!value) throw new Error(message); }
export function equal(actual, expected, message) {
  check(actual.length === expected.length && actual.every((v, i) => v === expected[i]), message);
}
export const hex = bytes => [...bytes].map(v => v.toString(16).padStart(2, "0")).join("");
export const hash = async bytes => hex(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)));
const pair = value => [Number(BigInt(value) & 0xffffffffn), Number(BigInt(value) >> 32n)];
export const policy = { units: 128, wasmBytes: 32 * 1024 * 1024, promotionHits: 0xffffffff, unitInstructions: 256, unitWasmBytes: 524288 };

export function fresh(artifact, p, { Engine, CodeCache, KEY }) {
  const instance = new WebAssembly.Instance(artifact.module, { ring3_observability: { now_ns: () => 0n } });
  const api = Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith("ring3_abi_v1_")).map(([name, value]) => [name.slice(13), value]));
  const engine = new Engine(api, instance.exports.memory, artifact.sha256);
  engine.ok("open", 32, ...KEY); engine.base = api.arena_ptr() >>> 0;
  for (const [address, permission] of [[ENTRY, 7], [DATA, 3], [STACK, 3]]) engine.ok("map", address, 1, permission);
  engine.ok("configure_execution_with_limits", policy.units, policy.wasmBytes, policy.promotionHits, policy.unitInstructions, policy.unitWasmBytes);
  const initial = new Uint8Array(engine.memory.buffer, engine.base, api.arena_len() >>> 0).slice();
  const codePage = new Uint8Array(4096); codePage.set(p.code);
  engine.request(codePage); engine.ok("upload", ENTRY, codePage.length);
  engine.request(p.data); engine.ok("upload", DATA, p.data.length);
  const cache = new CodeCache(engine, { slots: policy.units, budget: 50000 });
  const owners = new Map();
  for (const op of p.operations) owners.set(op.pc, cache.entry(op.pc));
  check(cache.evictions === 0 && cache.promotions === 0, "prewarm fits the explicit cache policy");
  return { artifact, p, engine, cache, initial, owners, codePage, moduleRecords: null };
}

export async function modules(context) {
  const { engine, cache } = context, result = [];
  for (const unit of cache.slots.values()) {
    engine.ok("resident_module", ...pair(unit.id));
    const d = new DataView(engine.take(24).buffer), at = d.getUint32(16, true), size = d.getUint32(20, true);
    const bytes = new Uint8Array(engine.memory.buffer).slice(at, at + size);
    result.push({ id: String(unit.id), slot: unit.slot, tier: unit.tier, size, sha256: await hash(bytes),
      imports: WebAssembly.Module.imports(new WebAssembly.Module(bytes)) });
  }
  return result;
}

export function reset(c, iterations, { cw = 0x037f, cancelled = false, fault = false } = {}) {
  const { engine, initial, p } = c;
  engine.request(p.data); engine.ok("upload", DATA, p.data.length);
  engine.request(new Uint8Array(4096)); engine.ok("upload", STACK, 4096);
  new Uint8Array(engine.memory.buffer, engine.base, initial.length).set(initial);
  const d = new DataView(engine.memory.buffer, engine.base, initial.length);
  for (const [offset, value] of [[16, 0x55555555], [20, iterations], [24, EXIT], [28, DATA], [32, ESP], [36, 0], [40, 0],
    [44, fault ? 0x6000 : DATA + 0xa0], [48, ENTRY], [52, 2], [96, Number(cancelled)]]) d.setUint32(offset, value, true);
  d.setUint16(4236 + 16, cw, true);
}

export function execute(c, { budget = 50000, work = Infinity, cancelled = false, collect = true, dispatch = true } = {}) {
  const { engine, cache, p } = c;
  const before = cache.stats(), receipts = []; let retired = 0, calls = 0, last = null;
  const start = performance.now();
  do {
    const pc = engine.get(48), unit = c.owners.get(pc);
    check(unit && p.starts.has(pc), `unwarmed authored frontier ${pc.toString(16)}`);
    cache.budget = Math.min(budget, work - retired);
    last = cache.execute(unit, { dispatch });
    const rawReceipt = engine.take(64), receipt = new DataView(rawReceipt.buffer);
    check(receipt.getUint32(36, true) === last.retired && receipt.getUint32(44, true) === last.reason,
      "returned stop agrees with the complete outer execution receipt");
    check(receipt.getUint32(40, true) <= last.retired, "sealed child progress is bounded by aggregate retirement");
    if (!dispatch) check(last.retired === engine.get(76), "direct raw child retirement agrees with its receipt");
    if (collect) receipts.push({ ...last, owner: String(last.owner), raw: hex(rawReceipt) });
    retired += last.retired; calls++;
    if (cancelled || ![1, 3].includes(last.reason) || retired >= work || engine.get(48) === EXIT) break;
    check(last.retired > 0 && calls < 10000000, "positive bounded authored progress");
  } while (true);
  const elapsedMs = performance.now() - start;
  for (const name of ["admissions", "evictions", "promotions"])
    check(cache[name] === before[name], `no ${name} in execution span`);
  return { elapsedMs, retired, calls, last: { ...last, owner: String(last.owner) }, receipts,
    pc: engine.get(48), reason: last.reason, registers: Array.from({ length: 8 }, (_, i) => engine.get(16 + i * 4)), flags: engine.get(52) };
}

export function nontransfer(c) {
  const { engine, initial } = c, result = new Uint8Array(140 + initial.length - 4236);
  result.set(new Uint8Array(engine.memory.buffer, engine.base, 140));
  result.set(new Uint8Array(engine.memory.buffer, engine.base + 4236, initial.length - 4236), 140);
  return result;
}

export function inspect(c, intervals = [[ENTRY, 4096], [DATA, 4096], [STACK, 4096]]) {
  const { engine, initial } = c, before = new Uint8Array(engine.memory.buffer, engine.base, initial.length).slice(), result = [];
  try {
    for (const [address, length] of intervals) {
      const bytes = new Uint8Array(length), d = new DataView(bytes.buffer);
      for (let offset = 0; offset < length; offset += 4) {
        engine.ok("read32", address + offset); check(engine.get(116) === 0, "public diagnostic read succeeds");
        d.setUint32(offset, engine.get(120), true);
      }
      result.push({ address, bytes });
    }
    const after = new Uint8Array(engine.memory.buffer, engine.base, initial.length);
    equal(after.slice(0, 100), before.slice(0, 100), "inspection preserves State/Exit/cancel");
    equal(after.slice(140), before.slice(140), "inspection preserves every non-helper arena byte");
    const helper = new Uint8Array(40), d = new DataView(helper.buffer); helper.set(new TextEncoder().encode("R3MH"));
    d.setUint32(4, 0x10001, true); d.setUint32(8, 40, true);
    const final = result.at(-1).bytes; d.setUint32(20, new DataView(final.buffer).getUint32(final.length - 4, true), true);
    equal(after.slice(100, 140), helper, "public diagnostic helper has its exact literal raw-read result");
  } finally {
    new Uint8Array(engine.memory.buffer, engine.base + 100, 40).set(before.slice(100, 140));
    equal(new Uint8Array(engine.memory.buffer, engine.base, initial.length), before, "observation restores only checked helper scratch");
  }
  return result;
}

export function validate(c, run, iterations, options = {}) {
  const o = oracle(c.p, iterations, options.work ?? Infinity, options), arena = nontransfer(c), memory = inspect(c);
  equal(memory[0].bytes, c.codePage, "entire authored code page unchanged");
  const live = new Uint8Array(c.engine.memory.buffer, c.engine.base, c.initial.length);
  equal(live.slice(4364, 4588), c.initial.slice(4364, 4588), "SIMD/MXCSR and segment banks stay unchanged");
  equal(live.slice(4612), c.initial.slice(4612), "YMM and segment-base companion stay unchanged");
  if (o) {
    equal(run.registers, o.regs, "independent integer register oracle");
    check(run.pc === o.pc && run.flags === o.eflags && run.retired === o.retired && run.reason === o.reason,
      `independent PC/FLAGS/retirement/stop oracle ${JSON.stringify({ actual: [run.pc, run.flags, run.retired, run.reason], expected: [o.pc, o.eflags, o.retired, o.reason] })}`);
    equal(memory[1].bytes, o.ram, "all operand/output RAM bytes versus authored literal oracle");
    equal(memory[2].bytes, o.stack, "entire stack page versus independent CALL/RET oracle");
    const fp = new Uint8Array(c.engine.memory.buffer, c.engine.base + 4236, 128).slice();
    const expectedFp = expectedFpRecord(c.initial, o, options.cw ?? 0x037f);
    const fpDifferences = [...fp].flatMap((actual, offset) => actual === expectedFp[offset] ? [] : [{ offset, actual, expected: expectedFp[offset] }]);
    check(fpDifferences.length === 0, `all FP80 record bytes versus exact dyadic oracle: ${JSON.stringify({
      case: c.p.name, artifact: c.artifact.label, artifactSha256: c.artifact.sha256, cw: options.cw ?? 0x037f,
      iterations, work: options.work ?? "complete", cancelled: Boolean(options.cancelled), fault: Boolean(options.fault),
      pc: run.pc, retired: run.retired, differences: fpDifferences, actualFpHex: hex(fp), expectedFpHex: hex(expectedFp),
    })}`);
    if (o.faulted) {
      check(c.engine.get(80) === 6 && c.engine.get(84) === 0x6000 && c.engine.get(88) === 2 && c.engine.get(92) === 4,
        "literal unmapped data-store page fault witness");
    }
  } else {
    check(!options.fault && !options.work, "weighted detailed frontiers use the existing actual oracle protocol");
    check(run.retired === (options.cancelled ? 0 : iterations * c.p.loopCount + c.p.finalCount), "authored weighted literal retirement");
    equal(memory[1].bytes, options.cancelled ? c.p.data : c.p.expectedData, "independent authored weighted rounding-margin RAM oracle");
    equal(memory[2].bytes, new Uint8Array(4096), "weighted workload does not alter stack");
  }
  return { ...run, arena, memory: memory.map(({ address, bytes }) => ({ address, hex: hex(bytes) })),
    oracle: o ? "exact dyadic FP80 + integer flags/PC/RAM/CALL/RET" : "authored weighted float32 rounding-margin output and literal retirement; FP80 parity only" };
}

export function compare(a, b) {
  equal(a.arena, b.arena, "full nontransfer architectural arena equality; no normalized fields");
  check(JSON.stringify(a.receipts) === JSON.stringify(b.receipts), "complete semantic/raw receipt sequence equality");
  check(JSON.stringify(a.memory) === JSON.stringify(b.memory), "entire mapped code/operand/stack RAM equality");
  check(a.retired === b.retired && a.pc === b.pc && a.flags === b.flags, "equal completed guest work");
}
export function publicResult({ arena, ...value }) { return { ...value, arenaHex: hex(arena) }; }
