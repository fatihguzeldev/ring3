use super::*;

pub(super) fn supports(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::FlagsToAh | Operation::AhToFlags | Operation::SetAlFromCarry
    )
}

pub(super) fn emit(out: &mut Output, operation: Operation) -> bool {
    match operation {
        Operation::FlagsToAh => {
            out.c()
                .local_get(FLAGS)
                .i32_const(0xd5)
                .i32_and()
                .i32_const(2)
                .i32_or()
                .local_set(R);
            out.store_byte(ByteRegister::Ah, R);
        }
        Operation::AhToFlags => {
            out.c()
                .local_get(FLAGS)
                .i32_const(!0xd5)
                .i32_and()
                .local_get(reg(Register32::Eax))
                .i32_const(8)
                .i32_shr_u()
                .i32_const(0xd5)
                .i32_and()
                .i32_or()
                .i32_const(2)
                .i32_or()
                .local_set(FLAGS);
        }
        Operation::SetAlFromCarry => {
            out.c()
                .i32_const(0)
                .local_get(FLAGS)
                .i32_const(1)
                .i32_and()
                .i32_sub()
                .local_set(R);
            out.store_byte(ByteRegister::Al, R);
        }
        _ => return false,
    }
    true
}
