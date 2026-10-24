use super::*;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};
use wasm_encoder::{
    CodeSection, CustomSection, EntityType, ExportKind, ExportSection, FunctionSection, GlobalType,
    ImportSection, MemoryType, Module, TypeSection,
};

fn constant(value: i64) -> Function {
    let mut f = Function::new([]);
    f.instructions().i64_const(value).end();
    f
}

fn module(words: &[i64], numeric: bool, kernels: usize, base_global: u32) -> Vec<u8> {
    let index = super::super::super::support::owner_defined(9, numeric) + kernels as u32;
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32], [ValType::I64]);
    let mut imports = ImportSection::new();
    imports.import(
        "ring3",
        "memory",
        EntityType::Memory(MemoryType {
            minimum: 0,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        }),
    );
    if !words.is_empty() {
        for global in 0..base_global {
            imports.import(
                "ring3",
                &format!("padding.{global}"),
                EntityType::Global(GlobalType {
                    val_type: ValType::I32,
                    mutable: false,
                    shared: false,
                }),
            );
        }
        imports.import(
            "ring3",
            "recovery.base",
            EntityType::Global(GlobalType {
                val_type: ValType::I32,
                mutable: false,
                shared: false,
            }),
        );
    }
    let mut declarations = FunctionSection::new();
    for _ in 0..=index {
        declarations.function(0);
    }
    let mut exports = ExportSection::new();
    exports.export("get", ExportKind::Func, index);
    let mut bodies = CodeSection::new();
    for index in 0..index {
        bodies.function(&constant(0x1234_0000 + i64::from(index)));
    }
    bodies.function(&getter(words, base_global));
    let mut m = Module::new();
    m.section(&types)
        .section(&imports)
        .section(&declarations)
        .section(&exports)
        .section(&bodies);
    let blob = encode(words, 0xfedc_ba98_7654_3210);
    if !blob.is_empty() {
        m.section(&CustomSection {
            name: "ring3.recovery".into(),
            data: blob.into(),
        });
    }
    m.finish()
}

fn record(name: &str, words: &[i64], bytes: &[u8], global: u32) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut hex, "{byte:02x}").unwrap();
    }
    let values = words
        .iter()
        .map(|word| format!("\"{word}\""))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{name:\"{name}\",hex:\"{hex}\",values:[{values}],global:{global}}}")
}

#[test]
fn portable_blob_has_exact_owner_header_signed_dictionary_and_zero_padding() {
    assert!(encode(&[], u64::MAX).is_empty());
    let bytes = encode(&[-1, 0, 1, 2, -1], 0x0123_4567_89ab_cdef);
    assert_eq!(&bytes[..4], b"R3RD");
    let fields: Vec<_> = bytes[4..32]
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect();
    assert_eq!(fields, [2, 5, 2, 1, 4, 48, 56]);
    assert_eq!(&bytes[32..40], &0x0123_4567_89ab_cdef_u64.to_le_bytes());
    assert_eq!(&bytes[40..48], &[1, 0, 0, 0, 2, 0, 0, 0]);
    assert_eq!(&bytes[48..56], &0xe4_u64.to_le_bytes());
    assert_eq!(
        &bytes[56..76],
        &[
            255, 255, 255, 255, 1, // signed -1 has high-word index1
            0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 2, 0, 0, 0, 0,
        ]
    );
    assert_eq!(&bytes[76..], &[0, 0, 0, 0, 255, 255, 255, 255]);
    assert_eq!(bytes.len(), 84);
}

fn high_dictionary(highs: u32, lows: u32) -> Vec<i64> {
    (0..highs)
        .flat_map(|high| {
            (0..lows).map(move |low| ((u64::from(high) << 32) | u64::from(low)) as i64)
        })
        .collect()
}

#[test]
fn split_dictionary_width_thresholds_require_strict_size_gain() {
    for (highs, lows, mode) in [(1, 1, 0), (1, 2, 1), (256, 3, 1), (257, 2, 0), (257, 3, 2)] {
        let words = high_dictionary(highs, lows);
        let layout = Layout::new(&words).unwrap();
        assert_eq!(layout.mode, mode, "{highs} high words and {lows} low words");
        let bytes = encode(&words, u64::MAX);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), mode);
        assert_eq!(
            u32::from_le_bytes(bytes[44..48].try_into().unwrap()),
            if mode == 0 { 0 } else { highs }
        );
        let dictionary_bytes = bytes.len() - layout.dictionary_offset as usize;
        if mode == 0 {
            assert_eq!(dictionary_bytes, words.len() * 8);
        } else {
            assert_eq!(
                dictionary_bytes,
                words.len() * (4 + mode as usize) + highs as usize * 4
            );
            assert!(dictionary_bytes < words.len() * 8);
        }
    }
}

#[test]
fn recovery_load_getter_is_bounded_exact_and_read_only_for_both_owner_layouts() {
    fn nonlinear_words(distinct: usize) -> Vec<i64> {
        (0..distinct * 2 + 17)
            .map(|index| {
                let value = ((index * 73 + index / distinct * 19) % distinct) as u64;
                (value.wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(23) ^ 0xd1b5_4a32_d192_ed03)
                    as i64
            })
            .collect()
    }
    let cross_257 = nonlinear_words(257);
    let cross_509 = nonlinear_words(509);
    assert_eq!(cross_257.iter().collect::<BTreeSet<_>>().len(), 257);
    assert_eq!(cross_509.iter().collect::<BTreeSet<_>>().len(), 509);
    let cases = vec![
        ("empty", vec![]),
        ("single-negative", vec![i64::MIN]),
        (
            "exact-literal-bits",
            vec![
                (1i64 << 62) - 1,
                1i64 << 62,
                -(1i64 << 62),
                -(1i64 << 62) - 1,
                0x7ff0_0000_0000_0001,
                0x7ff8_0123_4567_89ab,
                i64::MIN,
                i64::MAX,
            ],
        ),
        ("crossing-257-distinct-nonlinear", cross_257),
        ("crossing-509-distinct-nonlinear", cross_509),
        ("u8-high-index255", high_dictionary(256, 3)),
        ("u16-high-index256", high_dictionary(257, 3)),
        ("equal-size-raw-fallback", high_dictionary(257, 2)),
        ("constant-last-packet", vec![-1; 130]),
        (
            "signed-bits",
            vec![i64::MIN, i64::MAX, -1, 0, 1, i64::MIN + 1, i64::MAX - 1],
        ),
        (
            "high-packed-bits",
            [i64::MIN, -1, 0, i64::MAX]
                .into_iter()
                .cycle()
                .take(131)
                .collect(),
        ),
        (
            "partial-packet",
            (0..529).map(|i| (i % 137) as i64 * 17 - 1000).collect(),
        ),
        (
            "wrapping-bits",
            (0..81)
                .map(|i| (i64::MAX - 7).wrapping_add(3 * i))
                .collect(),
        ),
    ];
    let mut records = Vec::new();
    for numeric in [false, true] {
        for kernels in [0, 3] {
            for global in [0, 7] {
                for (name, words) in &cases {
                    records.push(record(
                        &format!("{name}-numeric{numeric}-kernels{kernels}-global{global}"),
                        words,
                        &module(words, numeric, kernels, global),
                        global,
                    ));
                }
            }
        }
    }
    let script = format!(
        r#"
import assert from 'node:assert/strict';
class Reader {{
  constructor(bytes) {{this.bytes=bytes;this.at=0;}}
  byte() {{assert(this.at<this.bytes.length);return this.bytes[this.at++];}}
  unsigned() {{let value=0,shift=0;for(;;){{const b=this.byte();value+=(b&127)*2**shift;if(!(b&128))return value;shift+=7;assert(shift<35);}}}}
  leb() {{for(let count=0;count<10;count++)if(!(this.byte()&128))return;assert.fail('bounded LEB');}}
  take(n) {{assert(this.at+n<=this.bytes.length);const value=this.bytes.subarray(this.at,this.at+n);this.at+=n;return value;}}
}}
function body(bytes) {{
  const r=new Reader(bytes);r.take(8);let result;
  while(r.at<bytes.length){{const section=r.byte(),s=new Reader(r.take(r.unsigned()));
    if(section===10){{const count=s.unsigned();for(let i=0;i<count;i++)result=s.take(s.unsigned());}}
  }}
  assert(result);return result;
}}
function operators(bytes) {{
  const r=new Reader(bytes),locals=r.unsigned();for(let i=0;i<locals;i++){{r.unsigned();r.byte();}}
  const ops=[];
  while(r.at<bytes.length){{const op=r.byte(),node={{op}};
    if(op===4||op===0x41||op===0x42)r.leb();
    else if(op>=0x20&&op<=0x23)node.index=r.unsigned();
    else if([0x28,0x29,0x2d,0x2f].includes(op)){{node.align=r.unsigned();node.offset=r.unsigned();}}
    else assert([5,11,0x49,0x4b,0x6a,0x6b,0x6c,0x71,0x74,0x76,0x83,0x84,0x86,0x88,0xa7,0xad].includes(op),
      'getter has no loops, calls, stores, mutable globals or other work: '+op.toString(16));
    ops.push(node);
  }}
  return ops;
}}
const shapes=new Map();
for(const item of [{}]) {{
  const bytes=Buffer.from(item.hex,'hex'),module=new WebAssembly.Module(bytes),sections=WebAssembly.Module.customSections(module,'ring3.recovery');
  const imports=WebAssembly.Module.imports(module),expected=[{{module:'ring3',name:'memory',kind:'memory'}}];
  if(item.values.length){{for(let index=0;index<item.global;index++)expected.push({{module:'ring3',name:'padding.'+index,kind:'global'}});
    expected.push({{module:'ring3',name:'recovery.base',kind:'global'}});}}
  assert.deepEqual(imports,expected,item.name+' exact imports');
  const ops=operators(body(bytes)),loads=ops.filter(node=>[0x28,0x29,0x2d,0x2f].includes(node.op));
  const blob=item.values.length?new Uint8Array(sections[0]):new Uint8Array();
  assert.equal(sections.length,item.values.length?1:0,item.name+' exactly one nonempty owner blob');
  if(item.values.length){{
    const h=new DataView(blob.buffer,blob.byteOffset,blob.byteLength);
    assert.equal(Buffer.from(blob.subarray(0,4)).toString(),'R3RD');assert.equal(h.getUint32(4,true),2);
    assert.equal(h.getUint32(8,true),item.values.length);assert.equal(h.getBigUint64(32,true),0xfedcba9876543210n);
    const bits=h.getUint32(12,true),packed=h.getUint32(16,true),dictionary=h.getUint32(20,true);
    assert.equal(bits,Math.max(1,Math.ceil(Math.log2(dictionary))));
    const mode=h.getUint32(40,true),highs=h.getUint32(44,true),dictionaryOffset=48+8*packed;
    assert.equal(packed,Math.ceil(item.values.length*bits/64));assert.equal(h.getUint32(24,true),48);
    assert.equal(h.getUint32(28,true),dictionaryOffset);assert([0,1,2].includes(mode));
    const entryBytes=mode?4+mode:8,highOffset=dictionaryOffset+entryBytes*dictionary;
    assert.equal(blob.length,highOffset+4*highs);
    assert.equal(highs,mode?new Set(item.values.map(value=>Number(BigInt.asUintN(64,BigInt(value))>>32n))).size:0);
    if(mode){{assert(highs<=(mode===1?256:65536));assert(entryBytes*dictionary+4*highs<8*dictionary);}}
    const wanted=mode?[[0x29,0,48],[0x29,0,48],[0x28,0,dictionaryOffset],
      [mode===1?0x2d:0x2f,0,dictionaryOffset+4],[0x28,0,highOffset]]:
      [[0x29,0,48],[0x29,0,48],[0x29,0,dictionaryOffset]];
    assert.deepEqual(loads.map(node=>[node.op,node.align,node.offset]),wanted);
    assert(ops.filter(node=>node.op===0x23).every(node=>node.index===item.global));
    assert.equal(ops.filter(node=>node.op===0x23).length,mode?5:3);
    const current=ops.map(node=>node.op),shape=shapes.get(mode);
    if(shape)assert.deepEqual(current,shape,'getter operation count is independent of metadata size');else shapes.set(mode,current);
  }} else {{assert.equal(loads.length,0);assert.deepEqual(ops.map(node=>node.op),[0x42,11]);}}
  const bindings=(memory,base)=>{{const supplied={{memory}};
    if(item.values.length){{for(let index=0;index<item.global;index++)supplied['padding.'+index]=new WebAssembly.Global({{value:'i32',mutable:false}},0);
      supplied['recovery.base']=new WebAssembly.Global({{value:'i32',mutable:false}},base);}}
    return{{ring3:supplied}};
  }};
  for(const base of [0,127,1025]){{
    const memory=new WebAssembly.Memory({{initial:1}});new Uint8Array(memory.buffer).set(blob,base);
    const before=new Uint8Array(memory.buffer).slice(),get=new WebAssembly.Instance(module,bindings(memory,base)).exports.get;
    for(let index=0;index<item.values.length;index++)assert.equal(get(index),BigInt(item.values[index]),item.name+' word '+index);
    for(let index=item.values.length;index<item.values.length+70;index++)assert.equal(get(index),0n,item.name+' tail '+index);
    for(const index of [-1,0x7fffffff,0x80000000,0xffffffff])assert.equal(get(index),0n,item.name+' unsigned bound '+index);
    assert.deepEqual(new Uint8Array(memory.buffer),before,item.name+' metadata and surrounding memory are read-only');
  }}
  // No memory exists at all, and the base is intentionally invalid. An OOB
  // index must return zero without touching either packed or dictionary data.
  const absent=new WebAssembly.Instance(module,bindings(new WebAssembly.Memory({{initial:0}}),-1)).exports.get;
  for(const index of [item.values.length,-1,0x7fffffff,0x80000000,0xffffffff])assert.equal(absent(index),0n,item.name+' OOB before any load');
}}
"#,
        records.join(",")
    );
    let mut child = Command::new("node")
        .arg("--input-type=module")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("node validates and executes actual immutable recovery loads");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
