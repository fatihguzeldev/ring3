use super::*;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};
use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, FunctionSection, GlobalType, ImportSection,
    MemoryType, Module, TypeSection,
};

fn module() -> Vec<u8> {
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 4], [ValType::I32]);
    types.ty().function([ValType::I32; 2], [ValType::I64]);
    let mut imports = ImportSection::new();
    imports.import(
        "test",
        "memory",
        EntityType::Memory(MemoryType {
            minimum: 0,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        }),
    );
    imports.import(
        "test",
        "continuation",
        EntityType::Global(GlobalType {
            val_type: ValType::I32,
            mutable: false,
            shared: false,
        }),
    );
    let mut declarations = FunctionSection::new();
    declarations.function(0).function(1);
    let mut exports = ExportSection::new();
    exports
        .export("recover", ExportKind::Func, 0)
        .export("word", ExportKind::Func, 1);
    let mut code = CodeSection::new();
    code.function(&emit(1, 0)).function(&emit_word());
    let mut m = Module::new();
    m.section(&types)
        .section(&imports)
        .section(&declarations)
        .section(&exports)
        .section(&code);
    m.finish()
}
fn hex(bytes: &[u8]) -> String {
    let mut s = String::new();
    for byte in bytes {
        write!(&mut s, "{byte:02x}").unwrap();
    }
    s
}
fn pair(a: u32, b: u32) -> i64 {
    (u64::from(a) | (u64::from(b) << 32)) as i64
}
fn scalar() -> Vec<i64> {
    // one row computes inputEAX+32, staging the old input before any output.
    vec![
        9,
        0,
        0,
        0,
        0,
        0,
        pair(12, 3),
        pair(3, 2),
        2,
        0,
        1,
        7,
        pair(1, 2),
        3, // closure markers1,2,3
        pair(0, 0),
        0,
        0,
        0, // available color0
        pair(2, 1),
        32,
        0,
        0, // constant color1
        pair(3, 2),
        0,
        pair(1, 2),
        0,
    ] // add color2
}
fn tagged() -> Vec<i64> {
    // VALID0 requests raw lanes and must never capture native color1.
    vec![
        15,
        pair(1 << 16 | 1 << 8, 0),
        -1,
        0,
        0,
        0,
        pair(21, 4),
        pair(1, 0),
        i64::from(1u32 << 16 | 1 << 24),
        pair(2, 1),
        (u64::from(76u32 | 2 << 16 | 1 << 18 | 1 << 24) | (1u64 << 32)) as i64,
        pair(3, 2),
        (u64::from(110u32 | 3 << 16 | 1 << 18 | 1 << 24) | (1u64 << 32)) as i64,
        pair(4, 3),
        (u64::from(118u32 | 3 << 16 | 1 << 18 | 1 << 24) | (1u64 << 32)) as i64,
        0,
        4,
        7,
        9,
        11,
        13, // list
        pair(1, 2),
        pair(3, 4), // closure
        pair(0, 0),
        0,
        0,
        0,
        pair(0 | 2 << 8, 1),
        0,
        0,
        0,
        pair(0 | 1 << 8, 2),
        0,
        0,
        0,
        pair(0, 3),
        0,
        0,
        0,
    ]
}
#[test]
fn shared_helper_authentication_two_phase_bits_and_epoch_wrap_execute_real_wasm() {
    let module = hex(&module());
    let scalar = hex(&super::super::data::encode(&scalar(), 7));
    let tagged = hex(&super::super::data::encode(&tagged(), 7));
    let script = format!(
        r#"
import assert from 'node:assert/strict';
const memory=new WebAssembly.Memory({{initial:32}}),v=new DataView(memory.buffer),bytes=new Uint8Array(memory.buffer);
const P=0x10000,H=0x2000,C=0x1000,B=0x40000,O=0x3000,S=0x4000,G=0x5000,L=0x5800,BANK=0x6000;
const put=(at,x)=>v.setUint32(at,x>>>0,true),wide=(at,x)=>v.setBigUint64(at,BigInt.asUintN(64,BigInt(x)),true);
const get=at=>v.getUint32(at,true),bits=at=>v.getBigUint64(at,true),cell=c=>P+1024+16*c;
const instance=new WebAssembly.Instance(new WebAssembly.Module(Buffer.from('{module}','hex')),{{test:{{memory,continuation:new WebAssembly.Global({{value:'i32',mutable:false}},H)}}}});
const recover=(...args)=>instance.exports.recover(...(args.length?args:[C,B,1,P]));
function setup(blob,colors,recipeBase,recipeCount){{
  bytes.fill(0);bytes.set(Buffer.from(blob,'hex'),B);
  for(const[o,x]of[[0,0x5233504b],[4,1],[8,132096],[12,8192],[16,C],[20,H],[40,1024],
    [56,7],[64,2],[68,colors],[88,B],[92,1],[96,recipeBase],[100,recipeCount],[104,7],[108,180]])put(P+o,x);
  wide(P+24,0x12345678n);wide(P+32,9);wide(P+56,7);wide(P+72,11);wide(P+80,9);
  for(const[o,x]of[[0,2],[4,1],[8,C],[36,O],[40,1],[44,L],[48,S],[52,G],[56,3],[72,1],[76,2]])put(H+o,x);
  wide(H+16,9);wide(H+24,0x12345678n);wide(H+80,7);wide(H+88,11);wide(H+104,9);
  wide(O,7);wide(O+8,11);put(O+24,2);put(O+36,1);put(S+8,0);wide(L+16,7);wide(G+16,11);
  put(C+92,BANK);put(P+128,BANK);put(P+132,BANK+512);put(P+140,99);put(P+144,10);
}}
function capture(c,marker,value){{assert.equal(get(cell(c)+8),marker);assert.equal(get(cell(c)+12),get(P+52));wide(cell(c),value);put(cell(c)+8,-marker);}}
const architecture=()=>Buffer.concat([Buffer.from(bytes.slice(C,C+128)),Buffer.from(bytes.slice(BANK,BANK+896))]);
setup('{scalar}',3,14,3);
const arch=architecture();assert.equal(recover(),1);assert.equal(get(P+48),1);assert.equal(get(cell(0)+8),1);
capture(0,1,0x7fffffc0);assert.equal(recover(),2);assert.equal(recover(),0);
assert.equal(bits(P+152),1n);assert.equal(bits(P+160),0x7fffffe0n);assert.deepEqual(architecture(),arch);
// each forged authority rejects before architectural mutation; restore only the exact probe.
for(const[offset,value,isWide]of[[56,8,true],[64,1,false],[72,12,true],[80,10,true],[16,C+4,false],[88,B+1,false],[92,2,false]]){{
  setup('{scalar}',3,14,3);const saved=isWide?bits(P+offset):get(P+offset),before=architecture();
  if(isWide)wide(P+offset,value);else put(P+offset,value);
  assert.equal(recover(),3);assert.deepEqual(architecture(),before);if(isWide)wide(P+offset,saved);else put(P+offset,saved);
}}
for(const mode of[1,4]){{
  setup('{scalar}',3,14,3);put(H+112,mode);put(H+76,0);put(P+64,0);wide(H+88,0);wide(P+72,0);put(O+24,0);
  if(mode===1)put(H+72,0);
  assert.equal(recover(),1);capture(0,1,0x7fffffc0);assert.equal(recover(),2);assert.equal(recover(),0);
}}
setup('{scalar}',3,14,3);for(const args of[[C,B,0,P],[C,B,-2147483648,P],[C+1,B,1,P],[C,B+1,1,P],[C,B,1,-1]])assert.equal(recover(...args),3);
put(H+4,0);assert.equal(recover(),3);put(H+4,1);wide(H+104,10);assert.equal(recover(),3);
setup('{tagged}',4,23,4);put(P+112,1);put(P+52,0xffffffff);wide(cell(7000),0x7ff8000000000001n);put(cell(7000)+8,-99);put(cell(7000)+12,33);
assert.equal(recover(),1);assert.equal(get(P+52),1);assert.equal(get(cell(7000)+8),0);assert.equal(get(cell(7000)+12),0);
assert.equal(bits(cell(7000)),0x7ff8000000000001n);capture(0,1,0);assert.equal(recover(),2);
assert.equal(get(cell(1)+8),0);capture(2,3,0xc000000000000001n);capture(3,4,0xc000);assert.equal(recover(),0);
assert.equal(bits(P+160+8*35),0xc000000000000001n);assert.equal(bits(P+160+8*43),0xc000n);
assert.equal(bits(P+152)&(1n<<27n),0n);assert.equal(bits(P+160+8*51),0n);
console.log('shared helper authentication, staged scalar, VALID0, full bits and epoch wrap passed');
"#
    );
    let mut child = Command::new("node")
        .args(["--input-type=module"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("node validates authored shared helper");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
