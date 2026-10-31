use super::*;

// these units and the service are wasm functions: a long run cannot borrow a
// javascript recursion trampoline to hide an accumulating dispatcher stack.
fn fixture_module(kind: u32) -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32], [ValType::I32]);
    module.section(&types);
    let mut imports = ImportSection::new();
    imports.import(
        "env",
        "memory",
        EntityType::Memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        }),
    );
    module.section(&imports);
    let mut functions = FunctionSection::new();
    functions.function(0);
    module.section(&functions);
    let mut exports = ExportSection::new();
    exports.export("run", ExportKind::Func, 0);
    module.section(&exports);
    let mut function = Function::new([]);
    let c = &mut function.instructions();
    if kind == 0 {
        // configurable exact instruction service; controls live outside CpuState.
        c.i32_const(0x3000)
            .i32_const(0x3000)
            .i32_load(mem(0))
            .i32_const(1)
            .i32_add()
            .i32_store(mem(0));
        c.i32_const(0x3014)
            .i32_load(mem(0))
            .if_(BlockType::Empty)
            .unreachable()
            .end();
        c.local_get(0)
            .local_get(0)
            .i32_load(mem(48))
            .i32_const(0x3008)
            .i32_load(mem(0))
            .i32_sub()
            .i32_store(mem(48));
        c.local_get(0)
            .local_get(0)
            .i32_load(mem(56))
            .i32_const(0x3018)
            .i32_load(mem(0))
            .i32_add()
            .i32_store(mem(56));
        for (offset, control) in [(40, 0x300c), (52, 0x3010)] {
            c.local_get(0)
                .i32_const(control)
                .i32_load(mem(0))
                .i32_store(mem(offset));
        }
        c.i32_const(0x3004).i32_load(mem(0));
    } else if kind == 1 {
        c.local_get(0).i32_const(7).i32_store(mem(52));
        c.i32_const(7);
    } else if kind == 2 {
        // one ordinary instruction, followed by a handoff to an absent unit.
        for (offset, delta) in [(8, 1), (48, -1), (56, 1)] {
            c.local_get(0)
                .local_get(0)
                .i32_load(mem(offset))
                .i32_const(delta)
                .i32_add()
                .i32_store(mem(offset));
        }
        c.local_get(0).i32_const(0x1080).i32_store(mem(40));
        c.local_get(0).i32_const(0).i32_store(mem(52));
        c.i32_const(0);
    } else if kind == 3 {
        // a return value alone is insufficient to authorize a service call.
        c.i32_const(7);
    } else if kind == 4 {
        c.local_get(0).i32_const(99).i32_store(mem(52));
        c.i32_const(99);
    } else if kind == 5 {
        // a scalar prefix may have consumed the last work item before a bogus 7.
        c.local_get(0).i32_const(0).i32_store(mem(48));
        c.local_get(0).i32_const(7).i32_store(mem(52));
        c.i32_const(7);
    } else {
        for (offset, delta) in [(48, -1), (56, 1)] {
            c.local_get(0)
                .local_get(0)
                .i32_load(mem(offset))
                .i32_const(delta)
                .i32_add()
                .i32_store(mem(offset));
        }
        c.local_get(0).i32_const(7).i32_store(mem(52));
        c.i32_const(7);
    }
    c.end();
    let mut code = CodeSection::new();
    code.function(&function);
    module.section(&code);
    module.finish()
}

#[test]
fn generated_direct_dispatch_services_are_precise_and_bounded() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-dispatch-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&scratch).unwrap();
    fs::write(scratch.join("plain.wasm"), emit_dispatch()).unwrap();
    fs::write(scratch.join("direct.wasm"), emit_dispatch_with_service()).unwrap();
    for (name, kind) in [
        ("service", 0),
        ("instruction", 1),
        ("ordinary", 2),
        ("fake", 3),
        ("invalid", 4),
        ("exhausted", 5),
        ("prefix", 6),
    ] {
        fs::write(scratch.join(format!("{name}.wasm")), fixture_module(kind)).unwrap();
    }
    let hashes: Vec<_> = (0..17)
        .chain([0x1000, 0x1001, 0x00401001, 0x10001001, 0x80000001, u32::MAX])
        .map(|pc| [pc, directory_hash(pc) & 15])
        .collect();
    fs::write(scratch.join("hashes.json"), format!("{hashes:?}")).unwrap();
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:1}),data=new DataView(memory.buffer),ptr=0x8000;
const word=(at,value)=>data.setUint32(ptr+at,value>>>0,true),get=at=>data.getUint32(ptr+at,true),control=(at,value)=>data.setUint32(at,value>>>0,true),counter=()=>data.getUint32(0x3000,true);
const table=new WebAssembly.Table({initial:3,element:'anyfunc'}),modules=new Map();
for(const name of['plain','direct','service','instruction','ordinary','fake','invalid','exhausted','prefix'])modules.set(name,new WebAssembly.Module(fs.readFileSync(`${root}/${name}.wasm`)));
assert.deepEqual(WebAssembly.Module.imports(modules.get('plain')).map(({module,name,kind})=>[module,name,kind]),[['env','memory','memory'],['env','functions','table']]);
assert.deepEqual(WebAssembly.Module.imports(modules.get('direct')).map(({module,name,kind})=>[module,name,kind]),[['env','memory','memory'],['env','functions','table'],['cpu','instruction','function']]);
const unit=name=>new WebAssembly.Instance(modules.get(name),{env:{memory}}).exports.run;
const service=unit('service'),instruction=unit('instruction'),ordinary=unit('ordinary');
const dispatch=new WebAssembly.Instance(modules.get('direct'),{env:{memory,functions:table},cpu:{instruction:service}}).exports.run;
const plain=new WebAssembly.Instance(modules.get('plain'),{env:{memory,functions:table}}).exports.run;
function directoryHash(pc){let value=pc>>>0;value=Math.imul(value^(value>>>16),0x7feb352d);value=Math.imul(value^(value>>>15),0x846ca68b);return(value^(value>>>16))>>>0;}
function init(quota=4){new Uint8Array(memory.buffer).fill(0);word(0,1);word(4,160);[41,2,3,4,0x4000,6,7,8].forEach((v,i)=>word(8+4*i,v));word(40,0x1000);word(44,0x202);word(48,quota);word(144,0x9000);word(148,15);control(0x3008,1);control(0x300c,0x1000);control(0x3018,1);table.set(1,instruction);table.set(2,ordinary);entry(0x1000,1);}
function entry(pc,slot){let index=directoryHash(pc)&15;while(data.getUint32(0x9004+8*index,true))index=(index+1)&15;data.setUint32(0x9000+8*index,pc,true);data.setUint32(0x9004+8*index,slot,true);}
function serviceStatus(status,{progress=0,slab=status,retired=progress,next=0x1000}={}){control(0x3004,status);control(0x3008,progress);control(0x300c,next);control(0x3010,slab);control(0x3018,retired);}
function scalar(){return Array.from({length:8},(_,i)=>get(8+4*i));}
init();assert.equal(plain(ptr),7);assert.equal(get(52),7);assert.equal(counter(),0);assert.equal(get(48),4);assert.equal(get(56),0);
init();serviceStatus(0,{progress:1,next:0x1040});entry(0x1040,2);assert.equal(dispatch(ptr),1);assert.equal(get(52),1);assert.equal(get(40),0x1080);assert.equal(get(8),42);assert.equal(get(48),2);assert.equal(get(56),2);assert.equal(counter(),1);assert.equal(get(44),0x202);
// native placement and emitted lookup must agree on odd, high and endpoint PCs.
for(const[pc,index]of JSON.parse(fs.readFileSync(`${root}/hashes.json`))){
  assert.equal(directoryHash(pc)&15,index);
  init(1);new Uint8Array(memory.buffer,0x9000,128).fill(0);word(40,pc);entry(pc,1);serviceStatus(0,{progress:1,next:pc});
  assert.equal(dispatch(ptr),2);assert.equal(get(52),2);assert.equal(get(40),pc);assert.equal(get(48),0);assert.equal(get(56),1);assert.equal(counter(),1);assert.equal(get(8),41);assert.equal(get(44),0x202);
}
// seventeen distinct keys force a real collision in sixteen initial buckets.
let collision;const buckets=new Map();
for(let pc=0;pc<=16;pc++){const index=directoryHash(pc)&15;if(buckets.has(index)){collision=[buckets.get(index),pc];break;}buckets.set(index,pc);}
assert(collision);assert.notEqual(collision[0],collision[1]);assert.equal(directoryHash(collision[0])&15,directoryHash(collision[1])&15);
init();new Uint8Array(memory.buffer,0x9000,128).fill(0);word(40,collision[0]);entry(collision[0],1);entry(collision[1],2);serviceStatus(0,{progress:1,next:collision[1]});
assert.equal(dispatch(ptr),1);assert.equal(get(52),1);assert.equal(get(40),0x1080);assert.equal(get(8),42);assert.equal(get(48),2);assert.equal(get(56),2);assert.equal(counter(),1);assert.equal(get(44),0x202);
init(1);serviceStatus(0,{progress:1});assert.equal(dispatch(ptr),2);assert.equal(get(52),2);assert.equal(get(48),0);assert.equal(get(56),1);assert.equal(counter(),1);
init(0);assert.equal(dispatch(ptr),2);assert.equal(get(52),2);assert.equal(counter(),0);
for(const[status,progress]of[[3,0],[4,0],[6,1],[8,0],[9,0],[9,1],[10,1]]){init();const before=scalar();serviceStatus(status,{progress,next:progress?0x1002:0x1000});assert.equal(dispatch(ptr),status,`terminal${status}`);assert.equal(get(52),status);assert.equal(counter(),1);assert.equal(get(48),4-progress);assert.equal(get(56),progress);assert.equal(get(40),progress?0x1002:0x1000);assert.deepEqual(scalar(),before);}
for(const progress of[0,-1]){init();serviceStatus(0,{progress,retired:0});assert.equal(dispatch(ptr),8);assert.equal(get(52),8);assert.equal(counter(),1);assert.equal(get(56),0);}
init();table.set(1,unit('prefix'));serviceStatus(0,{progress:0});assert.equal(dispatch(ptr),8);assert.equal(get(52),8);assert.equal(counter(),1);assert.equal(get(48),3);assert.equal(get(56),1);
for(const status of[1,2,5,7,11,99,-1]){init();serviceStatus(status);assert.equal(dispatch(ptr),8,`invalidservice${status}`);assert.equal(get(52),8);assert.equal(counter(),1);}
for(const[status,slab]of[[0,7],[3,4],[8,7],[10,9]]){init();serviceStatus(status,{slab,progress:status===0?1:0});assert.equal(dispatch(ptr),8);assert.equal(get(52),8);assert.equal(counter(),1);}
for(const name of['fake','invalid','exhausted']){init();table.set(1,unit(name));assert.equal(dispatch(ptr),8,name);assert.equal(get(52),8);assert.equal(counter(),0);assert.equal(get(56),0);}
init();control(0x3014,1);assert.throws(()=>dispatch(ptr),WebAssembly.RuntimeError);assert.equal(counter(),1);assert.equal(get(48),4);assert.equal(get(56),0);assert.equal(get(52),7);
init(100000);serviceStatus(0,{progress:1});assert.equal(dispatch(ptr),2);assert.equal(get(52),2);assert.equal(counter(),100000);assert.equal(get(48),0);assert.equal(get(56),100000);assert.equal(get(40),0x1000);assert.equal(get(8),41);
console.log('direct Wasm service handoff/budget/fault/SMC, fail-closed fake/status/progress, one-shot trap and100k bounded calls passed');
"#).arg(&scratch).output().unwrap();
    assert!(
        result.status.success(),
        "fixture {} failed:\n{}\n{}",
        scratch.display(),
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    fs::remove_dir_all(scratch).unwrap();
}
