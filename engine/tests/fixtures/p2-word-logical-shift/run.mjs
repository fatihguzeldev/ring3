import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync, readdirSync, lstatSync } from "node:fs";
import { join } from "node:path";
import { readEngine } from "../support/engine.mjs";

const [enginePath, output, root] = process.argv.slice(2);
const SIZE = 4364;
const PC = 0x1000;
const RAW = [0, 1, 2, 15, 16, 17, 31, 32, 33, 255];
const EDGES = [0, 1, 0x7fff, 0x8000, 0xffff];
assert.ok(enginePath && output && root);
assert.match(process.env.RING3_ENGINE_SHA256 ?? "", /^[a-f0-9]{64}$/);
assert.deepEqual(readdirSync(output).sort(), ["initial-arena.bin", "word-logical-shift.x86"]);
for (const name of readdirSync(output)) {
    const stat = lstatSync(join(output, name));
    assert.ok(stat.isFile() && !stat.isSymbolicLink());
}
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const engine = readEngine(enginePath);
assert.equal(engine.sha256, process.env.RING3_ENGINE_SHA256);
assert.deepEqual(WebAssembly.Module.imports(engine.module), []);
const initial = readFileSync(join(output, "initial-arena.bin"));
assert.equal(initial.length, SIZE);
const bank = Buffer.alloc(1024, 0xcc);
const groups = [];
for (const [family, opcode, offset] of [["one", 0xd1, 0], ["immediate", 0xc1, 256], ["cl", 0xd3, 512]]) {
    let at = offset;
    const rows = [];
    for (const [kind, extension] of [["shl", 4], ["shr", 5]]) {
        for (let destination = 0; destination < 8; destination++) {
            for (let pass = 0; pass < (family === "immediate" ? 2 : 1); pass++) {
                const raw = RAW[(destination + pass * 8) % RAW.length];
                const bytes = Buffer.from([0x66, opcode, 0xc0 | (extension << 3) | destination,
                    ...(family === "immediate" ? [raw] : [])]);
                rows.push({ kind, family, destination, raw, pc: PC + at, bytes });
                bank.set(bytes, at);
                at += bytes.length;
            }
        }
    }
    const instructions = rows.length + 1;
    assert.ok(instructions <= 64);
    assert.equal(at - offset, family === "immediate" ? 128 : 48);
    bank.set([0xeb, 0], at);
    groups.push({ family, pc: PC + offset, length: at - offset + 2, rows, instructions });
}
const chain = Buffer.from([
    0x66, 0xb9, 0x21, 0, 0x66, 0xd3, 0xe1, 0x66, 0xd3, 0xe9,
    0x66, 0xc1, 0xe1, 0, 0x0f, 0x92, 0xc0, 0x66, 0x83, 0xd0, 0, 0xeb, 0,
]);
assert.equal(chain.length, 23);
bank.set(chain, 768);
groups.push({ family: "retained", pc: PC + 768, length: chain.length, rows: [], instructions: 7 });
assert.deepEqual(bank, readFileSync(join(output, "word-logical-shift.x86")));

const paths = [
    "engine/src/abi/arena.rs", "engine/src/abi/header.rs", "engine/src/abi/x86/state.rs",
    "engine/src/abi/x86/exit.rs", "engine/src/abi/x86/x87.rs", "engine/src/cpu/x86/ir.rs",
    "engine/src/cpu/x86/decode/decoder.rs", "engine/src/cpu/x86/decode/operands.rs",
    "engine/src/cpu/x86/decode/lower.rs", "engine/src/cpu/x86/decode/profile.rs",
    "engine/src/cpu/x86/decode/integer.rs", "engine/src/cpu/dbt/region.rs",
    "engine/src/cpu/dbt/wasm/integer.rs", "engine/src/cpu/dbt/wasm/emitter.rs",
    "engine/src/cpu/dbt/wasm/abi.rs", "engine/src/cpu/dbt/wasm/locals.rs",
    "engine/src/memory/space.rs", "engine/src/process/instance.rs", "engine/src/process/resident.rs",
    "engine/src/process/wasm.rs", "engine/tests/cpu_word_logical_shift_wasm.rs",
    "engine/tests/fixtures/p2-word-logical-shift/run.mjs", "engine/tests/fixtures/support/engine.mjs",
];
function pins() {
    return Object.fromEntries(paths.map(path => {
        const bytes = readFileSync(join(root, path));
        return [path, { bytes: bytes.length, sha256: hash(bytes) }];
    }));
}
const sourcePins = pins();
const frames = [];
const journal = [];
const modules = [];
const counts = { contexts: 0, targets: 0, matrix: 0, retained: 0, generated_calls: 0, retired: 0, controls: 0 };
writeFileSync(join(output, "engine.wasm"), engine.bytes, { flag: "wx" });

function record(magic, size, fields) {
    const bytes = Buffer.alloc(size);
    bytes.write(magic);
    bytes.writeUInt16LE(1, 4);
    bytes.writeUInt16LE(1, 6);
    bytes.writeUInt32LE(size, 8);
    fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4));
    return bytes;
}
function words(values) {
    return Buffer.concat(values.map(value => {
        const bytes = Buffer.alloc(4);
        bytes.writeUInt32LE(value >>> 0);
        return bytes;
    }));
}
const state = cpu => record("R3ST", 56, [...cpu.registers, cpu.pc, cpu.flags]);
const exit = (reason, retired) => record("R3EX", 40, [reason, retired, 0, 0, 0, 0]);
const arena = context => Buffer.from(new Uint8Array(context.memory.buffer, context.base, SIZE));
function capture(context, label) {
    const bytes = arena(context);
    const row = { context: context.context, label, offset: frames.length * SIZE, bytes: SIZE, sha256: hash(bytes) };
    frames.push(bytes);
    return row;
}
function input(context, offset, bytes, label) {
    const before = capture(context, "before input " + label);
    const wanted = arena(context);
    new Uint8Array(context.memory.buffer).set(bytes, context.base + offset);
    wanted.set(bytes, offset);
    assert.deepEqual(arena(context), wanted);
    journal.push({ context: context.context, name: "input", offset, hex: bytes.toString("hex"), label,
        before, after: capture(context, "after input " + label) });
}
function host(context, name, args, label = name) {
    const before = capture(context, "before " + label);
    const wanted = arena(context);
    assert.equal(context.api[name](...args), 0, label);
    const after = arena(context);
    if (name.startsWith("compile_resident")) {
        assert.equal(after.readUInt32LE(140), 1);
        assert.equal(after.readUInt32LE(144), 24);
        after.subarray(140, 164).copy(wanted, 140);
    }
    assert.deepEqual(after, wanted, "exact host arena effect " + label);
    journal.push({ context: context.context, name, args, label,
        before, after: capture(context, "after " + label) });
}
function upload(context, address, bytes) {
    input(context, 140, bytes, "declared upload");
    host(context, "upload", [address, bytes.length]);
}
function cpu(context) {
    const bytes = arena(context);
    return { registers: Array.from({ length: 8 }, (_, index) => bytes.readUInt32LE(16 + index * 4)),
        pc: bytes.readUInt32LE(48), flags: bytes.readUInt32LE(52) };
}
function seed(context, pc, salt, flags, destination, value, raw) {
    const registers = Array.from({ length: 8 }, (_, index) =>
        ((0xa100 + index + ((salt & 1) << 5)) * 0x10000 + EDGES[(index + salt) % EDGES.length]) >>> 0);
    if (destination !== undefined) {
        registers[destination] = ((registers[destination] & 0xffff0000) | value) >>> 0;
    }
    if (raw !== undefined) {
        registers[1] = ((registers[1] & 0xffffff00) | raw) >>> 0;
    }
    const bytes = Buffer.alloc(100);
    bytes.set(state({ registers, pc, flags }));
    bytes.set(exit(3, 0), 56);
    input(context, 0, bytes, "explicit current CPU and exit seed");
}
function run(context, budget, label, wanted, reason = 1, retired = 1, status = 0, args) {
    const before = capture(context, "before generated " + label);
    const expected = arena(context);
    if (status === 0) {
        expected.set(state(wanted));
        expected.set(exit(reason, retired), 56);
    }
    const params = args ?? [context.base, context.base + 56, budget, context.base + 96];
    assert.equal(context.run(...params), status, label);
    assert.deepEqual(arena(context), expected, "full4364 " + label);
    counts.generated_calls++;
    if (status === 0) {
        counts.retired += retired;
    }
    journal.push({ context: context.context, name: "run", label, budget, status, args: params,
        before, after: capture(context, "after generated " + label) });
}
function shift(kind, value, raw, oldFlags) {
    const count = raw & 31;
    if (count === 0) {
        return { result: value, flags: oldFlags };
    }
    const result = kind === "shl" ? (value << count) & 0xffff : value >>> count;
    const carry = count >= 16 ? 0 : kind === "shl" ? (value >>> (16 - count)) & 1 : (value >>> (count - 1)) & 1;
    let ones = 0;
    for (let bit = 0; bit < 8; bit++) {
        ones += (result >>> bit) & 1;
    }
    const overflow = count === 1 ? kind === "shl" ? ((result >>> 15) ^ carry) & 1 : value >>> 15 : 0;
    const flags = (oldFlags & 0x402) | carry | (ones % 2 === 0 ? 4 : 0)
        | (result === 0 ? 64 : 0) | (result & 0x8000 ? 128 : 0) | (overflow << 11);
    return { result, flags };
}
function shifted(context, form, category, label) {
    const old = cpu(context);
    const raw = form.family === "one" ? 1 : form.family === "immediate" ? form.raw : old.registers[1] & 255;
    const outcome = shift(form.kind, old.registers[form.destination] & 0xffff, raw, old.flags);
    const registers = [...old.registers];
    registers[form.destination] = ((old.registers[form.destination] & 0xffff0000) | outcome.result) >>> 0;
    run(context, 1, label, { ...old, registers, pc: old.pc + form.bytes.length, flags: outcome.flags });
    counts.targets++;
    counts[category]++;
}
function compile(context, group) {
    input(context, 140, words(context.entries ? [group.pc] : [group.pc, group.length]), "one block descriptors");
    const method = context.owner === "resident"
        ? (context.entries ? "compile_resident_entries" : "compile_resident")
        : (context.entries ? "compile_entries" : "compile");
    host(context, method, context.entries ? [1, 0] : [1]);
    let binding;
    if (context.owner === "resident") {
        const bytes = arena(context);
        binding = Object.fromEntries(["low", "high", "pointer", "length"].map((name, index) =>
            [name, bytes.readUInt32LE(148 + index * 4)]));
    } else {
        binding = { generation: context.api.generation(), pointer: context.api.module_ptr() >>> 0,
            length: context.api.module_len() >>> 0 };
    }
    const bytes = Buffer.from(new Uint8Array(context.memory.buffer, binding.pointer, binding.length));
    const module = new WebAssembly.Module(bytes);
    const guard = context.owner === "resident" ? "guard_resident" : "guard";
    assert.deepEqual(WebAssembly.Module.imports(module), [
        { module: "env", name: "memory", kind: "memory" }, { module: "ring3", name: guard, kind: "function" },
    ]);
    assert.deepEqual(WebAssembly.Module.exports(module), [{ name: "run", kind: "function" }]);
    context.run = new WebAssembly.Instance(module, { env: { memory: context.memory }, ring3: context.api }).exports.run;
    const groupIndex = groups.indexOf(group);
    const file = `${context.owner}-${context.entries ? "entries" : "explicit"}-group${groupIndex}.wasm`;
    writeFileSync(join(output, file), bytes, { flag: "wx" });
    modules.push({ context: context.context, owner: context.owner, entries: context.entries, group: groupIndex,
        arena_base: context.base, file, ...binding, bytes: bytes.length, sha256: hash(bytes), imports: [guard] });
}
function open(owner, entries) {
    const instance = new WebAssembly.Instance(engine.module, {});
    const context = { owner, entries, context: ++counts.contexts, memory: instance.exports.memory, api: {} };
    for (const name of ["open", "close", "arena_ptr", "map", "protect", "upload", "compile", "compile_entries",
        "compile_resident", "compile_resident_entries", "generation", "module_ptr", "module_len", "guard", "guard_resident"]) {
        context.api[name] = instance.exports["ring3_abi_v1_" + name];
    }
    assert.equal(context.api.open(2, context.context, 0x66555661), 0);
    context.base = context.api.arena_ptr() >>> 0;
    input(context, 0, initial, "canonical initialized full arena");
    host(context, "map", [PC, 1, 7]);
    upload(context, PC, bank);
    host(context, "protect", [PC, 1, 4]);
    return context;
}
function close(context) {
    const before = capture(context, "before close");
    assert.equal(context.api.close(), 0);
    journal.push({ context: context.context, name: "close", args: [], before });
    assert.equal(context.run(context.base, context.base + 56, 1, context.base + 96), 5, "closed owner status only");
    counts.generated_calls++;
    counts.controls++;
    journal.push({ context: context.context, name: "closed-run", status: 5 });
}
const literals = [
    ["shl", 0x8000, 0, 0x8000, 0xcd7], ["shr", 0xffff, 32, 0xffff, 0xcd7],
    ["shl", 0x8000, 1, 0, 0xc47], ["shl", 0x7fff, 1, 0xfffe, 0xc82],
    ["shr", 0x8000, 1, 0x4000, 0xc06], ["shr", 1, 1, 0, 0x447],
    ["shl", 1, 15, 0x8000, 0x486], ["shl", 0x8000, 16, 0, 0x446],
    ["shr", 0xffff, 16, 0, 0x446], ["shl", 0xffff, 17, 0, 0x446],
    ["shr", 0xffff, 31, 0, 0x446], ["shr", 0x8000, 33, 0x4000, 0xc06],
];
for (const [kind, value, raw, result, flags] of literals) {
    assert.deepEqual(shift(kind, value, raw, 0xcd7), { result, flags });
}
for (const owner of ["replacement", "resident"]) {
    for (const entries of [false, true]) {
        const context = open(owner, entries);
        for (const group of groups.slice(0, 3)) {
            compile(context, group);
            for (const form of group.rows) {
                const raws = form.family === "cl"
                    ? [0, 1, RAW[(form.destination + 2) % RAW.length], RAW[(form.destination + 6) % RAW.length]]
                    : [undefined, undefined];
                for (let seedIndex = 0; seedIndex < raws.length; seedIndex++) {
                    const value = EDGES[(form.destination + (form.kind === "shr" ? 2 : 0)) % EDGES.length];
                    seed(context, form.pc, seedIndex, seedIndex % 2 === 0 ? 0xcd7 : 2,
                        form.destination, value, raws[seedIndex]);
                    shifted(context, form, "matrix", "current operand and effective count " + group.family);
                }
            }
        }
        compile(context, groups[3]);
        const start = groups[3].pc;
        seed(context, start, 3, 0xcd7);
        let old = cpu(context);
        let registers = [...old.registers];
        registers[1] = ((registers[1] & 0xffff0000) | 0x21) >>> 0;
        run(context, 1, "retained MOV CX0021", { ...old, registers, pc: start + 4 });
        shifted(context, { kind: "shl", family: "cl", destination: 1, bytes: Buffer.from([0x66, 0xd3, 0xe1]) },
            "retained", "retained SHL CXCL reads old CX0021 and raw21");
        shifted(context, { kind: "shr", family: "cl", destination: 1, bytes: Buffer.from([0x66, 0xd3, 0xe9]) },
            "retained", "retained SHR CXCL reads new CX0042 and raw42");
        shifted(context, { kind: "shl", family: "immediate", raw: 0, destination: 1, bytes: Buffer.from([0x66, 0xc1, 0xe1, 0]) },
            "retained", "retained q0 preserves predecessor flags");
        old = cpu(context);
        registers = [...old.registers];
        registers[0] = ((registers[0] & 0xffffff00) | (old.flags & 1)) >>> 0;
        run(context, 1, "retained SETB AL consumes preserved carry", { ...old, registers, pc: start + 17 });
        old = cpu(context);
        registers = [...old.registers];
        const value = registers[0] & 0xffff;
        const carry = old.flags & 1;
        const result = (value + carry) & 0xffff;
        registers[0] = ((registers[0] & 0xffff0000) | result) >>> 0;
        let ones = 0;
        for (let bit = 0; bit < 8; bit++) {
            ones += (result >>> bit) & 1;
        }
        const flags = (old.flags & 0x402) | Number(value + carry > 0xffff)
            | (ones % 2 === 0 ? 4 : 0) | ((value ^ result) & 16)
            | (result === 0 ? 64 : 0) | (result & 0x8000 ? 128 : 0)
            | ((~value & (value ^ result) & 0x8000) ? 0x800 : 0);
        run(context, 1, "retained ADC AX0 uses current carry", { ...old, registers, pc: start + 21, flags });
        run(context, 2, "JMP then cold padding", { ...cpu(context), pc: start + 23 }, 3, 1);
        seed(context, start, 6, 0xcd7);
        run(context, 0, "zero budget", cpu(context), 1, 0);
        counts.controls++;
        input(context, 96, words([1]), "explicit cancellation");
        run(context, 1, "cancellation", cpu(context), 2, 0);
        counts.controls++;
        input(context, 96, words([0]), "clear cancellation");
        for (const args of [[context.base + 1, context.base + 56, 1, context.base + 96],
            [context.base, context.base + 57, 1, context.base + 96],
            [context.base, context.base + 16, 1, context.base + 96],
            [context.base, context.base + 56, 1, context.base + 16]]) {
            run(context, 1, "invalid or overlapping call pointers", cpu(context), 1, 0, 1, args);
            counts.controls++;
        }
        host(context, "protect", [PC, 1, 7]);
        upload(context, PC, Buffer.from([0x66]));
        run(context, 1, "same-byte code upload rejects stale owner", cpu(context), 1, 0, 4);
        counts.controls++;
        close(context);
    }
}
assert.deepEqual(pins(), sourcePins);
assert.deepEqual(counts, { contexts: 4, targets: 652, matrix: 640, retained: 12,
    generated_calls: 700, retired: 668, controls: 32 });
assert.equal(modules.length, 16);
const hostCounts = Object.fromEntries(["map", "protect", "upload", "compile", "compile_entries",
    "compile_resident", "compile_resident_entries", "close"].map(name =>
    [name, journal.filter(row => row.name === name).length]));
assert.deepEqual(hostCounts, { map: 4, protect: 8, upload: 8,
    compile: 4, compile_entries: 4, compile_resident: 4, compile_resident_entries: 4, close: 4 });
assert.equal(journal.filter(row => row.name === "input").length, 684);
assert.equal(journal.length, 1424);
assert.equal(frames.length, 2836);
const raw = Buffer.concat(frames);
writeFileSync(join(output, "arenas.bin"), raw, { flag: "wx" });
const result = {
    status: "ok", engine: { bytes: engine.bytes.length, sha256: engine.sha256, caller_sha256: process.env.RING3_ENGINE_SHA256 },
    source_pins: sourcePins, counts: { ...counts, modules: modules.length, host_counts: hostCounts,
        host_inputs: journal.filter(row => row.name === "input").length, journal_records: journal.length, raw_frames: frames.length },
    modules, journal, raw: { file: "arenas.bin", frames: frames.length, bytes: raw.length, sha256: hash(raw) },
    limits: [
        "finite exact register WORD SHL/SHR families in four bound profiles; native standalone proof is separate",
        "full4364 input/host/generated pairs and preclose frame; current low16/high16/other parents and count-dependent declared FLAGS, no memory helper or stack effect",
        "four resident units/context, instruction banks17/33/17/7 all<=64, no released pointer use; retained current CXCL/q0/SETB/ADC has no interphase host reseed",
        "initial Rust arena and first frames require canonical source-default joins; opaque initialized FP128 preserved, not arbitrary raw80 seeds",
        "module validation/import/hash/binding receipt only, no full body/allocator history proof; open/getters bounded, closed-run raw status only",
        "deterministic undefined flags policy, not hardware equality; no exhaustive values/full ISA/performance/CI/browser/game claim",
    ],
};
writeFileSync(join(output, "result.json"), JSON.stringify(result, null, 2), { flag: "wx" });
console.log(JSON.stringify({ status: result.status, ...result.counts, output }));
