use iced_x86::{Code, Instruction, OpKind, Register};

use crate::cpu::x86::{X87Binary as B, X87Condition as C, X87Format as F, X87Operation as O};

#[derive(Clone, Copy, Debug)]
pub(super) struct DecodedX87 {
    pub operation: O,
    pub waiting: bool,
    pub memory: bool,
}

pub(super) fn decode(instruction: &Instruction) -> Option<DecodedX87> {
    let st =
        |register: Register| -> u8 { (register as u32).wrapping_sub(Register::ST0 as u32) as u8 };
    let index = || {
        if instruction.op_count() > 1 {
            st(instruction.op1_register())
        } else {
            st(instruction.op0_register())
        }
    };
    let register_binary = |kind, pop, destination_first| O::Binary {
        kind,
        destination: if destination_first {
            st(instruction.op0_register())
        } else {
            0
        },
        source: if destination_first { 0 } else { index() },
        pop,
    };
    let memory_binary = |kind, format| O::BinaryMemory { kind, format };
    let store = |format, pop, truncate| O::Store {
        format,
        pop,
        truncate,
    };
    let compare = |quiet, pop, integer_flags| O::Compare {
        index: index(),
        quiet,
        pop,
        integer_flags,
    };
    let operation = match instruction.code() {
        Code::Fninit | Code::Finit => O::Initialize,
        Code::Fnclex | Code::Fclex => O::ClearExceptions,
        Code::Wait => O::Wait,
        Code::Fnop => O::NoOperation,
        Code::Fld_m32fp => O::Load(F::Float32),
        Code::Fld_m64fp => O::Load(F::Float64),
        Code::Fld_m80fp => O::Load(F::Extended80),
        Code::Fild_m16int => O::Load(F::Integer16),
        Code::Fild_m32int => O::Load(F::Integer32),
        Code::Fild_m64int => O::Load(F::Integer64),
        Code::Fbld_m80bcd => O::Load(F::PackedBcd),
        Code::Fst_m32fp => store(F::Float32, false, false),
        Code::Fstp_m32fp => store(F::Float32, true, false),
        Code::Fst_m64fp => store(F::Float64, false, false),
        Code::Fstp_m64fp => store(F::Float64, true, false),
        Code::Fstp_m80fp => store(F::Extended80, true, false),
        Code::Fist_m16int => store(F::Integer16, false, false),
        Code::Fistp_m16int => store(F::Integer16, true, false),
        Code::Fist_m32int => store(F::Integer32, false, false),
        Code::Fistp_m32int => store(F::Integer32, true, false),
        Code::Fistp_m64int => store(F::Integer64, true, false),
        Code::Fisttp_m16int => store(F::Integer16, true, true),
        Code::Fisttp_m32int => store(F::Integer32, true, true),
        Code::Fisttp_m64int => store(F::Integer64, true, true),
        Code::Fbstp_m80bcd => store(F::PackedBcd, true, false),
        Code::Fld_sti => O::LoadRegister { index: index() },
        Code::Fxch_st0_sti | Code::Fxch_st0_sti_DDC8 | Code::Fxch_st0_sti_DFC8 => {
            O::Exchange { index: index() }
        }
        Code::Fst_sti => O::StoreRegister {
            index: index(),
            pop: false,
        },
        Code::Fstp_sti | Code::Fstp_sti_DFD0 | Code::Fstp_sti_DFD8 => O::StoreRegister {
            index: index(),
            pop: true,
        },
        Code::Fstpnce_sti => O::StoreRegisterNoUnderflow { index: index() },
        Code::Fadd_st0_sti => register_binary(B::Add, false, false),
        Code::Fmul_st0_sti => register_binary(B::Multiply, false, false),
        Code::Fsub_st0_sti => register_binary(B::Subtract, false, false),
        Code::Fsubr_st0_sti => register_binary(B::ReverseSubtract, false, false),
        Code::Fdiv_st0_sti => register_binary(B::Divide, false, false),
        Code::Fdivr_st0_sti => register_binary(B::ReverseDivide, false, false),
        Code::Fadd_sti_st0 => register_binary(B::Add, false, true),
        Code::Fmul_sti_st0 => register_binary(B::Multiply, false, true),
        Code::Fsub_sti_st0 => register_binary(B::Subtract, false, true),
        Code::Fsubr_sti_st0 => register_binary(B::ReverseSubtract, false, true),
        Code::Fdiv_sti_st0 => register_binary(B::Divide, false, true),
        Code::Fdivr_sti_st0 => register_binary(B::ReverseDivide, false, true),
        Code::Faddp_sti_st0 => register_binary(B::Add, true, true),
        Code::Fmulp_sti_st0 => register_binary(B::Multiply, true, true),
        Code::Fsubp_sti_st0 => register_binary(B::Subtract, true, true),
        Code::Fsubrp_sti_st0 => register_binary(B::ReverseSubtract, true, true),
        Code::Fdivp_sti_st0 => register_binary(B::Divide, true, true),
        Code::Fdivrp_sti_st0 => register_binary(B::ReverseDivide, true, true),
        Code::Fadd_m32fp => memory_binary(B::Add, F::Float32),
        Code::Fmul_m32fp => memory_binary(B::Multiply, F::Float32),
        Code::Fsub_m32fp => memory_binary(B::Subtract, F::Float32),
        Code::Fsubr_m32fp => memory_binary(B::ReverseSubtract, F::Float32),
        Code::Fdiv_m32fp => memory_binary(B::Divide, F::Float32),
        Code::Fdivr_m32fp => memory_binary(B::ReverseDivide, F::Float32),
        Code::Fadd_m64fp => memory_binary(B::Add, F::Float64),
        Code::Fmul_m64fp => memory_binary(B::Multiply, F::Float64),
        Code::Fsub_m64fp => memory_binary(B::Subtract, F::Float64),
        Code::Fsubr_m64fp => memory_binary(B::ReverseSubtract, F::Float64),
        Code::Fdiv_m64fp => memory_binary(B::Divide, F::Float64),
        Code::Fdivr_m64fp => memory_binary(B::ReverseDivide, F::Float64),
        Code::Fiadd_m16int => memory_binary(B::Add, F::Integer16),
        Code::Fimul_m16int => memory_binary(B::Multiply, F::Integer16),
        Code::Fisub_m16int => memory_binary(B::Subtract, F::Integer16),
        Code::Fisubr_m16int => memory_binary(B::ReverseSubtract, F::Integer16),
        Code::Fidiv_m16int => memory_binary(B::Divide, F::Integer16),
        Code::Fidivr_m16int => memory_binary(B::ReverseDivide, F::Integer16),
        Code::Fiadd_m32int => memory_binary(B::Add, F::Integer32),
        Code::Fimul_m32int => memory_binary(B::Multiply, F::Integer32),
        Code::Fisub_m32int => memory_binary(B::Subtract, F::Integer32),
        Code::Fisubr_m32int => memory_binary(B::ReverseSubtract, F::Integer32),
        Code::Fidiv_m32int => memory_binary(B::Divide, F::Integer32),
        Code::Fidivr_m32int => memory_binary(B::ReverseDivide, F::Integer32),
        Code::Fcom_st0_sti | Code::Fcom_st0_sti_DCD0 => compare(false, 0, false),
        Code::Fcomp_st0_sti | Code::Fcomp_st0_sti_DCD8 | Code::Fcomp_st0_sti_DED0 => {
            compare(false, 1, false)
        }
        Code::Fucom_st0_sti => compare(true, 0, false),
        Code::Fucomp_st0_sti => compare(true, 1, false),
        Code::Fcompp => O::Compare {
            index: 1,
            quiet: false,
            pop: 2,
            integer_flags: false,
        },
        Code::Fucompp => O::Compare {
            index: 1,
            quiet: true,
            pop: 2,
            integer_flags: false,
        },
        Code::Fcomi_st0_sti => compare(false, 0, true),
        Code::Fucomi_st0_sti => compare(true, 0, true),
        Code::Fcomip_st0_sti => compare(false, 1, true),
        Code::Fucomip_st0_sti => compare(true, 1, true),
        Code::Fcom_m32fp => O::CompareMemory {
            format: F::Float32,
            pop: false,
        },
        Code::Fcomp_m32fp => O::CompareMemory {
            format: F::Float32,
            pop: true,
        },
        Code::Fcom_m64fp => O::CompareMemory {
            format: F::Float64,
            pop: false,
        },
        Code::Fcomp_m64fp => O::CompareMemory {
            format: F::Float64,
            pop: true,
        },
        Code::Ficom_m16int => O::CompareMemory {
            format: F::Integer16,
            pop: false,
        },
        Code::Ficomp_m16int => O::CompareMemory {
            format: F::Integer16,
            pop: true,
        },
        Code::Ficom_m32int => O::CompareMemory {
            format: F::Integer32,
            pop: false,
        },
        Code::Ficomp_m32int => O::CompareMemory {
            format: F::Integer32,
            pop: true,
        },
        Code::Fcmovb_st0_sti => O::ConditionalMove {
            index: index(),
            condition: C::Below,
        },
        Code::Fcmove_st0_sti => O::ConditionalMove {
            index: index(),
            condition: C::Equal,
        },
        Code::Fcmovbe_st0_sti => O::ConditionalMove {
            index: index(),
            condition: C::BelowOrEqual,
        },
        Code::Fcmovu_st0_sti => O::ConditionalMove {
            index: index(),
            condition: C::Unordered,
        },
        Code::Fcmovnb_st0_sti => O::ConditionalMove {
            index: index(),
            condition: C::NotBelow,
        },
        Code::Fcmovne_st0_sti => O::ConditionalMove {
            index: index(),
            condition: C::NotEqual,
        },
        Code::Fcmovnbe_st0_sti => O::ConditionalMove {
            index: index(),
            condition: C::Above,
        },
        Code::Fcmovnu_st0_sti => O::ConditionalMove {
            index: index(),
            condition: C::Ordered,
        },
        Code::Fchs => O::ChangeSign,
        Code::Fabs => O::Absolute,
        Code::Ftst => O::Test,
        Code::Fxam => O::Examine,
        Code::Fsqrt => O::SquareRoot,
        Code::Frndint => O::RoundToInteger,
        Code::Fscale => O::Scale,
        Code::Fxtract => O::Extract,
        Code::Fprem => O::Remainder { nearest: false },
        Code::Fprem1 => O::Remainder { nearest: true },
        Code::F2xm1 => O::Exp2MinusOne,
        Code::Fyl2x => O::Log2Multiply { plus_one: false },
        Code::Fyl2xp1 => O::Log2Multiply { plus_one: true },
        Code::Fpatan => O::PartialAtan,
        Code::Fsin => O::Trigonometric { function: 0 },
        Code::Fcos => O::Trigonometric { function: 1 },
        Code::Fptan => O::Trigonometric { function: 2 },
        Code::Fsincos => O::Trigonometric { function: 3 },
        Code::Fld1 => O::LoadConstant { index: 0 },
        Code::Fldl2t => O::LoadConstant { index: 1 },
        Code::Fldl2e => O::LoadConstant { index: 2 },
        Code::Fldpi => O::LoadConstant { index: 3 },
        Code::Fldlg2 => O::LoadConstant { index: 4 },
        Code::Fldln2 => O::LoadConstant { index: 5 },
        Code::Fldz => O::LoadConstant { index: 6 },
        Code::Ffree_sti => O::Free {
            index: index(),
            pop: false,
        },
        Code::Ffreep_sti => O::Free {
            index: index(),
            pop: true,
        },
        Code::Fdecstp => O::AdjustTop { increment: false },
        Code::Fincstp => O::AdjustTop { increment: true },
        Code::Fnstsw_AX | Code::Fstsw_AX => O::StatusToAx,
        Code::Fldcw_m2byte => O::LoadControl,
        Code::Fnstcw_m2byte | Code::Fstcw_m2byte => O::StoreControl,
        Code::Fnstsw_m2byte | Code::Fstsw_m2byte => O::StoreStatus,
        Code::Fldenv_m14byte => O::LoadEnvironment { operand16: true },
        Code::Fldenv_m28byte => O::LoadEnvironment { operand16: false },
        Code::Fnstenv_m14byte | Code::Fstenv_m14byte => O::StoreEnvironment { operand16: true },
        Code::Fnstenv_m28byte | Code::Fstenv_m28byte => O::StoreEnvironment { operand16: false },
        Code::Frstor_m94byte => O::Restore { operand16: true },
        Code::Frstor_m108byte => O::Restore { operand16: false },
        Code::Fnsave_m94byte | Code::Fsave_m94byte => O::Save { operand16: true },
        Code::Fnsave_m108byte | Code::Fsave_m108byte => O::Save { operand16: false },
        _ => return None,
    };
    let waiting = !matches!(
        instruction.code(),
        Code::Fninit
            | Code::Fnclex
            | Code::Fnstcw_m2byte
            | Code::Fnstsw_m2byte
            | Code::Fnstsw_AX
            | Code::Fnstenv_m14byte
            | Code::Fnstenv_m28byte
            | Code::Fnsave_m94byte
            | Code::Fnsave_m108byte
    );
    Some(DecodedX87 {
        operation,
        waiting,
        memory: (0..instruction.op_count())
            .any(|index| instruction.op_kind(index) == OpKind::Memory),
    })
}
