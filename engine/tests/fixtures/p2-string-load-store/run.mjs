import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { readEngine } from "../support/engine.mjs";

const [enginePath, output, root] = process.argv.slice(2);
assert.ok(enginePath && output && root);
assert.match(process.env.RING3_ENGINE_SHA256 ?? "", /^[0-9a-f]{64}$/);
const hash = bytes => createHash("sha256").update(bytes).digest("hex");
const engine = readEngine(enginePath), SIZE = 4364, PC = 0x1000, SRC = 0x5000, DST = 0x6000;
const bank = Buffer.alloc(64, 0xcc);
for (const [offset, bytes] of [[0,[0xac,0xeb,0]],[8,[0xaa,0xeb,0]],
  [16,[0xac,0xaa,0xac,0xaa,0xeb,0]],[32,[0x90,0xac,0xeb,0]],[40,[0x90,0xaa,0xeb,0]]]) bank.set(bytes, offset);
assert.deepEqual(bank, readFileSync(join(output, "string-load-store.x86")), "independent Rust/JS source bank");
const sourcePaths = ["engine/src/abi/arena.rs", "engine/src/cpu/x86/ir.rs", "engine/src/cpu/x86/decode/lower.rs",
  "engine/src/cpu/x86/decode/profile.rs", "engine/src/cpu/dbt/region.rs",
  "engine/src/cpu/dbt/wasm/integer.rs", "engine/src/cpu/dbt/wasm/memory.rs",
  "engine/src/cpu/dbt/wasm/memory/narrow.rs", "engine/src/cpu/dbt/wasm/memory/byte_store.rs",
  "engine/tests/cpu_string_load_store_wasm.rs", "engine/tests/fixtures/p2-string-load-store/run.mjs",
  "engine/tests/fixtures/support/engine.mjs"];
const sourcePins = () => Object.fromEntries(sourcePaths.map(path => {
  const bytes = readFileSync(join(root, path)); return [path, { bytes: bytes.length, sha256: hash(bytes) }];
}));
const beforeSources = sourcePins(), frames = [], journal = [], modules = [], cases = [];
let generated = 0, retired = 0, loads = 0, stores = 0;
writeFileSync(join(output, "engine.wasm"), engine.bytes, { flag: "wx" });
function record(magic, size, fields, version = 1) {
  const bytes = Buffer.alloc(size); bytes.write(magic); bytes.writeUInt16LE(version, 4);
  bytes.writeUInt16LE(1, 6); bytes.writeUInt32LE(size, 8);
  fields.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 16 + index * 4)); return bytes;
}
const state = cpu => record("R3ST", 56, [...cpu.registers, cpu.pc, cpu.flags]);
const exit = (reason, count, detail = 0, address = 0, access = 0) =>
  record("R3EX", 40, [reason, count, detail, address, access, access ? 1 : 0], 2);
const helper = (version, value = 0, detail = 0, address = 0, access = 0) =>
  record("R3MH", 40, [detail ? 1 : 0, detail ? 0 : value, detail, detail ? address : 0, access, 1], version);
const words = values => Buffer.concat(values.map(value => { const bytes = Buffer.alloc(4); bytes.writeUInt32LE(value >>> 0); return bytes; }));
function arena(ctx) { return Buffer.from(new Uint8Array(ctx.memory.buffer, ctx.base, SIZE)); }
function capture(ctx, label) {
  const bytes = arena(ctx), row = { owner: ctx.owner, label, offset: frames.length * SIZE,
    bytes: SIZE, sha256: hash(bytes) }; frames.push(bytes); return row;
}
function cpu(ctx) {
  const bytes = arena(ctx); return { registers: Array.from({length:8}, (_, i) => bytes.readUInt32LE(16 + i * 4)),
    pc: bytes.readUInt32LE(48), flags: bytes.readUInt32LE(52) };
}
function host(ctx, name, args, label = name) {
  const before = capture(ctx, "before " + label), oldCPU = arena(ctx).subarray(0, 56);
  assert.equal(ctx.api[name](...args), 0, label);
  assert.deepEqual(arena(ctx).subarray(0, 56), oldCPU, "host helper preserves CPU: " + label);
  journal.push({ owner: ctx.owner, name, args, label, before, after: capture(ctx, "after " + label) });
}
function input(ctx, offset, bytes, label) {
  const before = capture(ctx, "before input " + label);
  new Uint8Array(ctx.memory.buffer).set(bytes, ctx.base + offset);
  journal.push({ owner: ctx.owner, name: "input", offset, hex: bytes.toString("hex"), label,
    before, after: capture(ctx, "after input " + label) });
}
function upload(ctx, address, bytes) {
  input(ctx, 140, bytes, "upload bytes"); host(ctx, "upload", [address, bytes.length]);
}
function seed(ctx, pc, flags, source = SRC + 8, destination = DST + 8, al = 0x5a) {
  const registers = [0xa1b2c300|al,0xb1c2d378,0x34560f10,0x4567ff00,0x56789abc,0x6789abcd,source,destination];
  const bytes = Buffer.alloc(140); bytes.set(state({registers,pc,flags})); bytes.set(exit(3,0),56);
  bytes.set(record("R3MH",40,[0,0xdecafbad,0,0,0,0]),100);
  input(ctx, 0, bytes, "explicit initial CPU"); return cpu(ctx);
}
function run(ctx, budget, label, expectedCPU, reason = 1, count = 1, expectedHelper, fault = null, status = 0) {
  const beforeBytes = arena(ctx), before = capture(ctx, "before generated " + label);
  const expected = Buffer.from(beforeBytes);
  if (status === 0) {
    expected.set(state(expectedCPU)); expected.set(exit(reason,count, fault?.detail, fault?.address, fault?.access),56);
    if (expectedHelper) expected.set(expectedHelper,100);
  }
  assert.equal(ctx.run(ctx.base,ctx.base+56,budget,ctx.base+96),status,label);
  assert.deepEqual(arena(ctx),expected,"full arena: " + ctx.owner + "/" + label);
  generated++; retired += count * Number(status === 0);
  const row = { owner:ctx.owner,name:"run",label,budget,status,before,after:capture(ctx,"after generated " + label) };
  journal.push(row); return row;
}
function read(ctx, address) {
  host(ctx,"read8",[address]); return arena(ctx).readUInt32LE(120);
}
function step(ctx, kind, label, value, reason = 1, diagnostic = true) {
  const before = cpu(ctx), delta = before.flags & 0x400 ? -1 : 1;
  const next = {registers:[...before.registers],pc:(before.pc+1)>>>0,flags:before.flags};
  if (kind === "load") {
    next.registers[0] = ((before.registers[0] & 0xffffff00) | value)>>>0;
    next.registers[6] = (next.registers[6] + delta)>>>0; loads++;
  } else {
    assert.equal(before.registers[0] & 255,value,"STOSB uses declared current AL");
    next.registers[7] = (next.registers[7] + delta)>>>0; stores++;
  }
  const row = run(ctx,1,label,next,reason,1,helper(kind === "load" ? 2 : 3,kind === "load" ? value : 0));
  if (kind === "store" && diagnostic) assert.equal(read(ctx,before.registers[7]),value,"physical declared destination byte");
  cases.push({owner:ctx.owner,kind,label,before,next,value,run:row}); return next;
}
function open(owner, ordinal) {
  const instance = new WebAssembly.Instance(engine.module, {}), ctx = {owner,memory:instance.exports.memory,api:{}};
  for (const name of ["open","close","arena_ptr","map","unmap","protect","upload","read8","compile",
    "compile_resident","generation","module_ptr","module_len","guard","guard_resident","store8","store_resident8"])
    ctx.api[name] = instance.exports["ring3_abi_v1_"+name];
  assert.equal(ctx.api.open(8,ordinal,0x51525354),0); ctx.base = ctx.api.arena_ptr()>>>0;
  for (const address of [PC,SRC,DST,0xfffff000,0]) host(ctx,"map",[address,1,address === PC ? 7 : 3]);
  upload(ctx,PC,bank);
  input(ctx,140,words([PC,3,PC+8,3,PC+16,6,PC+32,4,PC+40,4]),"five compiler block descriptors");
  let binding;
  if (owner === "replacement") {
    host(ctx,"compile",[5]); binding = {generation:ctx.api.generation(),pointer:ctx.api.module_ptr()>>>0,length:ctx.api.module_len()>>>0};
  } else {
    host(ctx,"compile_resident",[5]); const receipt = arena(ctx);
    binding = Object.fromEntries(["low","high","pointer","length"].map((name,i)=>[name,receipt.readUInt32LE(148+i*4)]));
  }
  const bytes = Buffer.from(new Uint8Array(ctx.memory.buffer,binding.pointer,binding.length));
  const module = new WebAssembly.Module(bytes), names = owner === "replacement" ? ["guard","read8","store8"]
    : ["guard_resident","read8","store_resident8"];
  assert.deepEqual(WebAssembly.Module.imports(module),[{module:"env",name:"memory",kind:"memory"},
    ...names.map(name=>({module:"ring3",name,kind:"function"}))]);
  ctx.run = new WebAssembly.Instance(module,{env:{memory:ctx.memory},ring3:ctx.api}).exports.run;
  ctx.binding = binding; const file = owner + ".wasm"; writeFileSync(join(output,file),bytes,{flag:"wx"});
  modules.push({owner,file,...binding,bytes:bytes.length,sha256:hash(bytes),imports:names}); return ctx;
}
for (const [index,owner] of ["replacement","resident"].entries()) {
  const ctx = open(owner,index+1);
  const fp = record("R3FP",128,[0x55aa037f,0x03ffffff,0xf1234567,0xfedcba98,0xe246f135]);
  fp.set(Buffer.from(Array.from({length:80},(_,i)=>(i*73+29)&255)),40);
  input(ctx,4236,fp,"declared opaque FPU tail preserved by integer strings");
  for (const kind of ["load","store"]) for (const flags of [2,0x402,0x8d7,0xcd7]) for (const value of [0,1,0x80,0xff]) {
    const entry = PC + (kind === "load" ? 0 : 8);
    if (kind === "load") {
      upload(ctx,SRC+8,Buffer.from([value])); seed(ctx,entry,flags,SRC+8,0x9008);
    } else {
      upload(ctx,DST+8,Buffer.from([0x55])); seed(ctx,entry,flags,0x9008,DST+8,value);
    }
    step(ctx,kind,"both DF/current byte and high EAX",value);
    run(ctx,2,"JMP then cold continuation",{...cpu(ctx),pc:entry+3},3,1);
  }
  for (const kind of ["load","store"]) for (const [address,flags] of [[0xffffffff,2],[0,0xcd7],[SRC+8,2]]) {
    upload(ctx,address,Buffer.from([kind === "load" ? 0xa6 : 0x55]));
    seed(ctx,PC+(kind === "load" ? 0 : 8),flags,address,address,0xa6);
    step(ctx,kind,"wrapping or equal pointers",0xa6);
  }
  for (const flags of [2,0xcd7]) for (const overlap of [false,true]) {
    const delta = flags & 0x400 ? -1 : 1, destination = overlap ? SRC+8+delta : DST+8;
    upload(ctx,SRC+8,Buffer.from([0x23])); upload(ctx,SRC+8+delta,Buffer.from([0x91]));
    seed(ctx,PC+16,flags,SRC+8,destination,0x5a);
    step(ctx,"load","first live source -> AL",0x23,1,false);
    step(ctx,"store","first live AL -> memory",0x23,1,false);
    step(ctx,"load","second source observes overlap write",overlap ? 0x23 : 0x91,1,false);
    step(ctx,"store","second current AL is not compiled seed",overlap ? 0x23 : 0x91,1,false);
    assert.equal(read(ctx,destination),0x23); assert.equal(read(ctx,destination+delta),overlap ? 0x23 : 0x91);
  }
  const faults = [
    {kind:"load",address:0x7008,access:1,detail:1},
    {kind:"load",address:0x7008,access:1,detail:2,permission:2},
    {kind:"store",address:0x8008,access:2,detail:1},
    {kind:"store",address:0x8008,access:2,detail:2,permission:1},
  ];
  for (const fault of faults) {
    const page = fault.address & ~0xfff;
    if (fault.permission) {
      host(ctx,"map",[page,1,3]); upload(ctx,fault.address,Buffer.from([fault.kind === "load" ? 0x81 : 0x55]));
      host(ctx,"protect",[page,1,fault.permission]);
    }
    const entry = PC+(fault.kind === "load" ? 32 : 40);
    const initial = seed(ctx,entry,0xcd7,fault.kind === "load" ? fault.address : 0x9008,
      fault.kind === "store" ? fault.address : 0x9008,0x81);
    const before = {...initial,pc:entry+1};
    for (let attempt = 0; attempt < 2; attempt++) run(ctx,attempt ? 1 : 2,"NOP commits, fault target retains CPU on retry",before,5,attempt ? 0 : 1,
      helper(fault.access === 1 ? 2 : 3,0,fault.detail,fault.address,fault.access),fault);
    if (fault.kind === "store" && fault.permission) assert.equal(read(ctx,fault.address),0x55,"failed STOSB preserves physical sentinel");
    if (fault.permission) host(ctx,"protect",[page,1,3]); else host(ctx,"map",[page,1,3]);
    upload(ctx,fault.address,Buffer.from([fault.kind === "load" ? 0x81 : 0x55]));
    assert.deepEqual(cpu(ctx),before,"repair changes no CPU input or target EIP");
    step(ctx,fault.kind,"repaired target same current CPU and owner",0x81); host(ctx,"unmap",[page,1]);
  }
  const initial = seed(ctx,PC+16,2,0x7008,0x8008,0x5a);
  const readFault = {detail:1,address:0x7008,access:1}, writeFault = {detail:1,address:0x8008,access:2};
  for (let attempt=0;attempt<2;attempt++) run(ctx,4,"unmapped source wins before later unmapped destination",initial,5,0,helper(2,0,1,0x7008,1),readFault);
  host(ctx,"map",[0x7000,1,3]); upload(ctx,0x7008,Buffer.from([0x81]));
  step(ctx,"load","priority repair commits only AL and ESI",0x81,1,false);
  const loaded = cpu(ctx);
  for (let attempt=0;attempt<2;attempt++) run(ctx,3,"following STOSB fault retains completed LODSB",loaded,5,0,helper(3,0,1,0x8008,2),writeFault);
  host(ctx,"map",[0x8000,1,3]); step(ctx,"store","priority repair uses retained current AL",0x81);
  host(ctx,"unmap",[0x7000,1]); host(ctx,"unmap",[0x8000,1]);
  for (const entry of [PC,PC+8]) {
    seed(ctx,entry,2,0x9008,0xa008);
    run(ctx,0,"zero budget avoids unmapped byte",cpu(ctx),1,0);
    input(ctx,96,words([1]),"explicit cancellation"); run(ctx,1,"cancel avoids unmapped byte",cpu(ctx),2,0);
  }
  seed(ctx,PC+8,2,0x9008,PC+8,0xaa);
  step(ctx,"store","same-byte code store commits EDI before invalidation",0xaa,6,false);
  const held = Buffer.from(new Uint8Array(ctx.memory.buffer,ctx.binding.pointer,ctx.binding.length));
  run(ctx,1,"stale known owner preserves full arena",cpu(ctx),1,0,undefined,null,4);
  assert.deepEqual(Buffer.from(new Uint8Array(ctx.memory.buffer,ctx.binding.pointer,ctx.binding.length)),held,
    "immediate stale callback preserves live allocation before any getter or allocation");
  host(ctx,"close",[]); run(ctx,1,"closed known arena preserves full arena",cpu(ctx),1,0,undefined,null,5);
}
assert.deepEqual(sourcePins(),beforeSources,"fixture source bytes unchanged during execution");
assert.deepEqual({loads,stores,generated,retired},{loads:60,stores:62,generated:222,retired:194},"fixed finite source schedule");
const raw = Buffer.concat(frames); writeFileSync(join(output,"arenas.bin"),raw,{flag:"wx"});
const result = {status:"ok",engine:{bytes:engine.bytes.length,sha256:engine.sha256},source_pins:beforeSources,
  counts:{owners:2,modules:modules.length,lodsb:loads,stosb:stores,generated_calls:generated,retired,raw_frames:frames.length},
  modules,cases,journal,raw:{file:"arenas.bin",bytes:raw.length,sha256:hash(raw)},
  limits:["finite LODSB/STOSB cases; REP, prefixes and wider strings excluded",
    "native tests validate/decode modules; this fixture runs actual engine-generated Wasm",
    "full saved arena observations, selected physical byte diagnostics; no full RAM or generic body certification",
    "stale allocation comparison is live before close; closed owners have no allocation lifetime claim"]};
writeFileSync(join(output,"result.json"),JSON.stringify(result,null,2),{flag:"wx"});
console.log(JSON.stringify({status:result.status,...result.counts,output}));
