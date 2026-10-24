//! ordered integer loads and stores consume the same typed fp payloads.
mod kernel;

use super::{BITS, EXP, LEFT, PACKED, POINTER, STATUS, memory, native};
use crate::cpu::core::{
    emitter::{Output, mem},
    region::values::executable::fp::{FpEffect, FpEffectKind, FpPayload, NativeDomain},
};
use crate::cpu::x86::X87Format;
use wasm_encoder::{Function, ValType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core::emitter) enum Kernel {
    Load64,
    Store { format: X87Format, truncate: bool },
}

impl Kernel {
    pub fn parameters(self) -> Vec<ValType> {
        match self {
            Self::Load64 => vec![ValType::I64],
            Self::Store { .. } => vec![ValType::F64, ValType::I32, ValType::I32],
        }
    }

    pub fn results(self) -> Vec<ValType> {
        match self {
            Self::Load64 => vec![
                ValType::I64,
                ValType::I32,
                ValType::F64,
                ValType::I32,
                ValType::I32,
            ],
            Self::Store { .. } => vec![ValType::I64, ValType::I32, ValType::I32],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Load64 => "guest.fp.integer.load64",
            Self::Store {
                format: X87Format::Integer16,
                truncate: false,
            } => "guest.fp.integer.store16",
            Self::Store {
                format: X87Format::Integer32,
                truncate: false,
            } => "guest.fp.integer.store32",
            Self::Store {
                format: X87Format::Integer64,
                truncate: false,
            } => "guest.fp.integer.store64",
            Self::Store {
                format: X87Format::Integer16,
                truncate: true,
            } => "guest.fp.integer.truncate16",
            Self::Store {
                format: X87Format::Integer32,
                truncate: true,
            } => "guest.fp.integer.truncate32",
            Self::Store {
                format: X87Format::Integer64,
                truncate: true,
            } => "guest.fp.integer.truncate64",
            _ => unreachable!("integer kernel format"),
        }
    }
}

pub(in crate::cpu::core::emitter) fn emit_kernel(kernel: &Kernel) -> Function {
    match *kernel {
        Kernel::Load64 => kernel::load64(),
        Kernel::Store { format, truncate } => kernel::store(width(format), truncate),
    }
}

fn width(format: X87Format) -> u32 {
    match format {
        X87Format::Integer16 => 2,
        X87Format::Integer32 => 4,
        X87Format::Integer64 => 8,
        _ => unreachable!("integer effect format"),
    }
}

pub(super) fn emit(out: &mut Output<'_>, effect: &FpEffect) -> bool {
    match &effect.kind {
        FpEffectKind::IntegerLoad {
            address,
            format,
            result,
        } => {
            memory::prepare_value(out, address, width(*format), false, false, POINTER);
            out.c().local_get(POINTER);
            match (*format, *result) {
                (X87Format::Integer16, FpPayload::Native { value, .. }) => {
                    out.c().i32_load16_s(mem(0, 0)).f64_convert_i32_s();
                    out.define(value);
                }
                (X87Format::Integer32, FpPayload::Native { value, .. }) => {
                    out.c().i32_load(mem(0, 0)).f64_convert_i32_s();
                    out.define(value);
                }
                (
                    X87Format::Integer64,
                    FpPayload::Tagged {
                        low,
                        high,
                        native,
                        valid,
                        certificate,
                    },
                ) => {
                    let function = out.integer_kernel(Kernel::Load64);
                    out.c().i64_load(mem(0, 0)).call(function);
                    for output in [certificate, valid, native, high, low] {
                        out.define(output);
                    }
                }
                _ => unreachable!("integer load has its typed result representation"),
            }
        }
        FpEffectKind::IntegerStore {
            address,
            source,
            format,
            truncate,
        } => {
            // raw80 values that are not exactly represented by the native lane
            // stay on the existing before-op exact-suffix path.
            native(out, *source, LEFT, NativeDomain::Exact, effect.control, 0);
            memory::prepare_value(out, address, width(*format), true, false, POINTER);
            let function = out.integer_kernel(Kernel::Store {
                format: *format,
                truncate: *truncate,
            });
            out.c()
                .local_get(LEFT)
                .local_get(87)
                .local_get(STATUS)
                .call(function)
                .local_set(EXP)
                .local_set(BITS)
                .local_set(PACKED)
                .local_get(EXP)
                .i32_eqz()
                .br_if(0);
            // only scratch has changed. the pure callee preserves the prepared
            // destination, slot and transaction version until the actual write.
            out.c().local_get(POINTER).local_get(PACKED);
            match format {
                X87Format::Integer16 => {
                    out.c().i32_wrap_i64().i32_store16(mem(0, 0));
                }
                X87Format::Integer32 => {
                    out.c().i32_wrap_i64().i32_store(mem(0, 0));
                }
                X87Format::Integer64 => {
                    out.c().i64_store(mem(0, 0));
                }
                _ => unreachable!("integer store width"),
            }
            memory::commit(out);
            out.c()
                .local_get(BITS)
                .local_set(STATUS)
                .i32_const(0)
                .local_set(100);
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests;
