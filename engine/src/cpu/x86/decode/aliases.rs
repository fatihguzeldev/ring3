use iced_x86::Code;

// these amd-defined legacy aliases use existing canonical semantics only after fetch.
pub(super) fn canonical_code(code: Code) -> Option<Code> {
    Some(match code {
        Code::Add_rm8_imm8_82 => Code::Add_rm8_imm8,
        Code::Or_rm8_imm8_82 => Code::Or_rm8_imm8,
        Code::Adc_rm8_imm8_82 => Code::Adc_rm8_imm8,
        Code::Sbb_rm8_imm8_82 => Code::Sbb_rm8_imm8,
        Code::And_rm8_imm8_82 => Code::And_rm8_imm8,
        Code::Sub_rm8_imm8_82 => Code::Sub_rm8_imm8,
        Code::Xor_rm8_imm8_82 => Code::Xor_rm8_imm8,
        Code::Cmp_rm8_imm8_82 => Code::Cmp_rm8_imm8,
        Code::Sal_rm8_1 => Code::Shl_rm8_1,
        Code::Sal_rm8_CL => Code::Shl_rm8_CL,
        Code::Sal_rm8_imm8 => Code::Shl_rm8_imm8,
        Code::Sal_rm16_1 => Code::Shl_rm16_1,
        Code::Sal_rm16_CL => Code::Shl_rm16_CL,
        Code::Sal_rm16_imm8 => Code::Shl_rm16_imm8,
        Code::Sal_rm32_1 => Code::Shl_rm32_1,
        Code::Sal_rm32_CL => Code::Shl_rm32_CL,
        Code::Sal_rm32_imm8 => Code::Shl_rm32_imm8,
        Code::Movzx_r16_rm16 | Code::Movsx_r16_rm16 => Code::Mov_r16_rm16,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "aliases/tests/word.rs"]
mod word;
