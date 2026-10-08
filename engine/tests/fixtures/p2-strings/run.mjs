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
for (const [offset, bytes] of [[0,[0xa4,0xeb,0]],[16,[0xfc,0xeb,0]],[32,[0xfd,0xeb,0]],
  [48,[0xfd,0xa4,0xfc,0xa4,0xeb,0]]]) bank.set(bytes, offset);
assert.deepEqual(bank, readFileSync(join(output, "strings.x86")), "independent Rust/JS source bank");
const sourcePaths = ["engine/src/abi/arena.rs", "engine/src/cpu/x86/ir.rs", "engine/src/cpu/x86/decode/lower.rs",
  "engine/src/cpu/x86/decode/profile.rs", "engine/src/cpu/dbt/region.rs",
  "engine/src/cpu/dbt/wasm/integer.rs", "engine/src/cpu/dbt/wasm/memory.rs",
  "engine/src/cpu/dbt/wasm/memory/narrow.rs", "engine/src/cpu/dbt/wasm/memory/byte_store.rs",
  "engine/tests/cpu_strings_wasm.rs", "engine/tests/fixtures/p2-strings/run.mjs",
  "engine/tests/fixtures/support/engine.mjs"];
const sourcePins = () => Object.fromEntries(sourcePaths.map(path => {
  const bytes = readFileSync(join(root, path)); return [path, { bytes: bytes.length, sha256: hash(bytes) }];
}));
const beforeSources = sourcePins(), frames = [], journal = [], modules = [], cases = [];
let generated = 0, retired = 0, copies = 0;
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
function seed(ctx, pc, flags, source = SRC + 8, destination = DST + 8) {
  const registers = [0x1234807f,0xa1b2c378,0x34560f10,0x4567ff00,0x56789abc,0x6789abcd,source,destination];
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
function copy(ctx, label, value, reason = 1) {
  const before = cpu(ctx), delta = before.flags & 0x400 ? -1 : 1;
  assert.equal(read(ctx,before.registers[6]),value,"source byte matches declared upload input");
  const next = {registers:[...before.registers],pc:(before.pc+1)>>>0,flags:before.flags};
  next.registers[6] = (next.registers[6] + delta)>>>0; next.registers[7] = (next.registers[7] + delta)>>>0;
  const row = run(ctx,1,label,next,reason,1,helper(3)); copies++;
  assert.equal(read(ctx,before.registers[7]),value,"physical destination byte");
  cases.push({owner:ctx.owner,label,before,next,value,run:row}); return next;
}
function open(owner, ordinal) {
  const instance = new WebAssembly.Instance(engine.module, {}), ctx = {owner,memory:instance.exports.memory,api:{}};
  for (const name of ["open","close","arena_ptr","map","unmap","protect","upload","read8","compile",
    "compile_resident","generation","module_ptr","module_len","guard","guard_resident","store8","store_resident8"])
    ctx.api[name] = instance.exports["ring3_abi_v1_"+name];
  assert.equal(ctx.api.open(8,ordinal,0x51525354),0); ctx.base = ctx.api.arena_ptr()>>>0;
  for (const address of [PC,SRC,DST,0xfffff000,0]) host(ctx,"map",[address,1,address === PC ? 7 : 3]);
  upload(ctx,PC,bank);
  input(ctx,140,words([PC,3,PC+16,3,PC+32,3,PC+48,6]),"four compiler block descriptors");
  let binding;
  if (owner === "replacement") {
    host(ctx,"compile",[4]); binding = {generation:ctx.api.generation(),pointer:ctx.api.module_ptr()>>>0,length:ctx.api.module_len()>>>0};
  } else {
    host(ctx,"compile_resident",[4]); const receipt = arena(ctx);
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
  for (const flags of [2,0x8d7,0xcd7]) for (const value of [0,1,0x80,0xff]) {
    upload(ctx,SRC+8,Buffer.from([value])); upload(ctx,DST+8,Buffer.from([0x55]));
    seed(ctx,PC,flags); copy(ctx,"DF/current byte",value);
    run(ctx,2,"JMP then cold continuation",{...cpu(ctx),pc:PC+3},3,1);
  }
  for (const [source,destination,flags] of [[SRC+8,SRC+8,2],[SRC+8,SRC+9,0xcd7],
    [0xffffffff,DST+8,2],[SRC+8,0xffffffff,2],[0,DST+8,0xcd7],[SRC+8,0,0xcd7]]) {
    upload(ctx,source,Buffer.from([0xa6])); seed(ctx,PC,flags,source,destination); copy(ctx,"overlap or wrapping pointer",0xa6);
  }
  for (const [pc,set] of [[PC+16,false],[PC+32,true]]) for (const flags of [2,0xcd7]) {
    const before = seed(ctx,pc,flags), next = {...before,pc:pc+1,flags:set ? flags|0x400 : flags&~0x400};
    run(ctx,1,"CLD/STD preserves non-DF state",next);
  }
  upload(ctx,SRC+8,Buffer.from([0x23])); upload(ctx,SRC+7,Buffer.from([0x91]));
  seed(ctx,PC+48,2);
  run(ctx,1,"real STD before copy",{...cpu(ctx),pc:PC+49,flags:0x402});
  copy(ctx,"first retained-CPU DF1 MOVSB",0x23);
  run(ctx,1,"real CLD before copy",{...cpu(ctx),pc:PC+51,flags:2});
  copy(ctx,"second retained-CPU DF0 MOVSB",0x91);
  assert.equal(read(ctx,DST+8),0x23); assert.equal(read(ctx,DST+7),0x91);
  const faults = [
    {source:0x7008,destination:DST+8,address:0x7008,access:1,detail:1},
    {source:0x7008,destination:DST+8,address:0x7008,access:1,detail:2,permission:2},
    {source:SRC+8,destination:0x8008,address:0x8008,access:2,detail:1},
    {source:SRC+8,destination:0x8008,address:0x8008,access:2,detail:2,permission:1},
    {source:0x7008,destination:0x8008,address:0x7008,access:1,detail:1},
  ];
  for (const fault of faults) {
    if (fault.permission) host(ctx,"map",[fault.address&~0xfff,1,fault.permission]);
    if (fault.access === 1 && fault.destination === DST+8) upload(ctx,DST+8,Buffer.from([0x5a]));
    const before = seed(ctx,PC,0xcd7,fault.source,fault.destination);
    for (let attempt = 0; attempt < 2; attempt++) run(ctx,1,"fault then retained CPU retry",before,5,0,
      helper(fault.access === 1 ? 2 : 3,0,fault.detail,fault.address,fault.access),fault);
    if (fault.access === 1 && fault.destination === DST+8)
      assert.equal(read(ctx,DST+8),0x5a,"both failed source reads leave mapped destination sentinel unchanged");
    if (fault.permission) host(ctx,"protect",[fault.address&~0xfff,1,3]);
    else {
      for (const address of new Set([fault.source,fault.destination].filter(value=>value>=0x7000&&value<0x9000).map(value=>value&~0xfff)))
        host(ctx,"map",[address,1,3]);
    }
    upload(ctx,fault.source,Buffer.from([0x81]));
    assert.deepEqual(cpu(ctx),before,"fault repair changes no CPU input"); copy(ctx,"repaired fault same owner and CPU",0x81);
    for (const address of [0x7000,0x8000]) if ([fault.source&~0xfff,fault.destination&~0xfff].includes(address)) host(ctx,"unmap",[address,1]);
  }
  seed(ctx,PC,2,0x7008,0x8008);
  run(ctx,0,"zero budget avoids both unmapped addresses",cpu(ctx),1,0);
  input(ctx,96,words([1]),"explicit cancellation"); run(ctx,1,"cancel avoids both unmapped addresses",cpu(ctx),2,0);
  upload(ctx,SRC+8,Buffer.from([0xfc])); seed(ctx,PC,2,SRC+8,PC+1);
  copy(ctx,"code store commits pointers before invalidation",0xfc,6);
  const held = Buffer.from(new Uint8Array(ctx.memory.buffer,ctx.binding.pointer,ctx.binding.length));
  run(ctx,1,"stale known owner preserves full arena",cpu(ctx),1,0,undefined,null,4);
  assert.deepEqual(Buffer.from(new Uint8Array(ctx.memory.buffer,ctx.binding.pointer,ctx.binding.length)),held,
    "immediate stale callback preserves live allocation before any getter or allocation");
  host(ctx,"close",[]); run(ctx,1,"closed known arena preserves full arena",cpu(ctx),1,0,undefined,null,5);
}
assert.deepEqual(sourcePins(),beforeSources,"fixture source bytes unchanged during execution");
const raw = Buffer.concat(frames); writeFileSync(join(output,"arenas.bin"),raw,{flag:"wx"});
const result = {status:"ok",engine:{bytes:engine.bytes.length,sha256:engine.sha256},source_pins:beforeSources,
  counts:{owners:2,modules:modules.length,movsb:copies,generated_calls:generated,retired,raw_frames:frames.length},
  modules,cases,journal,raw:{file:"arenas.bin",bytes:raw.length,sha256:hash(raw)},
  limits:["finite MOVSB/CLD/STD cases; REP and wider strings excluded",
    "native tests validate/decode modules; this fixture runs actual engine-generated Wasm",
    "full saved arena observations, selected physical byte diagnostics; no full RAM or generic body certification",
    "stale allocation comparison is live before close; closed owners have no allocation lifetime claim"]};
writeFileSync(join(output,"result.json"),JSON.stringify(result,null,2),{flag:"wx"});
console.log(JSON.stringify({status:result.status,...result.counts,output}));
