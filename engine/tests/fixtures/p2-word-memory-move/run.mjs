import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {mkdirSync, readFileSync, writeFileSync} from "node:fs";
import {dirname, join} from "node:path";
import {fileURLToPath} from "node:url";
import {readEngine} from "../support/engine.mjs";

const directory = dirname(fileURLToPath(import.meta.url));
const dataBytes = readFileSync(join(directory, "cases.json"));
const data = JSON.parse(dataBytes);
const SIZE = 4364;
const sha = bytes => createHash("sha256").update(bytes).digest("hex");
const u32 = value => value >>> 0;
const bytes = hex => Buffer.from(hex, "hex");
function record(magic, size, fields, version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4);
  b.writeUInt16LE(1, 6); b.writeUInt32LE(size, 8);
  fields.forEach((v, i) => b.writeUInt32LE(u32(v), 16 + i * 4)); return b;
}
const state = c => record("R3ST", 56, [...c.registers, c.pc, c.flags]);
const exit = (reason, count, fault, version = 2) => record("R3EX", 40, [reason, count, fault?.detail ?? 0, fault?.address ?? 0, fault?.access ?? 0, fault ? 2 : 0], version);
const helper = (form, value = 0, fault) => record("R3MH", 40, [fault ? 1 : 0, value, fault?.detail ?? 0, fault?.address ?? 0, fault?.access ?? 0, 2], form === "load" ? 2 : 4);
const words = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(u32(v), i * 4)); return b;};
const copy = c => ({...c, registers: [...c.registers]});
const defaultRegisters = [0xb64d8001, 0xc75e2222, 0xd86f3333, 0xe9704444, 0xfa815555, 0x8b926666, 0x9ca37777, 0x5008];

function address(code, registers) {
  let p = 2; const modrm = code[p++], mod = modrm >>> 6, rm = modrm & 7;
  let value = 0;
  if (rm === 4) {
    const sib = code[p++], index = (sib >>> 3) & 7, base = sib & 7;
    if (index !== 4) value += registers[index] * 2 ** (sib >>> 6);
    if (base === 5 && mod === 0) {value += code.readUInt32LE(p); p += 4;} else value += registers[base];
  } else if (rm === 5 && mod === 0) {value += code.readUInt32LE(p); p += 4;} else value += registers[rm];
  if (mod === 1) value += code.readInt8(p);
  if (mod === 2) value += code.readInt32LE(p);
  return u32(value);
}

function makePlan() {
  const events = [], banks = [], modules = [], contexts = []; let calls = 0;
  const add = (c, kind, fields = {}) => {const event = {id: events.length, context: c.id, kind, ...fields}; events.push(event); return event;};
  function input(c, offset, b, label) {add(c, "input", {offset, hex: b.toString("hex"), label});}
  function host(c, name, args, label = name) {add(c, "host", {name, args, label});}
  function map(c, start, pages = 1, permissions = 3) {host(c, "map", [start, pages, permissions]); for (let p = 0; p < pages; p++) {const page = u32(start + p * 4096); c.pages.add(page); for (let i = 0; i < 4096; i++) c.ram.delete(page + i);}}
  function unmap(c, start, pages = 1) {host(c, "unmap", [start, pages]); for (let p = 0; p < pages; p++) {const page = u32(start + p * 4096); c.pages.delete(page); for (let i = 0; i < 4096; i++) c.ram.delete(page + i);}}
  function upload(c, start, b, label = "declared RAM upload") {input(c, 140, b, label); host(c, "upload", [start, b.length], label); b.forEach((v, i) => c.ram.set(start + i, v));}
  function diagnose(c, start, length, label) {for (let i = 0; i < length; i++) add(c, "host", {name: "read8", args: [start + i], label, diagnostic: true, oracle_byte: c.ram.get(start + i) ?? 0});}
  function seed(c, pc, registers = defaultRegisters, flags = 0xcd7) {
    c.cpu = {registers: [...registers], pc, flags}; const b = Buffer.alloc(140);
    b.set(state(c.cpu)); b.set(exit(3, 0), 56); b.set(record("R3MH", 40, [0, 0xdecafbad, 0, 0, 0, 0]), 100);
    input(c, 0, b, "declared CPU/exit/helper seed");
  }
  function pc(c, value) {c.cpu = {...c.cpu, pc: value}; input(c, 48, words([value]), "declared PC selection");}
  function run(c, module, budget, label, next, reason = 1, count = 1, packet, fault, status = 0, extra = {}) {
    add(c, "run", {module: module.id, budget, label, ...extra, oracle: {next, reason, count, helper: packet?.toString("hex"), fault, status, exit_version: module.version}});
    calls++; if (status === 0) c.cpu = copy(next);
  }
  function close(c, module, label = "closed retained guard") {host(c, "close", []); add(c, "closed_run", {module: module.id, args: [0xffffffff, 0xffffffff, 1, 0xffffffff], label}); calls++;}
  function open(profile, label) {
    const c = {id: contexts.length + 1, profile, owner: profile.split("-")[0], entries: profile.endsWith("entries"), pages: new Set(), ram: new Map()};
    contexts.push({id: c.id, profile, label, open: [16, c.id, 0x574f5244]}); add(c, "open", {args: [16, c.id, 0x574f5244]});
    add(c, "initial", {label: "complete canonical arena/nondefault FP128"}); return c;
  }
  function compile(c, blocks, label, helperNames, uploadCode = true) {
    let byteOffset = 0;
    const bank = {id: banks.length + 1, context: c.id, label, blocks: blocks.map(block => {const result = {pc: block.pc, hex: block.hex, byte_offset: byteOffset}; byteOffset += bytes(block.hex).length; return result;})};
    bank.file = `bank-${bank.id}.x86`; bank.bytes = byteOffset; bank.sha256 = sha(Buffer.concat(blocks.map(block => bytes(block.hex)))); banks.push(bank);
    if (uploadCode) for (const block of blocks) upload(c, block.pc, bytes(block.hex), "authored code bank " + label);
    input(c, 140, words(blocks.flatMap(block => c.entries ? [block.pc] : [block.pc, bytes(block.hex).length])), "exact block descriptors");
    const name = c.owner === "resident" ? (c.entries ? "compile_resident_entries" : "compile_resident") : (c.entries ? "compile_entries" : "compile");
    host(c, name, c.entries ? [blocks.length, 0] : [blocks.length]);
    if (c.owner === "replacement") for (const getter of ["generation", "module_ptr", "module_len"]) host(c, getter, []);
    const module = {id: modules.length + 1, context: c.id, bank: bank.id, label, owner: c.owner, entries: c.entries, version: helperNames.length ? 2 : 1,
      imports: [c.owner === "resident" ? "guard_resident" : "guard", ...helperNames.map(name => name === "store16" && c.owner === "resident" ? "store_resident16" : name)]};
    modules.push(module); add(c, "module", {module: module.id}); return module;
  }
  function discard(c, module) {if (c.owner === "resident") host(c, "discard_unacknowledged_resident", ["key_low", "key_high", `module:${module.id}:low`, `module:${module.id}:high`]);}
  function ensure(c, start, length) {const result = []; for (let page = Math.floor(start / 4096) * 4096; page <= start + length - 1; page += 4096) if (!c.pages.has(page)) {map(c, page); result.push(page);} return result;}
  function normal(c, module, test, entry) {
    const code = bytes(test.x86), at = address(code, test.registers), start = at - 1, length = at === 0xfffffffe ? 3 : 4;
    const mapped = ensure(c, start, length), b = Buffer.alloc(length, 0x55);
    if (test.form === "load") {b[1] = test.word & 255; b[2] = test.word >>> 8;}
    upload(c, start, b); seed(c, entry, test.registers, test.flags); diagnose(c, start, length, "positive canaries before " + test.id);
    const next = copy(c.cpu); next.pc += code.length;
    const value = test.form === "load" ? test.word : test.form === "store" ? test.registers[test.register] & 65535 : test.word;
    if (test.form === "load") next.registers[test.register] = u32((next.registers[test.register] & 0xffff0000) | value);
    run(c, module, 1, test.id, next, 1, 1, helper(test.form, test.form === "load" ? value : 0));
    if (test.form !== "load") {c.ram.set(at, value & 255); c.ram.set(at + 1, value >>> 8);}
    diagnose(c, start, length, "positive canaries after " + test.id); for (const page of mapped) unmap(c, page);
  }
  function faults(c, module, form, entry) {
    const target = entry + 1, targetLength = form === "immediate" ? 5 : 3;
    for (const kind of data.faults) {
      const crossing = kind.startsWith("second"), overflow = kind === "overflow", permission = kind.endsWith("permission");
      const at = overflow ? 0xffffffff : crossing ? 0x6fff : 0x6008;
      if (overflow) {map(c, 0xfffff000); upload(c, 0xfffffffe, Buffer.from([0x55, 0x55]));}
      else if (kind !== "first-unmapped") {
        map(c, 0x6000, kind === "second-permission" ? 2 : 1);
        const sentinel = form === "load" ? Buffer.from([0x55, 1, 0x80, 0x55]) : Buffer.alloc(4, 0x55);
        upload(c, at - 1, crossing && kind === "second-unmapped" ? sentinel.subarray(0, 2) : sentinel);
      }
      const denied = crossing ? 0x7000 : 0x6000;
      if (permission) host(c, "protect", [denied, 1, form === "load" ? 2 : 1]);
      const registers = [...defaultRegisters]; registers[7] = at; seed(c, entry, registers);
      const next = {...copy(c.cpu), pc: target}, fault = {detail: overflow ? 3 : permission ? 2 : 1, address: overflow ? at : crossing ? 0x7000 : at, access: form === "load" ? 1 : 2};
      run(c, module, 2, form + " " + kind + " prefix then fault", next, 5, 1, helper(form, 0, fault), fault);
      run(c, module, 1, form + " " + kind + " unchangedCPU retry", next, 5, 0, helper(form, 0, fault), fault);
      if (overflow) diagnose(c, 0xfffffffe, 2, "overflow top-byte preservation before address repair");
      if (overflow) {c.cpu.registers[7] = 0x5008; input(c, 44, words([0x5008]), "explicit overflow address-register repair");}
      else if (permission) host(c, "protect", [denied, 1, 3]); else map(c, crossing ? 0x7000 : 0x6000);
      const repaired = overflow ? 0x5008 : at, diagnosticStart = overflow ? 0x5007 : at - 1;
      const diagnosticLength = 4; diagnose(c, diagnosticStart, diagnosticLength, "failed target bytes after repair before retry");
      const value = form === "load" ? (c.ram.get(repaired) ?? 0) | ((c.ram.get(repaired + 1) ?? 0) << 8) : 0x8001;
      const done = {...copy(c.cpu), pc: target + targetLength}; if (form === "load") done.registers[0] = u32((done.registers[0] & 0xffff0000) | value);
      run(c, module, 1, form + " " + kind + " repaired exact target", done, 1, 1, helper(form, form === "load" ? value : 0));
      if (form !== "load") {c.ram.set(repaired, 1); c.ram.set(repaired + 1, 0x80);}
      diagnose(c, diagnosticStart, diagnosticLength, "repaired target canaries after");
      if (overflow) unmap(c, 0xfffff000); else unmap(c, 0x6000, crossing ? 2 : 1);
    }
    const registers = [...defaultRegisters]; registers[7] = 0x6008; seed(c, target, registers);
    run(c, module, 0, form + " zero before unmapped target", c.cpu, 1, 0);
    input(c, 96, words([1]), "declared cancel"); run(c, module, 1, form + " cancel1", c.cpu, 2, 0); run(c, module, 0, form + " cancel0", c.cpu, 2, 0); input(c, 96, words([0]), "clear cancel");
    if (form !== "immediate") {
      seed(c, target); const packet = helper(form, form === "load" ? 0x8001 : 0); packet.writeUInt32LE(1, 36);
      diagnose(c, 0x5007, 4, "inert malformed helper RAM before");
      run(c, module, 1, form + " inert foreign wrong-width helper", c.cpu, 7, 0, packet, undefined, 0, {foreign_helper: form, foreign_packet: packet.toString("hex"), protocol_detail: 2});
      diagnose(c, 0x5007, 4, "inert malformed helper RAM after");
    }
  }
  function mixed(c) {
    const module = compile(c, [{pc: data.mixed.pc, hex: data.mixed.x86}], "mixed currentRAM chain", ["read16", "store16"]);
    map(c, 0x6000, 2); upload(c, 0x6ffc, Buffer.alloc(6, 0x55)); seed(c, 0x1400, data.mixed.registers, data.mixed.flags);
    const end = copy(c.cpu); end.registers[0] = 0xa53c8002; end.registers[2] = 0xb74d8002; end.flags = 0x482; end.pc = 0x1800;
    run(c, module, 8, "mixed normal without reseed", end, 3, 6, helper("load", 0x8002));
    for (const [at, value] of [[0x6ffd, 1], [0x6ffe, 0x80], [0x6fff, 2], [0x7000, 0x80]]) c.ram.set(at, value);
    diagnose(c, 0x6ffc, 6, "mixed normal final RAM"); unmap(c, 0x6000, 2); map(c, 0x6000); upload(c, 0x6ffc, Buffer.alloc(4, 0x55)); seed(c, 0x1400, data.mixed.registers, data.mixed.flags);
    const stopped = copy(c.cpu); stopped.registers[0] = 0xa53c8002; stopped.flags = 0x482; stopped.pc = 0x140c;
    const fault = {detail: 1, address: 0x7000, access: 2};
    run(c, module, 8, "mixed first3 then atomic fault", stopped, 5, 3, helper("store", 0, fault), fault); c.ram.set(0x6ffd, 1); c.ram.set(0x6ffe, 0x80);
    diagnose(c, 0x6ffc, 4, "mixed fault accessible prefix unchanged"); run(c, module, 1, "mixed unchangedCPU retry", c.cpu, 5, 0, helper("store", 0, fault), fault);
    map(c, 0x7000); diagnose(c, 0x6ffc, 6, "mixed map-only repair before retry");
    run(c, module, 1, "mixed map-only store continuation", {...copy(c.cpu), pc: 0x1410}, 1, 1, helper("store")); c.ram.set(0x6fff, 2); c.ram.set(0x7000, 0x80);
    const reload = {...copy(c.cpu), pc: 0x1414}; reload.registers[2] = 0xb74d8002; run(c, module, 1, "mixed reload currentRAM", reload, 1, 1, helper("load", 0x8002));
    run(c, module, 2, "mixed cold continuation no replay", {...copy(c.cpu), pc: 0x1800}, 3, 1); diagnose(c, 0x6ffc, 6, "mixed repaired final RAM");
    for (const field of ["state", "exit", "cancel"]) run(c, module, 1, "invalid " + field + " pointer", c.cpu, 1, 0, undefined, undefined, 1, {invalid_pointer: field});
    pc(c, 0x1800); run(c, module, 1, "cold PC", c.cpu, 3, 0);
    upload(c, 0x1400, bytes("66"), "external same-byte code invalidation"); run(c, module, 1, "external stale owner", c.cpu, 1, 0, undefined, undefined, 4); close(c, module);
  }
  for (const profile of data.profiles) {
    const c = open(profile, "ordinary/fault/mixed"); map(c, 0x1000, 1, 7); map(c, 0x5000); upload(c, 0x5007, bytes("55018055"));
    for (const form of ["load", "store", "immediate"]) {
      const tests = data.positive_cases.filter(test => test.form === form);
      const blocks = tests.map((test, i) => ({pc: 0x1000 + i * 16, hex: test.x86 + "eb00", test}));
      blocks.push({pc: 0x1000 + tests.length * 16, hex: data.fault_forms[form], fault: true});
      for (let i = 0; i < blocks.length; i += 8) {
        const batch = blocks.slice(i, i + 8), module = compile(c, batch, form + " batch " + (i / 8), [form === "load" ? "read16" : "store16"]);
        for (const block of batch) if (block.test) normal(c, module, block.test, block.pc); else faults(c, module, form, block.pc);
        discard(c, module);
      }
    }
    mixed(c);
  }
  for (const profile of data.profiles) for (const form of ["store", "immediate"]) for (const same of [true, false]) {
    const c = open(profile, "self-code " + form + " " + same); map(c, 0x1000, 2, 7); const codePC = 0x1fff;
    const x86 = form === "store" ? data.self_code_forms.store : data.self_code_forms[same ? "immediate_same" : "immediate_changed"];
    const module = compile(c, [{pc: codePC, hex: x86}], "self-code writer", ["store16"]);
    let unrelated; if (c.owner === "resident") {map(c, 0x3000, 1, 7); unrelated = compile(c, [{pc: 0x3000, hex: "90eb00"}], "unrelated retained owner", []);}
    const value = same ? bytes(x86).readUInt16LE(0) : 0x9090, registers = [...defaultRegisters]; registers[0] = 0xa53c0000 | value; registers[7] = codePC; seed(c, codePC, registers, 2);
    diagnose(c, codePC, 2, "self-code before"); const next = {...copy(c.cpu), pc: codePC + (form === "store" ? 3 : 5)};
    run(c, module, 1, "self-code commits before invalidation", next, 6, 1, helper("store")); c.ram.set(codePC, value & 255); c.ram.set(codePC + 1, value >>> 8); diagnose(c, codePC, 2, "self-code after");
    run(c, module, 1, "self-code stale neutral", c.cpu, 1, 0, undefined, undefined, 4);
    const tail = compile(c, [{pc: next.pc, hex: "eb00"}], "fresh suffix no CPU/RAM repair", [], false);
    run(c, tail, 2, "suffix no store replay", {...copy(c.cpu), pc: next.pc + 2}, 3, 1);
    if (unrelated) {pc(c, 0x3000); run(c, unrelated, 1, "unrelated retained owner current", {...copy(c.cpu), pc: 0x3001}, 1, 1);}
    close(c, module);
  }
  for (const direction of data.cross_owner.directions) for (const word of data.cross_owner.words) {
    const c = open("resident-explicit", "cross-owner " + direction + " " + word); for (const at of [0x1000, 0x3000, 0x5000]) map(c, at, 1, 7);
    const from = direction === "a-to-b" ? 0x1000 : 0x3000, to = direction === "a-to-b" ? 0x3000 : 0x1000;
    const writer = compile(c, [{pc: from, hex: data.cross_owner.writer}], "cross-owner writer", ["store16"]);
    const companion = compile(c, [{pc: to, hex: data.cross_owner.companion}], "affected companion", []);
    const unrelated = compile(c, [{pc: 0x5000, hex: data.cross_owner.unrelated}], "unrelated companion", []);
    const registers = [...defaultRegisters]; registers[0] = 0xa53c0000 | word; registers[7] = to; seed(c, from, registers);
    diagnose(c, to, 2, "cross-owner before"); run(c, writer, 1, "writer stays current while companion invalidates", {...copy(c.cpu), pc: from + 3}, 1, 1, helper("store")); c.ram.set(to, word & 255); c.ram.set(to + 1, word >>> 8); diagnose(c, to, 2, "cross-owner after");
    run(c, companion, 1, "affected companion stale", c.cpu, 1, 0, undefined, undefined, 4); pc(c, 0x5000); run(c, unrelated, 1, "cross-owner unrelated current", {...copy(c.cpu), pc: 0x5001}, 1, 1);
    close(c, writer); add(c, "closed_run", {module: companion.id, args: [0xffffffff, 0xffffffff, 1, 0xffffffff], label: "closed affected companion"}); calls++;
  }
  const counts = {generated_calls: calls, engine_contexts: contexts.length, generated_modules: modules.length, events: events.length,
    input_events: events.filter(e => e.kind === "input" || e.kind === "initial").length,
    host_events: events.filter(e => e.kind === "host").length,
    host_api_calls: events.filter(e => e.kind === "host").length + contexts.length * 2,
    closed_status_only_calls: events.filter(e => e.kind === "closed_run").length,
    physical_files: banks.length + modules.length + 7,
    raw_frames: events.reduce((n, e) => n + (e.kind === "open" || e.kind === "host" && e.name === "close" ? 1 : ["input", "initial", "host", "run"].includes(e.kind) ? 2 : 0), 0)};
  assert.equal(counts.generated_calls, 600); assert.equal(counts.engine_contexts, 24); assert.equal(counts.generated_modules, 88);
  return {schema_version: 1, input_sha256: sha(dataBytes), counts, contexts, banks, modules, events};
}

const plan = makePlan();
function writePlan(output, writeCases = true) {
  writeFileSync(join(output, "plan.json"), JSON.stringify(plan, null, 2) + "\n", {flag: "wx"});
  if (writeCases) writeFileSync(join(output, "cases.json"), dataBytes, {flag: "wx"});
  for (const bank of plan.banks) writeFileSync(join(output, bank.file), Buffer.concat(bank.blocks.map(block => bytes(block.hex))), {flag: "wx"});
}
if (process.argv[2] === "--plan") {
  const output = process.argv[3]; assert.ok(output); mkdirSync(output, {recursive: true});
  writePlan(output);
  console.log(JSON.stringify(plan.counts));
} else {
  const [enginePath, injectorPath, output, root] = process.argv.slice(2); assert.ok(root);
  assert.match(process.env.RING3_ENGINE_SHA256 ?? "", /^[a-f0-9]{64}$/);
  const engine = readEngine(enginePath), injectorBytes = readFileSync(injectorPath), injectorModule = new WebAssembly.Module(injectorBytes);
  assert.deepEqual(WebAssembly.Module.imports(engine.module), []); assert.deepEqual(WebAssembly.Module.imports(injectorModule), [{module: "env", name: "memory", kind: "memory"}]);
  const initial = readFileSync(join(output, "initial-arena.bin")); assert.equal(initial.length, SIZE);
  assert.deepEqual(readFileSync(join(output, "cases.json")), dataBytes, "Rust bridge/source authored input agreement");
  writePlan(output, false); writeFileSync(join(output, "engine.wasm"), engine.bytes, {flag: "wx"});
  const paths = ["engine/src/abi/arena.rs", "engine/src/abi/memory_helper.rs", "engine/src/abi/x86/x87.rs", "engine/src/abi/wasm/exports.rs", "engine/src/cpu/x86/ir.rs", "engine/src/cpu/x86/decode/lower.rs", "engine/src/cpu/x86/decode/profile.rs", "engine/src/cpu/dbt/region.rs", "engine/src/cpu/dbt/wasm/integer.rs", "engine/src/cpu/dbt/wasm/emitter.rs", "engine/src/cpu/dbt/wasm/locals.rs", "engine/src/cpu/dbt/wasm/memory.rs", "engine/src/cpu/dbt/wasm/memory/narrow.rs", "engine/src/cpu/dbt/wasm/memory/word_store.rs", "engine/src/memory/space.rs", "engine/src/process/instance.rs", "engine/src/process/resident.rs", "engine/tests/cpu_word_memory_move_wasm.rs", "engine/tests/fixtures/p2-word-memory-move/run.mjs", "engine/tests/fixtures/p2-word-memory-move/cases.json", "engine/tests/fixtures/support/engine.mjs"];
  const sources = () => Object.fromEntries(paths.map(path => {const b = readFileSync(join(root, path)); return [path, {bytes: b.length, sha256: sha(b)}];}));
  const pins = sources(), contexts = new Map(), savedModules = new Map(), frames = [], frameHashes = [], journal = [], captures = [];
  const snapshot = c => {assert.equal(c.closed, false, "no freed arena access"); return Buffer.from(new Uint8Array(c.memory.buffer, c.base, SIZE));};
  const frame = c => {const b = snapshot(c), index = frames.length; frames.push(b); frameHashes.push(sha(b)); return index;};
  function save(status, failure) {
    const raw = Buffer.concat(frames); writeFileSync(join(output, "arenas.bin"), raw, {flag: "wx"});
    const result = {schema_version: 1, status, engine: {sha256: engine.sha256, bytes: engine.bytes.length}, injector: {sha256: sha(injectorBytes), bytes: injectorBytes.length}, input_sha256: sha(dataBytes), source_pins: pins, source_pins_after: sources(),
      counts: plan.counts, modules: captures, journal, raw: {file: "arenas.bin", bytes: raw.length, sha256: sha(raw), frame_sha256: frameHashes},
      observed_counts: {events: journal.length, generated_calls: journal.filter(row => row.kind === "run" || row.kind === "closed_run").length, engine_contexts: contexts.size, generated_modules: captures.length, raw_frames: frames.length},
      ...(failure ? {failure} : {}), limits: ["finite authored flat32 cases; no throughput or exhaustive ISA/RAM claim", "closed guards are status-only; no freed arena/module pointer access", "malformed inert helpers prove CPU/exit rejection; no foreign RAM rollback claim"]};
    writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2) + "\n", {flag: "wx"});
  }
  for (const event of plan.events) {
    let c = contexts.get(event.context); const row = {event_id: event.id, context: event.context, kind: event.kind};
    try {
    if (event.kind === "open") {
      const instance = new WebAssembly.Instance(engine.module, {}), api = Object.fromEntries(Object.entries(instance.exports).filter(([name]) => name.startsWith("ring3_abi_v1_")).map(([name, fn]) => [name.slice(13), fn]));
      assert.equal(api.open(...event.args), 0); c = {api, memory: instance.exports.memory, base: api.arena_ptr() >>> 0, closed: false, key: event.args.slice(1), getters: {}}; contexts.set(event.context, c);
      Object.assign(row, {args: event.args, return_value: 0, arena_base: c.base, after_frame: frame(c)});
    } else if (event.kind === "module") {
      const spec = plan.modules[event.module - 1]; let binding;
      if (spec.owner === "replacement") binding = {generation: c.getters.generation >>> 0, pointer: c.getters.module_ptr >>> 0, length: c.getters.module_len >>> 0};
      else {const b = snapshot(c); binding = {low: b.readUInt32LE(148), high: b.readUInt32LE(152), pointer: b.readUInt32LE(156), length: b.readUInt32LE(160)};}
      const encoded = Buffer.from(new Uint8Array(c.memory.buffer, binding.pointer, binding.length)), module = new WebAssembly.Module(encoded);
      assert.deepEqual(WebAssembly.Module.imports(module), [{module: "env", name: "memory", kind: "memory"}, ...spec.imports.map(name => ({module: "ring3", name, kind: "function"}))]);
      const file = `module-${spec.id}.wasm`; writeFileSync(join(output, file), encoded, {flag: "wx"});
      savedModules.set(spec.id, {...spec, binding, module, run: new WebAssembly.Instance(module, {env: {memory: c.memory}, ring3: c.api}).exports.run});
      const capture = {id: spec.id, context: spec.context, bank: spec.bank, binding, file, bytes: encoded.length, sha256: sha(encoded), imports: spec.imports}; captures.push(capture); Object.assign(row, {module_id: spec.id, capture});
    } else if (event.kind === "closed_run") {
      assert.equal(c.closed, true); row.module_id = event.module; row.args = event.args; row.return_value = savedModules.get(event.module).run(...event.args); assert.equal(row.return_value, 5);
    } else {
      row.before_frame = frame(c); const expected = snapshot(c);
      if (event.kind === "input" || event.kind === "initial") {
        const b = event.kind === "initial" ? initial : bytes(event.hex), offset = event.kind === "initial" ? 0 : event.offset;
        expected.set(b, offset); new Uint8Array(c.memory.buffer).set(b, c.base + offset); Object.assign(row, {offset, hex: b.toString("hex")});
      } else if (event.kind === "host") {
        const args = event.args.map(arg => typeof arg === "number" ? arg : arg === "key_low" ? c.key[0] : arg === "key_high" ? c.key[1] : savedModules.get(Number(arg.split(":")[1])).binding[arg.split(":")[2]]);
        Object.assign(row, {name: event.name, args}); row.return_value = c.api[event.name](...args);
        if (event.name === "close") c.closed = true;
        if (["generation", "module_ptr", "module_len"].includes(event.name)) c.getters[event.name] = row.return_value;
        else assert.equal(row.return_value, 0, event.label);
        if (event.name === "read8") expected.set(record("R3MH", 40, [0, event.oracle_byte, 0, 0, 0, 1], 2), 100);
        if (event.name === "compile_resident" || event.name === "compile_resident_entries") {const receipt = snapshot(c).subarray(140, 164); assert.equal(receipt.readUInt32LE(0), 1); assert.equal(receipt.readUInt32LE(4), 24); expected.set(receipt, 140);}
      } else if (event.kind === "run") {
        const module = savedModules.get(event.module); let fn = module.run, foreign;
        if (event.foreign_helper) {
          foreign = new WebAssembly.Instance(injectorModule, {env: {memory: c.memory}}); foreign.exports.configure(c.base + 100);
          const names = event.foreign_helper === "load" ? {read16: foreign.exports.read16} : {store16: foreign.exports.store16, store_resident16: foreign.exports.store_resident16};
          fn = new WebAssembly.Instance(module.module, {env: {memory: c.memory}, ring3: {...c.api, ...names}}).exports.run;
        }
        const args = [c.base, c.base + 56, event.budget, c.base + 96]; if (event.invalid_pointer) args[{state: 0, exit: 1, cancel: 3}[event.invalid_pointer]] = 0xffffffff;
        Object.assign(row, {module_id: event.module, args}); row.return_value = fn(...args); assert.equal(row.return_value, event.oracle.status, event.label);
        if (row.return_value === 0) {
          expected.set(state(event.oracle.next)); const fault = event.protocol_detail ? {detail: event.protocol_detail} : event.oracle.fault;
          const ex = exit(event.oracle.reason, event.oracle.count, fault, event.oracle.exit_version); if (event.protocol_detail) {ex.writeUInt32LE(0, 32); ex.writeUInt32LE(0, 36);} expected.set(ex, 56);
          if (event.oracle.helper) expected.set(bytes(event.oracle.helper), 100);
        }
        if (foreign) {row.foreign_helper = event.foreign_helper; row.foreign_trace = {calls: foreign.exports.calls(), address: foreign.exports.address() >>> 0, value: foreign.exports.value() >>> 0}; assert.equal(row.foreign_trace.calls, 1);}
      }
      if (!c.closed) {row.after_frame = frame(c); assert.deepEqual(snapshot(c), expected, "full4364 " + event.label);}
    }
    journal.push(row);
    } catch (error) {
      if (c && !c.closed && row.after_frame === undefined) row.after_frame = frame(c);
      journal.push(row); save("failed", {event_id: event.id, label: event.label, message: error.message}); throw error;
    }
  }
  try {assert.equal(frames.length, plan.counts.raw_frames); assert.deepEqual(sources(), pins);}
  catch (error) {save("failed", {phase: "final census/source verification", message: error.message}); throw error;}
  save("ok"); console.log(JSON.stringify({status: "ok", ...plan.counts, output}));
}
