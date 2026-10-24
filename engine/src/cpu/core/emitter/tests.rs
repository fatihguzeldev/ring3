use crate::{
    cpu::core::{CompileOptions, compile},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

#[test]
fn memory_heavy_module_keeps_the_complete_program_with_bounded_code_bytes() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let mut instructions = [0x83, 0x03, 1].repeat(24);
    instructions.extend_from_slice(&[0x83, 0xc0, 4, 0xff, 0xe2]);
    memory.write(GuestAddress(0x1000), &instructions).unwrap();
    let code = compile(
        &memory,
        GuestAddress(0x1000),
        CompileOptions {
            instructions: 64,
            blocks: 8,
            wasm_bytes: 1 << 20,
        },
    )
    .unwrap();
    assert_eq!(code.instruction_count(), 26);
    assert_eq!(code.entrypoints(), &[GuestAddress(0x1000)]);
    assert!(code.native_instructions().is_empty());
    assert!(code.is_current(&memory));
    println!(
        "complete 26-instruction module: {} bytes",
        code.wasm().len()
    );
    assert!(
        code.wasm().len() <= 36 * 1024,
        "the complete memory-heavy program needs compact side exits; got {} bytes",
        code.wasm().len()
    );
}

#[test]
fn folded_scalar_flags_recovery_emits_valid_wasm_on_exit_and_memory_fault_paths() {
    use std::{fmt::Write, process::Command};

    let cases: [(&str, &[u8], usize); 3] = [
        ("self-subtract", &[0x2b, 0xc0, 0xff, 0xe6], 2),
        (
            "compare-then-replace",
            &[0x83, 0xf8, 1, 0xb8, 0, 0, 0, 0, 0xff, 0xe6],
            3,
        ),
        (
            "flags-only-input-before-faulting-read",
            &[0x83, 0xf8, 1, 0xb8, 0, 0, 0, 0, 0x8b, 0x13, 0xff, 0xe6],
            4,
        ),
    ];
    let mut modules = String::from("const modules=[");
    for (name, instructions, count) in cases {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0x1000), instructions).unwrap();
        // Exercise the real decoder, mixed program, allocator and emitter.
        // The read's unmapped data address is deliberately unknown at compile
        // time, so its generated precise-fault recovery must also be emitted.
        let code = compile(&memory, GuestAddress(0x1000), CompileOptions::default()).unwrap();
        assert_eq!(code.instruction_count(), count, "complete {name} program");
        write!(&mut modules, "{{name:{name:?},hex:\"").unwrap();
        for byte in code.wasm() {
            write!(&mut modules, "{byte:02x}").unwrap();
        }
        modules.push_str("\"},");
    }
    modules.push_str(
        "];for(const {name,hex} of modules){try{new WebAssembly.Module(Buffer.from(hex,'hex'));}\
         catch(error){throw new Error(name+': '+error.message);}}",
    );
    // This needs only the same Node validator used by the other generated-code
    // tests, never an environment-selected engine.wasm or a mocked owner.
    let result = Command::new("node")
        .args(["-e", &modules])
        .output()
        .expect("validate emitted scalar recovery modules");
    assert!(
        result.status.success(),
        "generated scalar recovery validation failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn callback_spine_emits_only_modes_requested_by_the_authored_program() {
    use std::{fmt::Write, process::Command};

    let cases: [(&str, &[u8], usize, usize); 3] = [
        ("scalar-only", &[0x8d, 0x40, 1, 0xff, 0xe6], 0, 0),
        (
            "typed-fp-only",
            &[0xd9, 0x03, 0xd8, 0xc1, 0xdd, 0x5b, 8, 0xff, 0xe6],
            0,
            1,
        ),
        (
            "canonical-control-and-typed-fp",
            &[0xd9, 0x2b, 0xd9, 0x03, 0xd8, 0xc1, 0xff, 0xe6],
            1,
            1,
        ),
    ];
    let mut script = String::from("const modules=[");
    for (name, instructions, numeric, precise) in cases {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0x1000), instructions).unwrap();
        let code = compile(&memory, GuestAddress(0x1000), CompileOptions::default()).unwrap();
        write!(
            &mut script,
            "{{name:{name:?},numeric:{numeric},precise:{precise},hex:\""
        )
        .unwrap();
        for byte in code.wasm() {
            write!(&mut script, "{byte:02x}").unwrap();
        }
        script.push_str("\"},");
    }
    let inspector = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/cpu/core/fixtures/region/structure.mjs");
    write!(
        &mut script,
        "];(async()=>{{const {{readGeneratedModule}}=await import({:?});",
        inspector.to_str().unwrap()
    )
    .unwrap();
    script.push_str(
        "for(const row of modules){const bytes=Buffer.from(row.hex,'hex');new WebAssembly.Module(bytes);\
         const m=readGeneratedModule(bytes),run=[...m.names].find(([,name])=>name==='guest.run');\
         if(!run)throw new Error(row.name+': missing body');\
         const body=m.functions[run[0]-m.imports.length];\
         for(const [name,wanted]of[['numeric',row.numeric],['precise',row.precise]]){\
           const index=m.imports.findIndex(item=>item.module==='ring3'&&item.name===name);\
           const actual=index<0?0:body.ops.filter(op=>op.op===16&&op.call===index).length;\
           if(actual!==wanted)throw new Error(row.name+': '+name+' sites '+actual+' != '+wanted);\
         }}})().catch(error=>{console.error(error);process.exit(1)});",
    );
    let result = Command::new("node")
        .args(["-e", &script])
        .output()
        .expect("validate authored callback-mode modules");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn rejected_entries_never_load_poisoned_recovery_metadata() {
    use crate::cpu::core::{
        StopReason, compile_observed,
        trace::{BLOCK, TraceEvent},
    };
    use std::{fmt::Write, process::Command};

    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1000), &[0x83, 0x03, 1, 0xff, 0xe6])
        .unwrap();
    let original = compile(&memory, GuestAddress(0x1000), CompileOptions::default()).unwrap();
    let event = TraceEvent {
        kind: BLOCK,
        pc: 0x1000,
        end: 0x1005,
        offset: 0,
        owner: original.identity(),
    };
    let code = compile_observed(
        &memory,
        GuestAddress(0x1000),
        CompileOptions::default(),
        &[event, event],
        &[&original],
        memory.code_source_version(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(code.instruction_count(), 2);
    assert!(code.supports_entry(0x1000));
    assert!(!code.supports_entry(0x1003));
    let mut script = String::from("const owner=Buffer.from('");
    for byte in code.wasm() {
        write!(&mut script, "{byte:02x}").unwrap();
    }
    script.push_str("','hex'),driver=Buffer.from('");
    for byte in crate::cpu::core::dispatch::emit_dispatch() {
        write!(&mut script, "{byte:02x}").unwrap();
    }
    write!(
        &mut script,
        "','hex'),identity={}n,expected=[{},{},{},{},{}];",
        code.identity(),
        StopReason::Infrastructure as u32,
        StopReason::Infrastructure as u32,
        StopReason::Debug as u32,
        StopReason::NeedCode as u32,
        StopReason::CodeInvalidated as u32,
    )
    .unwrap();
    script.push_str(
        r#"
const assert=require('node:assert/strict'),module=new WebAssembly.Module(owner);
assert.deepEqual(WebAssembly.Module.imports(module).filter(x=>x.name==='recovery.base'),
  [{module:'ring3',name:'recovery.base',kind:'global'}]);
assert.equal(WebAssembly.Module.customSections(module,'ring3.recovery').length,1);
const memory=new WebAssembly.Memory({initial:2}),table=()=>new WebAssembly.Table({element:'anyfunc',initial:4097,maximum:4097});
const unexpected=()=>assert.fail('a rejected entry cannot invoke a native service');
const support=new WebAssembly.Instance(new WebAssembly.Module(driver),{ring3:{memory,functions:table(),publishers:table(),
  probe:unexpected,cold:unexpected}}).exports;
const instance=new WebAssembly.Instance(module,{ring3:{...support,memory,read:unexpected,write:unexpected,precise:unexpected,
  'support.abi':support.supportAbi,'recovery.base':new WebAssembly.Global({value:'i32',mutable:false},-1)}});
const view=new DataView(memory.buffer),put=(at,value)=>view.setUint32(at,value>>>0,true),CTX=256;
for(let kind=0;kind<5;kind++){
  new Uint8Array(memory.buffer).fill(0);
  new Uint8Array(memory.buffer,2048,512).fill(0xa5);
  [0x13579bdf,3,0x2468ace0,0x3000,0x4800,0x89abcdef,0x1800,0x76543210].forEach((value,index)=>put(CTX+4*index,value));
  put(CTX+32,kind===3?0x1003:0x1000);put(CTX+36,kind===2?0x102:2);
  put(CTX+40,0x11223344);put(CTX+44,7);put(CTX+80,1024);put(CTX+88,1032);put(CTX+112,kind===0?0:1);
  view.setBigUint64(1024,0x123456789abcdef0n,true);view.setBigUint64(1032,kind===4?12n:11n,true);
  view.setBigUint64(CTX+96,11n,true);view.setBigUint64(CTX+120,identity+(kind===1?1n:0n),true);
  const before=new Uint8Array(memory.buffer).slice();
  assert.equal(instance.exports.run(CTX,7),expected[kind],'the exact entry guard rejects before any poisoned metadata load');
  assert.deepEqual(new Uint8Array(memory.buffer),before,'rejection preserves complete Context, backing, ghosts and authority bytes');
}
"#,
    );
    let result = Command::new("node")
        .args(["-e", &script])
        .output()
        .expect("execute generated owner rejection guards with poisoned metadata");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
