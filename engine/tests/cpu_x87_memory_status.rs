use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};

#[test]
fn bare_fnstsw_memory_admits_via_existing_public_api() {
    let bytes = [0xdd, 0x38, 0xeb, 0];
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(&bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0x1000_u32.to_le_bytes(), 4_u32.to_le_bytes()].concat());
    engine.compile(1).expect("bare FNSTSW memory must admit");
}

use ring3_engine::{
    cpu::{
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, compile_entry_region,
            compile_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{EffectiveAddress, Operation},
        },
    },
    memory::{FaultReason, GuestAddress},
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

const PC: u32 = 0x1000;
const KEY: u64 = 0x1234_5678_9abc_def0;

fn code_at(pc: u32, bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    engine.map(base, pages, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
    engine.protect(base, pages, 4).unwrap();
    engine
}

fn request(engine: &mut EngineInstance, length: u32, entries: bool) {
    let words = if entries { vec![PC] } else { vec![PC, length] };
    for (index, value) in words.iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
}

#[test]
fn status_memory_decodes_standard_32_bit_effective_addresses() {
    use Register32::{Eax, Ebp, Ecx, Edi, Esp};
    for (bytes, base, index, scale, displacement) in [
        (vec![0xdd, 0x38], Some(Eax), None, 1, 0),
        (vec![0xdd, 0x39], Some(Ecx), None, 1, 0),
        (vec![0xdd, 0x3c, 0x24], Some(Esp), None, 1, 0),
        (vec![0xdd, 0x7d, 0xf0], Some(Ebp), None, 1, 0xffff_fff0),
        (
            vec![0xdd, 0xbf, 0x78, 0x56, 0x34, 0x12],
            Some(Edi),
            None,
            1,
            0x1234_5678,
        ),
        (vec![0xdd, 0x3c, 0x88], Some(Eax), Some(Ecx), 4, 0),
        (
            vec![0xdd, 0x3c, 0xcd, 0x78, 0x56, 0x34, 0x12],
            None,
            Some(Ecx),
            8,
            0x1234_5678,
        ),
        (
            vec![0xdd, 0x3d, 0x78, 0x56, 0x34, 0x12],
            None,
            None,
            1,
            0x1234_5678,
        ),
    ] {
        let engine = code_at(PC, &bytes);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(PC)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::X87StatusToMemory {
                address: EffectiveAddress {
                    base,
                    index,
                    scale,
                    displacement
                }
            }
        );
        assert_eq!(decoded.length() as usize, bytes.len());
        assert_eq!(decoded.next_pc(), GuestAddress(PC + bytes.len() as u32));
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
    }
    let engine = code_at(0x1fff, &[0xdd]);
    assert!(
        matches!(decode_one(engine.memory().unwrap(), GuestAddress(0x1fff)), Err(DecodeError::MemoryFault { fault, length: 2, .. }) if fault.reason == FaultReason::Unmapped && fault.address == GuestAddress(0x2000))
    );
    let engine = code_at(0x1fff, &[0xdd, 0x38]);
    assert_eq!(
        decode_one(engine.memory().unwrap(), GuestAddress(0x1fff))
            .unwrap()
            .next_pc(),
        GuestAddress(0x2001)
    );
}

#[test]
fn waited_prefixed_neighbors_and_standalone_memory_stay_refused() {
    for bytes in [
        vec![0x9b],
        vec![0x9b, 0xdd, 0x38],
        vec![0xd9, 0x38],
        vec![0xd9, 0xe8],
        vec![0xdd, 0xd8],
        vec![0xd8, 0xc1],
    ] {
        let engine = code_at(PC, &bytes);
        assert!(
            decode_one(engine.memory().unwrap(), GuestAddress(PC)).is_err(),
            "{bytes:02x?}"
        );
    }
    for prefix in [
        0x66, 0x67, 0xf0, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65,
    ] {
        let engine = code_at(PC, &[prefix, 0xdd, 0x38]);
        assert!(
            decode_one(engine.memory().unwrap(), GuestAddress(PC)).is_err(),
            "prefix{prefix:02x}"
        );
    }
    let engine = code_at(PC, &[0xdd, 0x38, 0xeb, 0]);
    for result in [
        compile_region(
            engine.memory().unwrap(),
            &[BlockSpec {
                entry: GuestAddress(PC),
                byte_length: 4,
            }],
            CompileLimits::default(),
        ),
        compile_entry_region(
            engine.memory().unwrap(),
            &[GuestAddress(PC)],
            CompileLimits::default(),
        ),
    ] {
        assert!(matches!(
            result,
            Err(CompileError::Instruction {
                pc: GuestAddress(PC),
                cause: InstructionError::BackendUnsupported
            })
        ));
    }
}

#[test]
fn four_bound_profiles_validate_word_store_and_shared_helper_types() {
    let mut input = Vec::new();
    for (mixed, bytes) in [
        (false, vec![0xdd, 0x38, 0xeb, 0]),
        (
            true,
            vec![
                0xdd, 0x38, 0x0f, 0xb7, 0x11, 0x8a, 0x03, 0x88, 0x03, 0xeb, 0,
            ],
        ),
    ] {
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = code_at(PC, &bytes);
                request(&mut engine, bytes.len() as u32, entries);
                let before = engine.arena().to_vec();
                let module = if resident {
                    let id = if entries {
                        engine.compile_resident_entries(1, 0)
                    } else {
                        engine.compile_resident(1)
                    }
                    .unwrap()
                    .get();
                    engine.guard_resident(KEY, id).unwrap();
                    engine.resident_bytes(id).unwrap().to_vec()
                } else {
                    let generation = if entries {
                        engine.compile_entries(1, 0)
                    } else {
                        engine.compile(1)
                    }
                    .unwrap();
                    engine.guard(KEY, generation).unwrap();
                    engine.artifact_bytes().unwrap().to_vec()
                };
                assert_eq!(engine.arena(), before);
                input.extend([u8::from(mixed), u8::from(resident)]);
                input.extend((module.len() as u32).to_le_bytes());
                input.extend(module);
            }
        }
    }
    let mut child = Command::new("node").args(["-e", r#"
const assert = require('node:assert/strict'), input = require('node:fs').readFileSync(0);
let at=0, count=0;
while(at<input.length) {
  const mixed=input[at++], resident=input[at++], length=input.readUInt32LE(at); at+=4;
  const bytes=input.subarray(at,at+length); at+=length;
  assert.ok(WebAssembly.validate(bytes)); const module=new WebAssembly.Module(bytes);
  const names=[resident?'guard_resident':'guard',...(mixed?['read8','read16',resident?'store_resident8':'store8']:[]),resident?'store_resident16':'store16'];
  assert.deepEqual(WebAssembly.Module.imports(module),[{module:'env',name:'memory',kind:'memory'},...names.map(name=>({module:'ring3',name,kind:'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]); count++;
}
assert.equal(count,8);
"#]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn late_refusal_preserves_existing_owners_and_entire_arena() {
    let mut engine = code_at(PC, &[0x90, 0xeb, 0]);
    request(&mut engine, 3, false);
    let generation = engine.compile(1).unwrap();
    let id = engine.compile_resident(1).unwrap().get();
    engine.map(0x3000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
        .copy_from_slice(&[0xdd, 0x38, 0x0f, 0x0b]);
    engine.upload(0x3000, 4).unwrap();
    for resident in [false, true] {
        for entries in [false, true] {
            let descriptor = if entries {
                vec![0x3000_u32]
            } else {
                vec![0x3000_u32, 4]
            };
            for (index, value) in descriptor.iter().enumerate() {
                engine.arena_mut().unwrap()
                    [TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
                    .copy_from_slice(&value.to_le_bytes());
            }
            let before = (
                engine.arena().to_vec(),
                engine.artifact_bytes().unwrap().to_vec(),
                engine.resident_bytes(id).unwrap().to_vec(),
            );
            assert!(match (resident, entries) {
                (false, false) => engine.compile(1).is_err(),
                (false, true) => engine.compile_entries(1, 0).is_err(),
                (true, false) => engine.compile_resident(1).is_err(),
                (true, true) => engine.compile_resident_entries(1, 0).is_err(),
            });
            assert_eq!(
                (
                    engine.arena().to_vec(),
                    engine.artifact_bytes().unwrap().to_vec(),
                    engine.resident_bytes(id).unwrap().to_vec()
                ),
                before
            );
            assert_eq!(engine.generation(), generation);
            engine.guard(KEY, generation).unwrap();
            engine.guard_resident(KEY, id).unwrap();
        }
    }
}

#[test]
fn generated_word_store_rejects_noncanonical_packets_before_cpu_publication() {
    let mut input = Vec::new();
    for resident in [false, true] {
        let mut engine = code_at(PC, &[0xdd, 0x38, 0xeb, 0]);
        request(&mut engine, 4, false);
        let module = if resident {
            let id = engine.compile_resident(1).unwrap().get();
            engine.resident_bytes(id).unwrap().to_vec()
        } else {
            engine.compile(1).unwrap();
            engine.artifact_bytes().unwrap().to_vec()
        };
        input.push(u8::from(resident));
        input.extend((module.len() as u32).to_le_bytes());
        input.extend(module);
    }
    let mut child = Command::new("node").args(["-e", r#"
const assert=require('node:assert/strict'), input=require('node:fs').readFileSync(0);
function record(magic,size,fields=[],version=1) {
  const b=Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version,4); b.writeUInt16LE(1,6); b.writeUInt32LE(size,8);
  fields.forEach((v,i)=>b.writeUInt32LE(v>>>0,16+i*4)); return b;
}
const success=[0,0,0,0,0,2], fault=[1,0,1,0x5008,2,2], infra=[2,0,1,0,0,2];
const rows=[];
function add(label,fields,reason,detail=0,address=0,access=0,length=0,status=0,ea=0x5008,retired=0,change) {
  const packet=record('R3MH',40,fields,4); if(change)change(packet);
  rows.push({label,packet,reason,detail,address,access,length,status,ea,retired});
}
add('success',success,1,0,0,0,0,0,0x5008,1);
add('same-code-write completion',success,6,0,0,0,0,11,0x5008,1);
add('top valid two-byte success',success,1,0,0,0,0,0,0xfffffffe,1);
add('first-byte fault',fault,5,1,0x5008,2,2);
add('second-byte permission fault',[1,0,2,0x5009,2,2],5,2,0x5009,2,2);
add('top nonoverflow second-byte fault',[1,0,2,0xffffffff,2,2],5,2,0xffffffff,2,2,0,0xfffffffe);
add('overflow fault',[1,0,3,0xffffffff,2,2],5,3,0xffffffff,2,2,0,0xffffffff);
add('version exhaustion',infra,7,1);
add('helper rejected',[2,0,2,0,0,2],7,3);
for(const [label,offset,value,width] of [
  ['magic',0,0,4],['old version3',4,3,2],['profile',6,2,2],['size',8,39,4],['reserved',12,1,4],
  ['value',20,1,4],['width1',36,1,4],['width4',36,4,4],['success detail',24,1,4],
  ['success address',28,0x5008,4],['success access',32,2,4],['unknown result',16,3,4]
])add(label,success,7,2,0,0,0,0,0x5008,0,b=>width===2?b.writeUInt16LE(value,offset):b.writeUInt32LE(value,offset));
for(const [label,fields,status,ea] of [
  ['detail0',[1,0,0,0x5008,2,2],0,0x5008],['detail4',[1,0,4,0x5008,2,2],0,0x5008],
  ['read access',[1,0,1,0x5008,1,2],0,0x5008],['before span',[1,0,1,0x5007,2,2],0,0x5008],
  ['after span',[1,0,1,0x500a,2,2],0,0x5008],['wrapped zero near top',[1,0,1,0,2,2],0,0xfffffffe],
  ['nonoverflow claim at overflowing start',[1,0,1,0xffffffff,2,2],0,0xffffffff],
  ['overflow claim at valid start',[1,0,3,0xfffffffe,2,2],0,0xfffffffe],
  ['overflow wrong point',[1,0,3,0,2,2],0,0xffffffff],['status11 fault',fault,11,0x5008],
  ['status11 infra',infra,11,0x5008],['infra address',[2,0,1,0x5008,0,2],0,0x5008],
  ['infra access',[2,0,1,0,2,2],0,0x5008],['infra detail0',[2,0,0,0,0,2],0,0x5008],
  ['infra detail3',[2,0,3,0,0,2],0,0x5008],['forged success overflows',success,0,0xffffffff]
])add(label,fields,7,2,0,0,0,status,ea);
add('host status9',success,7,3,0,0,0,9);
assert.equal(rows.length,38);
let at=0, modules=0, checks=0;
while(at<input.length) {
  const resident=input[at++], size=input.readUInt32LE(at); at+=4;
  const module=new WebAssembly.Module(input.subarray(at,at+size)); at+=size; modules++;
  for(const row of rows) {
    const memory=new WebAssembly.Memory({initial:1}), bytes=new Uint8Array(memory.buffer), base=128;
    const regs=[row.ea,0x12345678,0x23456789,0x3456789a,0x456789ab,0x56789abc,0x6789abcd,0x789abcde];
    bytes.set(record('R3ST',56,[...regs,0x1000,0xcd7]),base);
    bytes.set(record('R3EX',40,[3,0,0,0,0,0]),base+56);
    const fp=record('R3FP',128); fp.writeUInt16LE(0x81a5,18); fp.writeUInt16LE(0x55aa,20); fp.writeUInt16LE(0x7ff,22);
    fp.writeUInt32LE(0xf1234567,24); fp.writeUInt32LE(0xfedcba98,28); fp.writeUInt16LE(0xf135,32); fp.writeUInt16LE(0xe246,34);
    fp.set(Buffer.from(Array.from({length:80},(_,i)=>(i*73+29)&255)),40); bytes.set(fp,base+4236);
    const before=Buffer.from(bytes), expected=Buffer.from(before); let calls=0;
    const store=(...args)=>{calls++; const [address,value]=args.slice(resident?4:0); assert.equal(address>>>0,row.ea); assert.equal(value,0x81a5); bytes.set(row.packet,base+100); return row.status;};
    const run=new WebAssembly.Instance(module,{env:{memory},ring3:resident?{guard_resident:()=>0,store_resident16:store}:{guard:()=>0,store16:store}}).exports.run;
    expected.set(row.packet,base+100); expected.writeUInt32LE(row.retired?0x1002:0x1000,base+48);
    expected.set(record('R3EX',40,[row.reason,row.retired,row.detail,row.address,row.access,row.length],2),base+56);
    assert.equal(run(base,base+56,1,base+96),0,row.label); assert.equal(calls,1);
    assert.deepEqual(Buffer.from(bytes),expected,`${resident}/${row.label}: exact memory/CPU/FPU publication`); checks++;
  }
}
assert.equal(modules,2); assert.equal(checks,76);
"#]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
