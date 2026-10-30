use super::*;
use crate::memory::{PageRange, Permissions};

#[test]
fn generated_implicit_products_and_division_preserve_exact_results_and_faults() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };
    let scratch = std::env::temp_dir().join(format!(
        "ring3-runtime-multiply-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&scratch).unwrap();
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    for bits in [8, 16, 32] {
        for (name, group) in [("mul", 4), ("imul", 5), ("div", 6), ("idiv", 7)] {
            for source in ["reg", "mem"] {
                let mut code = if bits == 16 { vec![0x66] } else { Vec::new() };
                code.extend([
                    if bits == 8 { 0xf6 } else { 0xf7 },
                    if source == "reg" {
                        0xc1 + group * 8
                    } else {
                        3 + group * 8
                    },
                ]);
                memory.write(GuestAddress(0x1000), &code).unwrap();
                let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
                fs::write(
                    scratch.join(format!("{name}{bits}-{source}.wasm")),
                    &unit.wasm,
                )
                .unwrap();
            }
        }
    }
    for (name, code) in [
        ("mul-ah", &[0xf6, 0xe4][..]),
        ("imul-ah", &[0xf6, 0xec][..]),
        ("div-ah", &[0xf6, 0xf4][..]),
        ("idiv-ah", &[0xf6, 0xfc][..]),
        ("mul-edx", &[0xf7, 0xe2][..]),
        ("div-eax", &[0xf7, 0xf0][..]),
        ("idiv-edx", &[0xf7, 0xfa][..]),
    ] {
        memory.write(GuestAddress(0x1000), code).unwrap();
        let unit = compile(&memory, GuestAddress(0x1000), 1).unwrap();
        fs::write(scratch.join(format!("{name}.wasm")), &unit.wasm).unwrap();
    }
    let result = Command::new("node").arg("--input-type=module").arg("-e").arg(r#"
import assert from 'node:assert/strict';
import fs from 'node:fs';
const root=process.argv[1],memory=new WebAssembly.Memory({initial:2}),data=new DataView(memory.buffer),ptr=0x8000;
const word=(at,value)=>data.setUint32(ptr+at,value>>>0,true),get=at=>data.getUint32(ptr+at,true),registers=()=>Array.from({length:8},(_,i)=>get(8+4*i));
let fault=false;
const imports={env:{memory},cpu:{read(p,address,width){word(52,0);if(fault){word(52,3);word(60,address);return 0;}return width===1?data.getUint8(address):width===2?data.getUint16(address,true):data.getUint32(address,true);},write(){throw new Error('implicit arithmetic wrote guest memory');}}};
const functions=new Map(),load=async name=>{if(!functions.has(name))functions.set(name,(await WebAssembly.instantiate(fs.readFileSync(`${root}/${name}.wasm`),imports)).instance.exports.run);return functions.get(name);};
function init(eax,edx,ecx=1){new Uint8Array(memory.buffer).fill(0);word(0,1);word(4,160);[eax,ecx,edx,0x3000,0x4800,0x12345678,0xaabbccdd,0x8899aabb].forEach((v,i)=>word(8+4*i,v));word(40,0x1000);word(44,0xcd7);word(48,1);word(152,0x1f);fault=false;}
function source(bits,value){if(bits===8)data.setUint8(0x3000,value);else if(bits===16)data.setUint16(0x3000,value,true);else data.setUint32(0x3000,value,true);}
function prefix(bits,value){return bits===8?(0x1234ab00|value)>>>0:bits===16?(0x12340000|value)>>>0:value>>>0;}
for(const bits of[8,16,32]){const modulus=1n<<BigInt(bits),mask=modulus-1n,sign=modulus>>1n,values=[0n,1n,2n,sign-1n,sign,mask];for(const kind of['mul','imul'])for(const form of['reg','mem']){const fn=await load(`${kind}${bits}-${form}`);for(const a of values)for(const b of values){const signed=kind==='imul',product=signed?BigInt.asIntN(bits,a)*BigInt.asIntN(bits,b):a*b,raw=BigInt.asUintN(bits*2,product),overflow=signed?(product< -sign||product>=sign):product>mask,eax=prefix(bits,Number(a)),edx=0x98765432,ecx=bits===8?(0x55667700|Number(b))>>>0:bits===16?(0x55660000|Number(b))>>>0:Number(b);init(eax,edx,ecx);source(bits,Number(b));const before=registers();assert.equal(fn(ptr),0,`${kind}${bits}-${form}`);const after=registers(),expectedEax=bits===8?(0x12340000|Number(raw))>>>0:bits===16?(0x12340000|Number(raw&mask))>>>0:Number(raw&mask),expectedEdx=bits===8?edx:bits===16?(0x98760000|Number((raw>>16n)&mask))>>>0:Number((raw>>32n)&mask);assert.equal(after[0],expectedEax,`${kind}${bits}-${form} ${a} ${b} eax`);assert.equal(after[2],expectedEdx,`${kind}${bits}-${form} ${a} ${b} edx`);assert.deepEqual(after.filter((_,i)=>i!==0&&i!==2),before.filter((_,i)=>i!==0&&i!==2));assert.equal(get(44),0x402|(overflow?0x801:0));assert.equal(get(56),1);assert.equal(get(48),0);}}}
for(const bits of[8,16,32]){const modulus=1n<<BigInt(bits),mask=modulus-1n,sign=modulus>>1n,full=1n<<BigInt(bits*2);for(const kind of['div','idiv'])for(const form of['reg','mem']){const fn=await load(`${kind}${bits}-${form}`),signed=kind==='idiv',cases=signed?[-(full>>1n),-(sign+1n),-sign,-7n,-1n,0n,1n,7n,sign-1n,(full>>1n)-1n].flatMap(n=>[0n,1n,-1n,2n,-2n,-sign,sign-1n].map(d=>[n,d])):[[0n,0n],[0n,1n],[1n,2n],[mask,1n],[mask+1n,1n],[mask*2n+1n,2n],[full-1n,mask],[(mask-1n)*modulus+mask,mask]];for(const[n,denominator]of cases){const raw=BigInt.asUintN(bits*2,n),low=Number(raw&mask),high=Number((raw>>BigInt(bits))&mask),denomRaw=Number(BigInt.asUintN(bits,denominator)),eax=bits===8?(0x12340000|Number(raw))>>>0:bits===16?(0x12340000|low)>>>0:low,edx=bits===8?0x98765432:bits===16?(0x98760000|high)>>>0:high,ecx=bits===8?(0x55667700|denomRaw)>>>0:bits===16?(0x55660000|denomRaw)>>>0:denomRaw;init(eax,edx,ecx);source(bits,denomRaw);const before=registers(),q=denominator===0n?0n:n/denominator,r=denominator===0n?0n:n%denominator,bad=denominator===0n||(signed?q< -sign||q>=sign:q<0n||q>mask),status=fn(ptr);assert.equal(status,bad?9:0,`${kind}${bits}-${form} ${n}/${denominator}`);assert.equal(get(44),0xcd7);if(bad){assert.deepEqual(registers(),before);assert.equal(get(40),0x1000);assert.equal(get(48),1);assert.equal(get(56),0);assert.equal(get(68),0);assert.equal(get(60),0);}else{const qr=Number(BigInt.asUintN(bits,q)),rr=Number(BigInt.asUintN(bits,r)),expectedEax=bits===8?(0x12340000|qr|(rr<<8))>>>0:bits===16?(0x12340000|qr)>>>0:qr,expectedEdx=bits===8?edx:bits===16?(0x98760000|rr)>>>0:rr;assert.equal(get(8),expectedEax,`${kind}${bits}-${form} quotient`);assert.equal(get(16),expectedEdx,`${kind}${bits}-${form} remainder`);assert.deepEqual(registers().filter((_,i)=>i!==0&&i!==2),before.filter((_,i)=>i!==0&&i!==2));assert.equal(get(56),1);assert.equal(get(48),0);}}}}
for(const bits of[8,16,32])for(const kind of['mul','imul','div','idiv']){const fn=await load(`${kind}${bits}-mem`);init(0x12345678,0xabcdef01);source(bits,0);const before=registers();fault=true;assert.equal(fn(ptr),3,`${kind}${bits} source fault precedes arithmetic`);assert.deepEqual(registers(),before);assert.equal(get(44),0xcd7);assert.equal(get(40),0x1000);assert.equal(get(48),1);assert.equal(get(56),0);fault=false;}
for(const[name,eax,edx,status,outEax,outEdx,flags]of[
['mul-ah',0x12347f80,0x98765432,0,0x12343f80,0x98765432,0xc03],
['imul-ah',0x12347f80,0x98765432,0,0x1234c080,0x98765432,0xc03],
['div-ah',0x1234017f,0x98765432,9,0x1234017f,0x98765432,0xcd7],
['div-ah',0x1234007f,0x98765432,9,0x1234007f,0x98765432,0xcd7],
['idiv-ah',0x1234ff81,0x98765432,0,0x1234007f,0x98765432,0xcd7],
['mul-edx',0xfffffffe,2,0,0xfffffffc,1,0xc03],
['div-eax',7,0,0,1,0,0xcd7],
['idiv-edx',0xfffffff9,0xffffffff,0,7,0,0xcd7]]){init(eax,edx);const fn=await load(name);assert.equal(fn(ptr),status,name);assert.deepEqual([get(8),get(16),get(44)],[outEax,outEdx,flags],name);assert.equal(get(56),status===0?1:0);}
console.log('implicit MUL/IMUL and DIV/IDIV8/16/32 BigInt results, paired partial registers, quotient faults, i64MIN/-1 and source aliases passed');
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
