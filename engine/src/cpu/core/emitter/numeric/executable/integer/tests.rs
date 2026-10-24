use super::*;
use std::{fmt::Write, process::Command};
use wasm_encoder::{CodeSection, ExportKind, ExportSection, FunctionSection, Module, TypeSection};

fn module(kernel: Kernel) -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function(kernel.parameters(), kernel.results());
    module.section(&types);
    let mut functions = FunctionSection::new();
    functions.function(0);
    module.section(&functions);
    let mut exports = ExportSection::new();
    exports.export("run", ExportKind::Func, 0);
    module.section(&exports);
    let mut code = CodeSection::new();
    code.function(&emit_kernel(&kernel));
    module.section(&code);
    module.finish()
}

#[test]
fn pure_integer_kernels_preserve_exact_signed_payloads_rounding_and_status() {
    let mut script = String::from("const modules=[");
    for kernel in [
        Kernel::Load64,
        Kernel::Store {
            format: X87Format::Integer16,
            truncate: false,
        },
        Kernel::Store {
            format: X87Format::Integer32,
            truncate: false,
        },
        Kernel::Store {
            format: X87Format::Integer64,
            truncate: false,
        },
        Kernel::Store {
            format: X87Format::Integer16,
            truncate: true,
        },
        Kernel::Store {
            format: X87Format::Integer32,
            truncate: true,
        },
        Kernel::Store {
            format: X87Format::Integer64,
            truncate: true,
        },
    ] {
        let (width, truncate) = match kernel {
            Kernel::Load64 => (0, false),
            Kernel::Store { format, truncate } => (width(format), truncate),
        };
        write!(
            &mut script,
            "{{name:{:?},width:{width},truncate:{truncate},hex:'",
            kernel.name()
        )
        .unwrap();
        for byte in module(kernel) {
            write!(&mut script, "{byte:02x}").unwrap();
        }
        script.push_str("'},");
    }
    script.push_str(
        r#"];
const assert=require('node:assert/strict'),signed=value=>BigInt.asIntN(64,value);
const raw=value=>{const bytes=new ArrayBuffer(8),d=new DataView(bytes);d.setFloat64(0,value,true);return d.getBigUint64(0,true);};
for(const row of modules){
  const module=new WebAssembly.Module(Buffer.from(row.hex,'hex'));
  assert.deepEqual(WebAssembly.Module.imports(module),[],'the primitive cannot observe CPU or memory');
  const run=new WebAssembly.Instance(module).exports.run;
  if(!row.width){
    const cases=[
      [0n,0n,0,0n,1],
      [1n,0x8000000000000000n,0x3fff,0x3ff0000000000000n,1],
      [-1n,0x8000000000000000n,0xbfff,0xbff0000000000000n,1],
      [0x20000000000001n,0x8000000000000400n,0x4034,0x4340000000000000n,0],
      [0x7fffffffffffffffn,0xfffffffffffffffen,0x403d,0x43e0000000000000n,0],
      [-0x8000000000000000n,0x8000000000000000n,0xc03e,0xc3e0000000000000n,1],
    ];
    for(const [input,low,high,native,valid]of cases){
      const actual=run(input);
      assert.deepEqual([actual[0],actual[1],raw(actual[2]),actual[3],actual[4]],
        [signed(low),high,native,valid,valid],row.name+' exact signed input '+input);
    }
    continue;
  }
  const bits=row.width*8,limit=2**(bits-1),indefinite=signed(1n<<BigInt(bits-1));
  for(const pc of [0,0x200,0x300])for(let rc=0;rc<4;rc++){
    const cw=0x7f|pc|(rc<<10),mode=row.truncate?3:rc;
    // expected integers and magnitude increments are independent literal tables.
    const cases=[
      [1.5,[2n,1n,2n,1n],[1,0,1,0]],
      [-1.5,[-2n,-2n,-1n,-1n],[1,1,0,0]],
      [2.5,[2n,2n,3n,2n],[0,0,1,0]],
      [-2.5,[-2n,-3n,-2n,-2n],[0,1,0,0]],
    ];
    for(const [value,integers,increments]of cases)
      assert.deepEqual(run(value,cw,0x204),[integers[mode],0x24|(increments[mode]?0x200:0),1],
        row.name+' RC'+rc+' PC'+pc+' tie '+value);
    for(const zero of [0,-0])assert.deepEqual(run(zero,cw,0x224),[0n,0x24,1],row.name+' signed zero');
    assert.deepEqual(run(-limit,cw,0x224),[signed(BigInt(-limit)),0x24,1],row.name+' inclusive signed minimum');
    for(const oldStatus of [0x204,0x224])
      assert.deepEqual(run(limit,cw,oldStatus),[indefinite,(oldStatus&~0x200)|1,1],
        row.name+' positive boundary invalid preserves only prior PE');
    for(const special of [NaN,Infinity,-Infinity])
      assert.deepEqual(run(special,cw,0x224),[0n,0x224,0],row.name+' special falls back before effects');
  }
  if(row.width===2&&!row.truncate){
    assert.deepEqual(run(32767.5,0x7f,0x204),[32768n,5,1],'invalid nearest result suppresses new PE/C1');
    assert.deepEqual(run(32767.5,0x47f,0x204),[32767n,0x24,1],'downward result remains in range');
    assert.deepEqual(run(-32768.5,0x7f,0x204),[-32768n,0x24,1],'negative even tie remains in range');
    assert.deepEqual(run(-32768.5,0x47f,0x204),[32768n,5,1],'negative downward overflow is indefinite');
  }
  if(row.truncate)for(let rc=0;rc<4;rc++)
    assert.deepEqual(run(-1.75,0x7f|(rc<<10),0x204),[-1n,0x24,1],'FISTTP ignores architectural RC');
}
"#,
    );
    let result = Command::new("node")
        .args(["-e", &script])
        .output()
        .expect("execute authored pure integer conversion kernels");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
