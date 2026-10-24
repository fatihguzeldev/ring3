//! Ordered finite comparisons and exact m64 loads retain typed values locally.
use super::*;

fn normal_or_zero(code: &mut wasm_encoder::InstructionSink<'_>, value: u32, depth: u32) {
    code.local_get(value)
        .i64_reinterpret_f64()
        .i64_const(i64::MAX)
        .i64_and()
        .i64_eqz()
        .i32_eqz()
        .local_get(value)
        .i64_reinterpret_f64()
        .i64_const(52)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(2047)
        .i32_and()
        .i32_const(1)
        .i32_sub()
        .i32_const(2046)
        .i32_ge_u()
        .i32_and()
        .br_if(depth);
}

fn relation(
    code: &mut wasm_encoder::InstructionSink<'_>,
    left: u32,
    right: u32,
    status: u32,
    pending: u32,
) {
    // finite comparisons replace C0/C2/C3 and the last C1 writer, while all
    // sticky exceptions, TOP and unrelated status bits remain unchanged.
    code.local_get(status)
        .i32_const(!0x4700)
        .i32_and()
        .local_get(left)
        .local_get(right)
        .f64_lt()
        .i32_const(0x100)
        .i32_mul()
        .i32_or()
        .local_get(left)
        .local_get(right)
        .f64_eq()
        .i32_const(0x4000)
        .i32_mul()
        .i32_or()
        .local_set(status)
        .i32_const(0)
        .local_set(pending);
}

pub(super) fn reads_memory(effect: &FpEffect) -> bool {
    matches!(
        &effect.kind,
        FpEffectKind::Load64 { .. }
            | FpEffectKind::Compare {
                right: FpOperand::Memory32(_) | FpOperand::Memory64(_),
                ..
            }
    )
}

fn load64(
    out: &mut Output<'_>,
    address: &crate::cpu::core::region::values::executable::AddressValue,
) {
    memory::prepare_value(out, address, 8, false, false, POINTER);
    out.c()
        .local_get(POINTER)
        .i64_load(mem(0, 0))
        .f64_reinterpret_i64()
        .local_set(RIGHT);
}

pub(super) fn emit(out: &mut Output<'_>, effect: &FpEffect) {
    match &effect.kind {
        FpEffectKind::Load64 { address, result } => {
            // the real operand read precedes stack-overflow/domain rejection.
            // a rejected input still resumes the exact before-operation state.
            load64(out, address);
            super::guard_stack(out, effect, 0);
            normal_or_zero(out.c().raw(), RIGHT, 0);
            out.c().local_get(RIGHT);
            out.define(*result);
            clear_c1(out);
        }
        FpEffectKind::Compare { left, right, quiet } => {
            match right {
                FpOperand::Memory32(address) => {
                    memory::prepare_value(out, address, 4, false, false, POINTER);
                    out.c()
                        .local_get(POINTER)
                        .i32_load(mem(0, 0))
                        .local_set(BITS);
                    // no memory or stack side effect is committed by admission.
                    super::guard_stack(out, effect, 0);
                    direct::normal_or_zero(out, BITS, 0);
                    out.c()
                        .local_get(BITS)
                        .f32_reinterpret_i32()
                        .f64_promote_f32()
                        .local_set(RIGHT);
                }
                FpOperand::Memory64(address) => {
                    load64(out, address);
                    super::guard_stack(out, effect, 0);
                    normal_or_zero(out.c().raw(), RIGHT, 0);
                }
                FpOperand::Payload(payload) => {
                    native(out, *payload, RIGHT, NativeDomain::Exact, effect.control, 0);
                    normal_or_zero(out.c().raw(), RIGHT, 0);
                }
            }
            // admission of a raw left operand can use the shared native scratch.
            out.c().local_get(RIGHT).local_set(74);
            native(out, *left, LEFT, NativeDomain::Exact, effect.control, 0);
            normal_or_zero(out.c().raw(), LEFT, 0);
            out.c().local_get(74).local_set(RIGHT);
            // ordered and quiet compares agree for the admitted finite domain.
            // unsupported values retain their original quiet/signalling policy
            // through the existing before-operation exact suffix.
            let _ = quiet;
            relation(out.c().raw(), LEFT, RIGHT, STATUS, 100);
            // pop count, tags, TOP and preserved raw ghosts belong exclusively
            // to effect.after; the consumer never rotates payloads a second time.
        }
        _ => unreachable!("comparison consumer has a typed compare or m64 load"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fmt::Write, process::Command};
    use wasm_encoder::{
        CodeSection, ExportKind, ExportSection, Function, FunctionSection, Module, TypeSection,
    };

    #[test]
    fn finite_compare_guard_and_condition_codes_execute_without_rounding() {
        let mut types = TypeSection::new();
        types.ty().function([ValType::F64], [ValType::I32]);
        types.ty().function(
            [ValType::F64, ValType::F64, ValType::I32],
            [ValType::I32, ValType::I32],
        );
        let mut functions = FunctionSection::new();
        functions.function(0).function(1);
        let mut exports = ExportSection::new();
        exports.export("accept", ExportKind::Func, 0);
        exports.export("compare", ExportKind::Func, 1);
        let mut guard = Function::new([]);
        let mut code = guard.instructions();
        code.block(BlockType::Empty);
        normal_or_zero(&mut code, 0, 0);
        code.i32_const(1).return_().end().i32_const(0).end();
        let mut compare = Function::new([(1, ValType::I32)]);
        let mut code = compare.instructions();
        code.i32_const(7).local_set(3);
        relation(&mut code, 0, 1, 2, 3);
        code.local_get(2).local_get(3).end();
        let mut code = CodeSection::new();
        code.function(&guard).function(&compare);
        let mut module = Module::new();
        module
            .section(&types)
            .section(&functions)
            .section(&exports)
            .section(&code);
        let mut script = String::from("const b=Buffer.from('");
        for byte in module.finish() {
            write!(&mut script, "{byte:02x}").unwrap();
        }
        script.push_str(
            r#"','hex'),assert=require('node:assert/strict');
          const {accept,compare}=new WebAssembly.Instance(new WebAssembly.Module(b)).exports;
          for(const n of [0,-0,1,-1,2**-1022,-(2**-1022),Number.MAX_VALUE,-Number.MAX_VALUE])
            assert.equal(accept(n),1,'finite normal or signed zero');
          for(const n of [Number.MIN_VALUE,-Number.MIN_VALUE,Infinity,-Infinity,NaN])
            assert.equal(accept(n),0,'unsupported domain uses exact fallback');
          const old=0x7f3f,base=old&~0x4700;
          assert.deepEqual(compare(1,2,old),[base|0x100,0]);
          assert.deepEqual(compare(2,1,old),[base,0]);
          assert.deepEqual(compare(-0,0,old),[base|0x4000,0]);
          assert.deepEqual(compare(1.5+2**-24,1.5,0x220),[0x20,0]);
          assert.deepEqual(compare(2**-1022,Number.MAX_VALUE,0x20),[0x120,0]);
        "#,
        );
        let result = Command::new("node")
            .args(["-e", &script])
            .output()
            .expect("validate and execute finite comparison fragments");
        assert!(
            result.status.success(),
            "finite compare validation failed:\n{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
