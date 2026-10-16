use super::*;

pub(super) fn emit(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedOperation,
    imports: Imports,
    exit_depth: u32,
) -> bool {
    let mut store = false;
    match *instruction.operation() {
        Operation::FarCall { width, target } => {
            crate::cpu::dbt::wasm::instructions::far::call(
                code,
                width,
                target,
                instruction.next_pc().0,
                imports,
                exit_depth,
            );
            store = true;
        }
        Operation::FarReturn {
            width,
            stack_adjust,
        } => {
            crate::cpu::dbt::wasm::instructions::far::return_(
                code,
                width,
                stack_adjust,
                imports,
                exit_depth,
            );
        }
        Operation::InterruptReturn { width } => {
            crate::cpu::dbt::wasm::instructions::far::interrupt_return(
                code, width, imports, exit_depth,
            );
        }
        Operation::FarJump { width, target } => {
            crate::cpu::dbt::wasm::instructions::far::jump(code, width, target, imports, exit_depth)
        }
        Operation::Jump {
            target: BranchTarget::Direct(target),
        } => {
            check_near_target(code, target.0, imports, exit_depth);
            code.i32_const(target.0 as i32).local_set(EIP);
        }
        Operation::Jump {
            target: BranchTarget::Indirect(target),
        } => {
            indirect_target(code, target, imports, exit_depth);
            check_near_value(code, RESULT, imports, exit_depth);
            code.local_get(RESULT).local_set(EIP);
        }
        Operation::JumpWord { source } => {
            code.local_get(register(source))
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT);
            check_near_value(code, RESULT, imports, exit_depth);
            code.local_get(RESULT).local_set(EIP);
        }
        Operation::ReadJumpWord { address } => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            code.local_set(RESULT);
            check_near_value(code, RESULT, imports, exit_depth);
            code.local_get(RESULT).local_set(EIP);
        }
        Operation::ConditionalJump { condition, target } => {
            conditions::condition(code, condition);
            code.if_(BlockType::Result(ValType::I32));
            check_near_target(code, target.0, imports, exit_depth + 1);
            code.i32_const(target.0 as i32)
                .else_()
                .i32_const(instruction.next_pc().0 as i32)
                .end()
                .local_set(EIP);
        }
        Operation::CountBranchWord { kind, target } => {
            let ecx = register(Register32::Ecx);
            if kind == CountBranchKind::EcxZero {
                code.local_get(ecx).i32_const(65535).i32_and().i32_eqz();
            } else {
                code.local_get(ecx)
                    .i32_const(-65536)
                    .i32_and()
                    .local_get(ecx)
                    .i32_const(1)
                    .i32_sub()
                    .i32_const(65535)
                    .i32_and()
                    .i32_or()
                    .local_tee(LHS)
                    .i32_const(65535)
                    .i32_and()
                    .i32_eqz()
                    .i32_eqz();
                if matches!(
                    kind,
                    CountBranchKind::LoopEqual | CountBranchKind::LoopNotEqual
                ) {
                    conditions::condition(
                        code,
                        if kind == CountBranchKind::LoopEqual {
                            Condition::Equal
                        } else {
                            Condition::NotEqual
                        },
                    );
                    code.i32_and();
                }
            }
            code.if_(BlockType::Result(ValType::I32));
            check_near_target(code, target.0, imports, exit_depth + 1);
            code.i32_const(target.0 as i32)
                .else_()
                .i32_const(instruction.next_pc().0 as i32)
                .end()
                .local_set(EIP);
            if kind != CountBranchKind::EcxZero {
                code.local_get(LHS).local_set(ecx);
            }
        }
        Operation::CountBranch { kind, target } => {
            let ecx = register(Register32::Ecx);
            if kind == CountBranchKind::EcxZero {
                code.local_get(ecx).i32_eqz();
            } else {
                code.local_get(ecx)
                    .i32_const(1)
                    .i32_sub()
                    .local_tee(LHS)
                    .i32_const(0)
                    .i32_ne();
                match kind {
                    CountBranchKind::LoopEqual | CountBranchKind::LoopNotEqual => {
                        conditions::condition(
                            code,
                            if kind == CountBranchKind::LoopEqual {
                                Condition::Equal
                            } else {
                                Condition::NotEqual
                            },
                        );
                        code.i32_and();
                    }
                    _ => {}
                }
            }
            code.if_(BlockType::Result(ValType::I32));
            check_near_target(code, target.0, imports, exit_depth + 1);
            code.i32_const(target.0 as i32)
                .else_()
                .i32_const(instruction.next_pc().0 as i32)
                .end()
                .local_set(EIP);
            if kind != CountBranchKind::EcxZero {
                code.local_get(LHS).local_set(ecx);
            }
        }
        Operation::CallWord { target } => {
            check_near_target(code, target.0, imports, exit_depth);
            code.i32_const((instruction.next_pc().0 & 0xffff) as i32)
                .local_set(RESULT);
            memory::push_word_result(code, imports, exit_depth);
            code.i32_const(target.0 as i32).local_set(EIP);
            store = true;
        }
        Operation::CallRegisterWord { source } => {
            word_value(code, WordValue::Register(source));
            code.local_tee(RESULT);
            check_near_value(code, RESULT, imports, exit_depth);
            code.i32_const((instruction.next_pc().0 & 0xffff) as i32)
                .local_set(RESULT);
            memory::push_word_result(code, imports, exit_depth);
            code.local_set(EIP);
            store = true;
        }
        Operation::ReadCallWord { address } => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            code.local_tee(RESULT);
            check_near_value(code, RESULT, imports, exit_depth);
            code.i32_const((instruction.next_pc().0 & 0xffff) as i32)
                .local_set(RESULT);
            memory::push_word_result(code, imports, exit_depth);
            code.local_set(EIP);
            store = true;
        }
        Operation::Call {
            target: BranchTarget::Direct(target),
        } => {
            check_near_target(code, target.0, imports, exit_depth);
            code.i32_const(instruction.next_pc().0 as i32)
                .local_set(RESULT);
            stack::pointer::push_result(code, 4, imports, exit_depth);
            code.i32_const(target.0 as i32).local_set(EIP);
            store = true;
        }
        Operation::Call {
            target: BranchTarget::Indirect(target),
        } => {
            indirect_target(code, target, imports, exit_depth);
            check_near_value(code, RESULT, imports, exit_depth);
            code.local_get(RESULT)
                .i32_const(instruction.next_pc().0 as i32)
                .local_set(RESULT);
            stack::pointer::push_result(code, 4, imports, exit_depth);
            code.local_set(EIP);
            store = true;
        }
        Operation::Push {
            source: Value32::Memory(source),
        } => {
            memory::push_memory(code, source, imports, exit_depth);
            store = true;
        }
        Operation::Push { source } => {
            match source {
                Value32::Register(source) => {
                    code.local_get(register(source));
                }
                Value32::Immediate(value) => {
                    code.i32_const(value as i32);
                }
                Value32::Memory(_) => unreachable!("memory push handled above"),
            }
            code.local_set(RESULT);
            stack::pointer::push_result(code, 4, imports, exit_depth);
            store = true;
        }
        Operation::PushWord { source } => {
            word_value(code, source);
            code.local_set(RESULT);
            memory::push_word_result(code, imports, exit_depth);
            store = true;
        }
        Operation::PushMemoryWord { source } => {
            memory::checked_address(
                code,
                source,
                2,
                crate::cpu::x86::SegmentAccess::Read,
                imports,
                exit_depth,
            );
            stack::pointer::check(
                code,
                Register32::Esp,
                -2,
                2,
                crate::cpu::x86::SegmentAccess::Write,
                imports,
                exit_depth,
            );
            memory::load_narrow_value(code, source, SmallWidth::Word, imports, exit_depth);
            code.local_set(RESULT);
            memory::push_word_result(code, imports, exit_depth);
            store = true;
        }
        Operation::PushFlags => {
            code.local_get(FLAGS)
                .i32_const(0x00fc_ffff)
                .i32_and()
                .local_set(RESULT);
            stack::pointer::push_result(code, 4, imports, exit_depth);
            store = true;
        }
        Operation::PushFlagsWord => {
            code.local_get(FLAGS)
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT);
            memory::push_word_result(code, imports, exit_depth);
            store = true;
        }
        Operation::Pop {
            destination: Location32::Register(destination),
        } => {
            memory::pop_register(code, destination, imports, exit_depth);
        }
        Operation::Pop {
            destination: Location32::Memory(destination),
        } => {
            memory::pop_memory(code, destination, imports, exit_depth);
            store = true;
        }
        Operation::PopWord { destination } => {
            memory::pop_word_register(code, destination, imports, exit_depth);
        }
        Operation::PopMemoryWord { destination } => {
            memory::pop_word_memory(code, destination, imports, exit_depth);
            store = true;
        }
        Operation::Leave => {
            stack::pointer::load(code, Register32::Ebp, 0, 4, imports, exit_depth);
            code.local_set(RESULT);
            stack::pointer::assigned(code, register(Register32::Ebp), 4);
            code.local_set(register(Register32::Esp))
                .local_get(RESULT)
                .local_set(register(Register32::Ebp));
        }
        Operation::LeaveWord => {
            stack::pointer::load(code, Register32::Ebp, 0, 2, imports, exit_depth);
            stack::pointer::assigned(code, register(Register32::Ebp), 2);
            code.local_set(register(Register32::Esp))
                .local_get(register(Register32::Ebp))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(Register32::Ebp));
        }
        Operation::Return { stack_adjust } => {
            memory::pop_return(code, stack_adjust, imports, exit_depth);
        }
        Operation::ReturnWord { stack_adjust } => {
            stack::pointer::load(code, Register32::Esp, 0, 2, imports, exit_depth);
            code.local_set(RESULT);
            check_near_value(code, RESULT, imports, exit_depth);
            code.local_get(RESULT).local_set(EIP);
            stack::pointer::advance(code, 2 + i32::from(stack_adjust));
        }
        _ => unreachable!("instruction routed to control emitter"),
    }
    store
}
