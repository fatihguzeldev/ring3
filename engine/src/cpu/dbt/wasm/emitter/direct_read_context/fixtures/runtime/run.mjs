let input = '';
for await (const chunk of process.stdin) input += chunk;
const fixture = JSON.parse(input), initial = Buffer.from(fixture.arena, 'hex');
const PC = 0x1000, DATA = 0x4000, CROSS = DATA + 4093, TARGET = 0x2000;
const DESC = 0x1800, MAP = 0x2000, PERMS = MAP + (4 << 20), BACK = PERMS + (1 << 20);
const VALUES = new Map([[DATA, 0x81234567], [DATA + 4, 0x89abcdef], [CROSS, 0x10203040]]);
const PROGRAMS = {
  moves: [[0, 2, 'read', 0, DATA], [2, 3, 'read', 1, DATA + 4], [5, 2, 'copy'], [7, 2, 'jump']],
  fallback: [[0, 2, 'read', 0, DATA], [2, 6, 'read', 1, CROSS], [8, 1, 'nop'], [9, 2, 'jump']],
  untrusted: [[0, 2, 'read', 0, DATA], [2, 6, 'add', 1, CROSS], [8, 2, 'read', 6, DATA], [10, 2, 'jump']],
  last: [[0, 1, 'nop'], [1, 2, 'read', 0, DATA]],
  group: [[0, 2, 'read', 0, DATA], [2, 8, 'group'], [10, 2, 'read', 6, DATA], [12, 2, 'jump']],
};
let controls = 0, resetChecks = 0;

function instructions(body) {
  let at = 0;
  const leb = () => { let n = 0, shift = 0, byte; do { byte = body[at++]; assert.ok(byte !== undefined && shift <= 28); n += (byte & 127) * 2 ** shift; shift += 7; } while (byte & 128); return n; };
  let locals = 4;
  for (let n = leb(); n; n--) { locals += leb(); at++; }
  const ops = [];
  while (at < body.length) {
    const op = body[at++], args = [];
    if ([2, 3, 4].includes(op)) { args.push(body[at++]); assert.ok([0x40, 0x7f, 0x7e].includes(args[0])); }
    else if ([0x0c, 0x0d, 0x10, 0x20, 0x21, 0x22, 0x23, 0x24, 0x3f].includes(op)) args.push(leb());
    else if (op === 0x0e) { for (let n = leb() + 1; n; n--) args.push(leb()); }
    else if (op >= 0x28 && op <= 0x3e) args.push(leb(), leb());
    else if (op === 0x41 || op === 0x42) {
      let byte, n = 0; do { byte = body[at++]; assert.ok(byte !== undefined && ++n <= (op === 0x41 ? 5 : 10)); args.push(byte); } while (byte & 128);
    } else assert.ok([0x05, 0x0b, 0x0f, 0x1a, 0x1b].includes(op) || (op >= 0x45 && op <= 0xc4), `declared integer opcode ${op.toString(16)}`);
    ops.push({op, args});
  }
  return {locals, ops};
}

function pinReset(item, encoded) {
  const body = generatedRunBody(encoded), {locals, ops} = instructions(body);
  if (!item.resident) return;
  const tables = ops.flatMap((entry, index) => entry.op === 0x0e ? [index] : []);
  assert.equal(tables.length, 1, 'one selected authored block');
  let index = tables[0] - 1;
  assert.equal(ops[index--].op, 0x20, 'dispatch reads RESUME');
  let blocks = 0;
  while (ops[index].op === 2) { assert.deepEqual(ops[index--].args, [0x40]); blocks++; }
  assert.equal(blocks, PROGRAMS[item.name].length, 'one retained instruction label per authored operation');
  assert.deepEqual(ops[index--], {op: 0x21, args: [locals - 1]}, 'last private local reset before every selected br_table');
  assert.deepEqual(ops[index], {op: 0x41, args: [0]}, 'reset is literal false, not inherited predecessor state');
  resetChecks++;
}

function addFlags(flags, left, right, result) {
  let value = (flags & ~0x8d7) | 2;
  if (left + right > 0xffffffff) value |= 1;
  if ((result.toString(2).slice(-8).split('1').length - 1) % 2 === 0) value |= 4;
  value |= (left ^ right ^ result) & 16;
  if (result === 0) value |= 64;
  value |= (result >>> 24) & 128;
  if ((~(left ^ right) & (left ^ result) & 0x80000000) !== 0) value |= 0x800;
  return value >>> 0;
}

function expected(item, options, enabled) {
  const regs = [0x11111111, 0x22222222, options.target ?? TARGET, DATA, 0x9000, 0x55555555, 0x66666666, 0x77777777];
  let pc = PC + (options.entry ?? 0), flags = options.flags ?? 2, shadow = options.shadow ?? 0;
  let budget = options.budget ?? 32, cancel = options.cancel ?? 0, retired = 0, calls = 0, groups = 0;
  let limit = options.limit ?? 0xfffff, base = options.firstContext || options.getterMutation ? 0x8000 : 0, default32 = true;
  const stop = (reason, detail = 0, address = 0, access = 0, length = 0) => ({pc, flags, shadow, regs, retired, reason, detail, address, access, length, cancel, calls, groups});
  for (;;) {
    if (cancel) return stop(2);
    if (budget === 0) return stop(1);
    if (base !== 0 || !default32) return stop(pc > limit ? 11 : 3);
    const step = PROGRAMS[item.name].find(([offset]) => PC + offset === pc);
    if (!step) return stop(3);
    const [offset, length, kind, dest, address] = step;
    if (pc + length - 1 > limit) return stop(11);
    const tf = flags & 0x100;
    flags &= ~0x10000; shadow = 0;
    if (kind === 'read' || kind === 'add') {
      if (options.nullDs) return stop(11);
      const direct = enabled && item.resident && !options.forceFallback && address !== CROSS;
      if (!direct) {
        calls++;
        if (address === (options.mutateAddress ?? CROSS)) {
          const mode = options.mode ?? '';
          if (mode.includes('base')) base = 0x8000;
          if (mode.includes('default16')) default32 = false;
          if (mode.includes('limit')) limit = PC + offset + length - 1;
          if (mode.includes('cancel')) cancel = 1;
          if (base !== 0 || !default32) return stop(pc > limit ? 11 : 3);
          if (mode.includes('transport') || mode === 'invalidated') return stop(7, 3);
          if (mode.includes('malformed') || mode === 'legacy23') return stop(7, 2);
          if (mode === 'legacy-fault') return stop(5, 1, address, 1, 4);
        }
        if (address === CROSS && options.fault) return stop(23, options.fault === 'readonly' ? 5 : 4, DATA + 4096, 1, 4);
      }
      if (kind === 'read') regs[dest] = VALUES.get(address);
      else { const left = regs[dest], right = VALUES.get(address), value = (left + right) >>> 0; regs[dest] = value; flags = addFlags(flags, left, right, value); }
    } else if (kind === 'copy') regs[6] = regs[0];
    else if (kind === 'group') {
      assert.ok(!tf && budget >= 4, 'group scenario keeps its actual full skip');
      groups++;
      if (options.mode === 'group-base') base = 0x8000;
      if (options.mode === 'group-limit') limit = PC + 9;
      retired += 4; budget -= 4; pc += 8;
      continue;
    } else if (kind === 'jump') {
      if (regs[2] > limit) return stop(11);
    }
    pc = kind === 'jump' ? regs[2] : PC + offset + length;
    retired++; budget--;
    if (tf) return stop(16);
  }
}

function execute(item, module, options, enabled) {
  const memory = new WebAssembly.Memory({initial: 128, maximum: 128});
  const data = new DataView(memory.buffer), bytes = new Uint8Array(memory.buffer);
  const set = (at, value) => data.setUint32(at, value >>> 0, true), get = at => data.getUint32(at, true);
  bytes.fill(0x5a); bytes.set(initial); bytes.fill(0, MAP, BACK);
  set(4, 0x10003); set(12, options.shadow ?? 0);
  const regs = [0x11111111, 0x22222222, options.target ?? TARGET, DATA, 0x9000, 0x55555555, 0x66666666, 0x77777777];
  regs.forEach((value, index) => set(16 + 4 * index, value));
  set(48, PC + (options.entry ?? 0)); set(52, options.flags ?? 2); set(96, options.cancel ?? 0);
  const cache = get(fixture.segments + 32);
  set(fixture.segments + 32, (cache & ~0x010fffff) | (options.limit ?? 0xfffff));
  if (options.nullDs) { data.setUint16(fixture.segments + 16 + 2 * 3, 0, true); set(fixture.segments + 28 + 4 * 3, 0); }
  const changeBase = () => { data.setUint16(fixture.segments + 4, 3, true); set(fixture.bases + 20, 0x8000); };
  if (options.firstContext) changeBase();
  data.setBigUint64(DESC, 1n, true);
  [MAP, PERMS, BACK, 2].forEach((value, index) => set(DESC + 8 + index * 4, value));
  set(MAP + (DATA >>> 12) * 4, 1); set(MAP + ((DATA >>> 12) + 1) * 4, options.fault === 'unmapped' ? 0 : 2);
  bytes[PERMS + (DATA >>> 12)] = 1; bytes[PERMS + (DATA >>> 12) + 1] = options.fault === 'readonly' ? 2 : 1;
  for (const [address, value] of VALUES) for (let index = 0; index < 4; index++) bytes[BACK + address - DATA + index] = value >>> (8 * index);
  const packet = words => words.forEach((word, index) => set(100 + 4 * index, word));
  const success = value => packet([0x484d3352, 0x1000a, 40, 0, 0, value, 0, 0, 0, 4]);
  let calls = 0, groups = 0, getters = 0;
  const finishes = [];
  function read(address) {
    address >>>= 0; calls++;
    assert.ok(VALUES.has(address), 'only literal authored EAs reach the native fallback');
    if (address === (options.mutateAddress ?? CROSS)) {
      const mode = options.mode ?? '';
      if (mode.includes('base')) changeBase();
      if (mode.includes('default16')) set(fixture.segments + 32, get(fixture.segments + 32) & ~0x02000000);
      if (mode.includes('limit')) set(fixture.segments + 32, (get(fixture.segments + 32) & ~0x010fffff) | (PC + 7));
      if (mode.includes('cancel')) set(96, 1);
      if (mode.includes('transport')) return 7;
      if (mode === 'invalidated') return 11;
      if (mode === 'legacy-fault') { packet([0x484d3352, 0x1000a, 40, 0, 1, 0, 1, address, 1, 4]); return 0; }
      if (mode.includes('malformed')) { success(VALUES.get(address)); set(100, 0); return 0; }
      if (mode === 'legacy23') { success(VALUES.get(address)); return 23; }
    }
    for (let index = 0; index < 4; index++) {
      const at = address + index, mapping = get(MAP + (at >>> 12) * 4), permission = bytes[PERMS + (at >>> 12)];
      if (!mapping || !(permission & 1)) {
        packet([0x484d3352, 0x1000f, 40, 0, 9, 0, mapping ? 5 : 4, at, 1, 4]);
        data.setBigUint64(DESC, 0n, true);
        return 23;
      }
    }
    success(VALUES.get(address)); return 0;
  }
  const bindings = {};
  for (const imp of WebAssembly.Module.imports(module)) {
    if (imp.module === 'env') continue;
    if (imp.name === 'guard_detect' || imp.name === 'guard_detect_resident') bindings[imp.name] = () => 1n;
    else if (imp.name === 'finish_exception_detection') bindings[imp.name] = (...args) => { finishes.push(args); return 0; };
    else if (imp.name === 'cpu_read32') bindings[imp.name] = read;
    else if (imp.name === 'cpu_read32_view') bindings[imp.name] = epoch => {
      assert.equal(epoch, 1n); getters++;
      if (options.getterMutation) changeBase();
      return enabled && !options.forceFallback ? DESC : 0;
    };
    else if (imp.name === 'x87_resident_group') bindings[imp.name] = (...args) => {
      assert.equal(item.name, 'group'); assert.equal(args[4], 0); assert.equal(args[14], 4); groups++;
      if (options.mode === 'group-base') changeBase();
      if (options.mode === 'group-limit') set(fixture.segments + 32, (get(fixture.segments + 32) & ~0x010fffff) | (PC + 9));
      packet([0x484d3352, 0x10007, 40, 0, 0, args[13], 0, 0, 0, 0]);
      return (4n << 32n) | (3n << 36n);
    };
    else if (imp.name.startsWith('x87_')) bindings[imp.name] = () => { throw new Error('full group must take its real generated skip, not scalar/pair fallback'); };
    else throw new Error(`unexpected full-module import ${imp.name}`);
  }
  if (options.corrupt) set(options.corrupt === 'state' ? 0 : fixture.segments, 0);
  const before = Buffer.from(bytes);
  const {run} = new WebAssembly.Instance(module, {env: {memory}, ring3: bindings}).exports;
  const status = run(0, 56, options.budget ?? 32, 96);
  const output = {
    pc: get(48), flags: get(52), shadow: get(12), regs: regs.map((_, index) => get(16 + index * 4)),
    reason: get(72), retired: get(76), detail: get(80), address: get(84), access: get(88), length: get(92), cancel: get(96), calls, groups,
  };
  if (options.corrupt) { assert.equal(status, 2); assert.equal(getters, 0); assert.equal(calls, 0); assert.deepEqual(Buffer.from(bytes), before); }
  else {
    assert.equal(status, 0);
    const want = expected(item, options, enabled);
    assert.deepEqual(output, want, `${item.name}/${item.tier}: independent literal state/frontier/call count`);
    const version = want.reason === 16 ? 7 : want.reason === 23 ? 12 : 6;
    assert.deepEqual(Array.from({length: 10}, (_, index) => get(56 + 4 * index)), [0x58453352, 0x10000 | version, 40, 0, want.reason, want.retired, want.detail, want.address, want.access, want.length], 'literal complete Exit schema/payload');
    const seals = want.reason === 3 || want.reason === 5 || (want.reason >= 10 && want.reason <= 24);
    assert.equal(finishes.length, Number(seals));
    if (seals) assert.deepEqual(finishes[0], [1n, want.reason, want.detail, want.address, want.access, want.length, want.reason === 16 ? 2 : 0, 0, 0, 0, 0, 0], 'exact generated detector finish fields');
    assert.equal(getters, Number(item.resident), 'one pointer getter only after full preflight');
  }
  return {status, output, bytes: Buffer.from(bytes), finishes};
}

for (const item of fixture.modules) {
  const encoded = Buffer.from(item.bytes, 'hex'), module = new WebAssembly.Module(encoded);
  if (item.name !== 'group') pinReset(item, encoded);
  const imports = WebAssembly.Module.imports(module);
  assert.equal(imports.some(imp => imp.name === 'cpu_read32_view'), item.resident);
  const rows = [{}];
  if (item.resident && item.name === 'moves') {
    for (const entry of [0, 2, 5, 7]) for (const budget of [1, 4]) rows.push({entry, budget});
    for (const entry of [1, 3, 4, 6, 8]) rows.push({entry});
    rows.push({budget: 0}, {cancel: 1}, {budget: 0, cancel: 1, firstContext: true}, {flags: 0x102}, {flags: 0x10002}, {shadow: 1}, {flags: 0x10002, shadow: 1, limit: PC - 1}, {limit: PC + 1}, {getterMutation: true}, {firstContext: true}, {target: PC + 2, budget: 9}, {nullDs: true}, {corrupt: 'state'}, {corrupt: 'segment'});
  }
  if (item.name === 'fallback') {
    rows.push({entry: 2}, {entry: 8}, {budget: 2}, {mode: 'limit'}, {mode: 'limit-cancel'}, {mode: 'base'}, {mode: 'default16'}, {mode: 'base-transport'}, {mode: 'base-malformed'}, {mode: 'transport'}, {mode: 'invalidated'}, {mode: 'malformed'}, {mode: 'legacy23'}, {mode: 'legacy-fault'}, {fault: 'readonly'}, {fault: 'unmapped'});
  }
  if (item.name === 'untrusted') rows.push({mode: 'base'}, {mode: 'default16'}, {mode: 'limit'}, {mode: 'base-transport'}, {mode: 'base-malformed'});
  if (item.name === 'last') rows.push({budget: 1}, {budget: 2}, {entry: 1});
  if (item.name === 'group') rows.push({mode: 'group-base'}, {mode: 'group-limit'});
  for (const options of rows) {
    const fallback = execute(item, module, options, false), direct = execute(item, module, options, true);
    assert.deepEqual(direct.bytes, fallback.bytes, `${item.name}/${item.resident}/${item.tier}/${JSON.stringify(options)}: all Wasm RAM/opaque arena bytes`);
    assert.deepEqual(direct.finishes, fallback.finishes, 'same exact detection calls');
    controls++;
  }
}
assert.equal(resetChecks, 8, 'both tiers pin actual reset-before-br_table for four resident programs');
assert.equal(controls, 172, 'focused authored rows across exact bindings and tiers');
console.log('full-emitter direct read context controls passed');
