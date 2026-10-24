//! pure arithmetic kernels; status ownership stays with the ordered caller.
use crate::cpu::x86::X87Binary;
use wasm_encoder::{BlockType, Function, InstructionSink, ValType};

pub(in crate::cpu::core::emitter) const RECIPE: i32 = 1 << 16;
const LEFT: u32 = 0;
const RIGHT: u32 = 1;
const MODE: u32 = 2;
const STATUS: u32 = 3;
const OK: u32 = 4;
const EFFECT: u32 = 5;
const EXP: u32 = 6;
const BITS: u32 = 7;
const RESULT: u32 = 8;
const EXACT: u32 = 9;
const ROUNDED: u32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core::emitter) struct Kernel {
    pub kind: X87Binary,
    pub exact_product: bool,
}

// callers admit masked round-to-nearest control. mode0 operands are normal
// binary32 values or signed zero; mode1 operands are exact bounded binary64.
// exact_product requires an unconditional native24 pair in the same cw epoch.

impl Kernel {
    pub fn name(&self) -> &'static str {
        match self.kind {
            X87Binary::Add => "guest.fp.add",
            X87Binary::Subtract => "guest.fp.sub",
            X87Binary::ReverseSubtract => "guest.fp.rsub",
            X87Binary::Multiply if self.exact_product => "guest.fp.mul_exact",
            X87Binary::Multiply => "guest.fp.mul",
            X87Binary::Divide => "guest.fp.div",
            X87Binary::ReverseDivide => "guest.fp.rdiv",
        }
    }
    pub fn recipe(self) -> i32 {
        match self.kind {
            X87Binary::Add | X87Binary::Subtract | X87Binary::ReverseSubtract => 1,
            X87Binary::Multiply => 2,
            X87Binary::Divide | X87Binary::ReverseDivide => 3,
        }
    }
}

fn operands(kind: X87Binary) -> (u32, u32) {
    if matches!(kind, X87Binary::ReverseSubtract | X87Binary::ReverseDivide) {
        (RIGHT, LEFT)
    } else {
        (LEFT, RIGHT)
    }
}

fn operation(c: &mut InstructionSink<'_>, kind: X87Binary) {
    let (left, right) = operands(kind);
    c.local_get(left).local_get(right);
    match kind {
        X87Binary::Add => {
            c.f64_add();
        }
        X87Binary::Subtract | X87Binary::ReverseSubtract => {
            c.f64_sub();
        }
        X87Binary::Multiply => {
            c.f64_mul();
        }
        X87Binary::Divide | X87Binary::ReverseDivide => {
            c.f64_div();
        }
    }
}

fn bound(c: &mut InstructionSink<'_>, value: u32) {
    c.local_get(value)
        .i64_reinterpret_f64()
        .i64_const(52)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(2047)
        .i32_and()
        .i32_const(1023)
        .i32_sub()
        .local_tee(EXP)
        .i32_const(-900)
        .i32_lt_s()
        .local_get(EXP)
        .i32_const(900)
        .i32_gt_s()
        .i32_or()
        .local_get(value)
        .f64_const(0.0.into())
        .f64_ne()
        .i32_and()
        .br_if(1);
}

fn double(c: &mut InstructionSink<'_>, kernel: Kernel) {
    operation(c, kernel.kind);
    c.local_set(RESULT);
    if kernel.kind == X87Binary::Multiply && kernel.exact_product {
        c.i32_const(0).local_set(EFFECT);
        return;
    }
    bound(c, RESULT);
    let (left, right) = operands(kernel.kind);
    match kernel.kind {
        X87Binary::Multiply => {
            c.local_get(RESULT)
                .f64_const(0.0.into())
                .f64_eq()
                .local_get(LEFT)
                .f64_const(0.0.into())
                .f64_ne()
                .i32_and()
                .local_get(RIGHT)
                .f64_const(0.0.into())
                .f64_ne()
                .i32_and()
                .br_if(1);
        }
        X87Binary::Divide | X87Binary::ReverseDivide => {
            c.local_get(RESULT)
                .f64_const(0.0.into())
                .f64_eq()
                .local_get(left)
                .f64_const(0.0.into())
                .f64_ne()
                .i32_and()
                .br_if(1)
                .local_get(RESULT)
                .f64_abs()
                .local_get(right)
                .f64_abs()
                .f64_mul()
                .local_set(EXACT);
            bound(c, EXACT);
        }
        _ => {}
    }
    c.i32_const(RECIPE | kernel.recipe()).local_set(EFFECT);
}

fn single(c: &mut InstructionSink<'_>, kind: X87Binary) {
    if matches!(
        kind,
        X87Binary::Add | X87Binary::Subtract | X87Binary::ReverseSubtract
    ) {
        c.local_get(LEFT)
            .i64_reinterpret_f64()
            .i64_const(52)
            .i64_shr_u()
            .i32_wrap_i64()
            .i32_const(2047)
            .i32_and()
            .local_get(RIGHT)
            .i64_reinterpret_f64()
            .i64_const(52)
            .i64_shr_u()
            .i32_wrap_i64()
            .i32_const(2047)
            .i32_and()
            .i32_sub()
            .local_tee(EXP)
            .i32_const(24)
            .i32_gt_s()
            .local_get(EXP)
            .i32_const(-24)
            .i32_lt_s()
            .i32_or()
            .local_get(LEFT)
            .f64_const(0.0.into())
            .f64_ne()
            .i32_and()
            .local_get(RIGHT)
            .f64_const(0.0.into())
            .f64_ne()
            .i32_and()
            .br_if(1);
    }
    operation(c, kind);
    c.local_set(EXACT)
        .local_get(EXACT)
        .f64_abs()
        .f64_const(2.0f64.powi(-126).into())
        .f64_lt()
        .local_get(EXACT)
        .f64_const(0.0.into())
        .f64_ne()
        .i32_and()
        .br_if(1)
        .local_get(EXACT)
        .f32_demote_f64()
        .i32_reinterpret_f32()
        .local_set(BITS)
        .local_get(BITS)
        .i32_const(0x7fff_ffff)
        .i32_and()
        .i32_eqz()
        .i32_eqz()
        .local_get(BITS)
        .i32_const(23)
        .i32_shr_u()
        .i32_const(255)
        .i32_and()
        .i32_const(1)
        .i32_sub()
        .i32_const(254)
        .i32_ge_u()
        .i32_and()
        .br_if(1)
        .local_get(BITS)
        .f32_reinterpret_i32()
        .f64_promote_f32()
        .local_tee(ROUNDED)
        .local_set(RESULT);
    let division = matches!(kind, X87Binary::Divide | X87Binary::ReverseDivide);
    if division {
        let (left, right) = operands(kind);
        c.local_get(ROUNDED)
            .local_get(right)
            .f64_mul()
            .f64_abs()
            .local_set(EXACT)
            .local_get(left)
            .f64_abs()
            .local_set(ROUNDED);
    } else {
        c.local_get(EXACT)
            .f64_abs()
            .local_set(EXACT)
            .local_get(ROUNDED)
            .f64_abs()
            .local_set(ROUNDED);
    }
    c.local_get(EXACT)
        .local_get(ROUNDED)
        .f64_ne()
        .i32_const(5)
        .i32_shl();
    if division {
        c.local_get(EXACT).local_get(ROUNDED).f64_gt();
    } else {
        c.local_get(ROUNDED).local_get(EXACT).f64_gt();
    }
    c.i32_const(9).i32_shl().i32_or().local_set(EFFECT);
}

pub(in crate::cpu::core::emitter) fn emit_status() -> Function {
    super::super::double::emit_resolver()
}

pub(in crate::cpu::core::emitter) fn emit(kernel: &Kernel, resolver: u32) -> Function {
    let mut function = Function::new([(4, ValType::I32), (3, ValType::F64)]);
    let mut c = function.instructions();
    c.block(BlockType::Empty)
        .local_get(MODE)
        .i32_const(1)
        .i32_gt_u()
        .br_if(0)
        .local_get(MODE)
        .if_(BlockType::Empty);
    double(&mut c, *kernel);
    c.else_();
    single(&mut c, kernel.kind);
    c.end()
        .local_get(EFFECT)
        .i32_const(RECIPE)
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(STATUS)
        .i32_const(0x20)
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(EFFECT)
        .i32_const(0xff)
        .i32_and()
        .local_set(EFFECT)
        .else_()
        .local_get(EFFECT)
        .i32_const(0xff)
        .i32_and()
        .local_get(STATUS);
    let (left, right) = operands(kernel.kind);
    c.local_get(left).local_get(right);
    if matches!(
        kernel.kind,
        X87Binary::Subtract | X87Binary::ReverseSubtract
    ) {
        c.f64_neg();
    }
    c.local_get(RESULT)
        .call(resolver)
        .local_set(STATUS)
        .i32_const(0)
        .local_set(EFFECT)
        .end()
        .else_()
        .local_get(STATUS)
        .i32_const(!0x200)
        .i32_and()
        .local_get(EFFECT)
        .i32_or()
        .local_set(STATUS)
        .i32_const(0)
        .local_set(EFFECT)
        .end()
        .i32_const(1)
        .local_set(OK)
        .end()
        .local_get(OK)
        .i32_eqz()
        .if_(BlockType::Empty)
        .f64_const(0.0.into())
        .local_set(RESULT)
        .i32_const(0)
        .local_set(STATUS)
        .i32_const(0)
        .local_set(EFFECT)
        .end()
        .local_get(RESULT)
        .local_get(STATUS)
        .local_get(EFFECT)
        .local_get(OK)
        .end();
    function
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fmt::Write, process::Command};
    use wasm_encoder::{
        CodeSection, ExportKind, ExportSection, FunctionSection, Module, TypeSection,
    };

    #[test]
    fn finite_kernels_are_valid_narrow_pure_functions() {
        let mut script = String::from("const modules=[");
        for kind in [
            X87Binary::Add,
            X87Binary::Subtract,
            X87Binary::ReverseSubtract,
            X87Binary::Multiply,
            X87Binary::Divide,
            X87Binary::ReverseDivide,
        ] {
            for exact_product in [false, true] {
                let kernel = Kernel {
                    kind,
                    exact_product,
                };
                let mut types = TypeSection::new();
                types.ty().function(
                    [ValType::F64, ValType::F64, ValType::I32, ValType::I32],
                    [ValType::F64, ValType::I32, ValType::I32, ValType::I32],
                );
                types.ty().function(
                    [
                        ValType::I32,
                        ValType::I32,
                        ValType::F64,
                        ValType::F64,
                        ValType::F64,
                    ],
                    [ValType::I32],
                );
                let mut functions = FunctionSection::new();
                functions.function(0).function(1);
                let mut exports = ExportSection::new();
                exports.export("run", ExportKind::Func, 0);
                let mut code = CodeSection::new();
                code.function(&emit(&kernel, 1)).function(&emit_status());
                let mut module = Module::new();
                module
                    .section(&types)
                    .section(&functions)
                    .section(&exports)
                    .section(&code);
                let bytes = module.finish();
                let expected = match kind {
                    X87Binary::Add => 10.0,
                    X87Binary::Subtract => 6.0,
                    X87Binary::ReverseSubtract => -6.0,
                    X87Binary::Multiply => 16.0,
                    X87Binary::Divide => 4.0,
                    X87Binary::ReverseDivide => 0.25,
                };
                write!(
                    &mut script,
                    "{{name:{:?},expected:{expected},recipe:{},exact:{exact_product},hex:\"",
                    kernel.name(),
                    kernel.recipe()
                )
                .unwrap();
                for byte in bytes {
                    write!(&mut script, "{byte:02x}").unwrap();
                }
                script.push_str("\"},");
            }
        }
        script.push_str(r#"];const assert=require('node:assert/strict');for(const item of modules){
          const module=new WebAssembly.Module(Buffer.from(item.hex,'hex'));
          assert.deepEqual(WebAssembly.Module.imports(module),[]);
          const run=new WebAssembly.Instance(module).exports.run;
          assert.deepEqual(run(8,2,0,0),[item.expected,0,0,1],item.name+' PC24');
          assert.deepEqual(run(8,2,1,0),[item.expected,0,0,1],item.name+' PC53 exact operation');
          assert.deepEqual(run(8,2,1,544),[item.expected,item.name==='guest.fp.mul_exact'?32:544,item.name==='guest.fp.mul_exact'?0:item.recipe,1],item.name+' sticky PE and pending C1');
          assert.deepEqual(run(8,2,0,544),[item.expected,32,0,1],item.name+' PC24 clears last C1');
          assert.deepEqual(run(8,2,2,544),[0,0,0,0],item.name+' invalid mode is neutral');
          if(item.name==='guest.fp.add'){
            assert.deepEqual(run(1.5,2**-24,0,0),[1.5,32,0,1]);
            assert.deepEqual(run(1.5,2**-24,1,0),[1.5+2**-24,0,0,1]);
            assert.deepEqual(run(1.5+2**-23,2**-24,0,0),[1.5+2**-22,544,0,1]);
            assert(Object.is(run(-0,-0,0,0)[0],-0));
          }
          if(item.name==='guest.fp.mul'||item.name==='guest.fp.mul_exact'){
            assert.deepEqual(run(3.4028234663852886e38,2,0,0),[0,0,0,0]);
            assert(Object.is(run(-0,3.5,1,0)[0],-0));
          }
          if(item.name==='guest.fp.div')assert.deepEqual(run(8,0,1,544),[0,0,0,0]);
        }"#);
        let result = Command::new("node")
            .args(["-e", &script])
            .output()
            .expect("validate and execute narrow arithmetic kernels");
        assert!(
            result.status.success(),
            "narrow arithmetic kernel validation failed:\n{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
