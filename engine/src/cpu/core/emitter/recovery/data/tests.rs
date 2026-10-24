use super::*;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};
use wasm_encoder::{CodeSection, ExportKind, ExportSection, FunctionSection, Module, TypeSection};

fn constant(value: i64) -> Function {
    let mut f = Function::new([]);
    f.instructions().i64_const(value).end();
    f
}
fn module(words: &[i64], numeric: bool, kernels: usize) -> Vec<u8> {
    let getter = super::super::super::support::owner_defined(9, numeric);
    let mut generated = functions(words, numeric, kernels);
    let root = generated.remove(0);
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32], [ValType::I64]);
    let mut declarations = FunctionSection::new();
    for _ in 0..getter as usize + 1 + kernels + generated.len() {
        declarations.function(0);
    }
    let mut exports = ExportSection::new();
    exports.export("get", ExportKind::Func, getter);
    let mut bodies = CodeSection::new();
    for index in 0..getter {
        bodies.function(&constant(0x1234_0000 + i64::from(index)));
    }
    bodies.function(&root);
    for index in 0..kernels {
        bodies.function(&constant(-0x1234_0000 - index as i64));
    }
    for f in generated {
        bodies.function(&f);
    }
    let mut m = Module::new();
    m.section(&types)
        .section(&declarations)
        .section(&exports)
        .section(&bodies);
    m.finish()
}
fn direct_chunk(words: &[i64]) -> Vec<u8> {
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32], [ValType::I64]);
    let mut declarations = FunctionSection::new();
    declarations.function(0);
    let mut exports = ExportSection::new();
    exports.export("get", ExportKind::Func, 0);
    let mut bodies = CodeSection::new();
    bodies.function(&chunk(words));
    let mut m = Module::new();
    m.section(&types)
        .section(&declarations)
        .section(&exports)
        .section(&bodies);
    m.finish()
}
fn record(name: &str, words: &[i64], bytes: &[u8], chunk_only: bool) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut hex, "{byte:02x}").unwrap();
    }
    let values = words
        .iter()
        .map(|word| format!("\"{word}\""))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{name:\"{name}\",hex:\"{hex}\",values:[{values}],chunkOnly:{chunk_only}}}")
}

#[test]
fn recovery_data_roundtrips_exact_words_and_rejects_oob_for_both_owner_layouts() {
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
    assert_eq!(
        cross_257
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        257
    );
    assert_eq!(
        cross_509
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        509
    );
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
        ("contiguous-257-distinct-nonlinear", cross_257),
        ("contiguous-509-distinct-nonlinear", cross_509),
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
            "partial-multiple-chunks",
            (0..529).map(|i| (i % 137) as i64 * 17 - 1000).collect(),
        ),
        (
            "wrapping-source-run",
            (0..81)
                .map(|i| (i64::MAX - 7).wrapping_add(3 * i))
                .collect(),
        ),
    ];
    let mut records = Vec::new();
    for numeric in [false, true] {
        for kernels in [0, 3] {
            for (name, words) in &cases {
                records.push(record(
                    &format!("{name}-numeric{numeric}-kernels{kernels}"),
                    words,
                    &module(words, numeric, kernels),
                    false,
                ));
            }
        }
    }
    let wrapping: Vec<_> = (0..64)
        .map(|i| (i64::MAX - 5).wrapping_add(3 * i))
        .collect();
    records.push(record(
        "affine-wraps-signed-max",
        &wrapping,
        &direct_chunk(&wrapping),
        true,
    ));
    let shifted: Vec<_> = [77, -5]
        .into_iter()
        .chain((0..61).map(|i| (i64::MIN + 7).wrapping_sub(11 * i)))
        .collect();
    records.push(record(
        "affine-nonzero-start-and-last-default",
        &shifted,
        &direct_chunk(&shifted),
        true,
    ));
    let script = format!(
        r#"
import assert from 'node:assert/strict';
for (const item of [{}]) {{
  const module=new WebAssembly.Module(Buffer.from(item.hex,'hex'));
  assert.deepEqual(WebAssembly.Module.imports(module),[],item.name);
  const get=new WebAssembly.Instance(module).exports.get;
  for(let index=0;index<item.values.length;index++) {{
    assert.equal(get(index),BigInt(item.values[index]),item.name+' word '+index);
    if(item.chunkOnly)assert.equal(get(index+64),BigInt(item.values[index]),item.name+' chunk-local '+index);
  }}
  if(item.chunkOnly) {{
    for(let index=item.values.length;index<64;index++)assert.equal(get(index),0n,item.name+' last chunk default');
  }} else {{
    for(let index=item.values.length;index<item.values.length+70;index++)assert.equal(get(index),0n,item.name+' tail '+index);
    for(const index of [-1,0x7fffffff,0x80000000,0xffffffff])assert.equal(get(index),0n,item.name+' unsigned bound '+index);
  }}
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
        .expect("node validates and executes emitted recovery helpers");
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
