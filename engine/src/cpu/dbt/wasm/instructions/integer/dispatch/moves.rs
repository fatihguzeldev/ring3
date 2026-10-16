use super::*;

pub(super) fn emit(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedOperation,
    imports: Imports,
    exit_depth: u32,
) -> bool {
    let mut store = false;
    match *instruction.operation() {
        Operation::MoveByteSwap {
            width,
            destination,
            source,
        } => {
            store = super::super::scalar_bits::move_byte_swap(
                code,
                width,
                destination,
                source,
                imports,
                exit_depth,
            );
        }
        Operation::TranslateByteAddress { address } => {
            code.local_get(register(Register32::Ebx))
                .local_get(register(Register32::Eax))
                .i32_const(255)
                .i32_and()
                .i32_add();
            if address.address_size == crate::cpu::x86::ir::AddressSize::Bits16 {
                code.i32_const(65535).i32_and();
            }
            memory::checked_offset(
                code,
                address.resolved_segment(),
                1,
                crate::cpu::x86::SegmentAccess::Read,
                imports,
                exit_depth,
            );
            memory::load_narrow_at_address(code, SmallWidth::Byte, imports, exit_depth);
            insert_byte(code, ByteRegister::Al);
        }
        Operation::TranslateByte => {
            code.local_get(register(Register32::Ebx))
                .local_get(register(Register32::Eax))
                .i32_const(0xff)
                .i32_and()
                .i32_add();
            memory::checked_offset(
                code,
                crate::cpu::x86::SegmentRegister::Ds,
                1,
                crate::cpu::x86::SegmentAccess::Read,
                imports,
                exit_depth,
            );
            memory::load_narrow_at_address(code, SmallWidth::Byte, imports, exit_depth);
            insert_byte(code, ByteRegister::Al);
        }
        Operation::Move {
            destination: Location32::Register(destination),
            source,
        } => {
            if let Value32::Memory(address) = source {
                memory::load(code, address, destination, imports, exit_depth);
            } else {
                value(code, source);
                code.local_set(register(destination));
            }
        }
        Operation::Move {
            destination: Location32::Memory(address),
            source,
        } => {
            memory::store(code, address, source, imports, exit_depth);
            store = true;
        }
        Operation::MoveByte {
            destination,
            source,
        } => {
            byte_value(code, source);
            insert_byte(code, destination);
        }
        Operation::MoveWord {
            destination,
            source,
        } => {
            word_value(code, source);
            code.local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(destination));
        }
        Operation::LoadByte {
            destination,
            address,
        } => {
            memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
            let (parent, high) = match destination {
                ByteRegister::Al => (Register32::Eax, false),
                ByteRegister::Cl => (Register32::Ecx, false),
                ByteRegister::Dl => (Register32::Edx, false),
                ByteRegister::Bl => (Register32::Ebx, false),
                ByteRegister::Ah => (Register32::Eax, true),
                ByteRegister::Ch => (Register32::Ecx, true),
                ByteRegister::Dh => (Register32::Edx, true),
                ByteRegister::Bh => (Register32::Ebx, true),
            };
            if high {
                code.i32_const(8).i32_shl();
            }
            code.local_get(register(parent))
                .i32_const(if high { !0xff00 } else { !0xff })
                .i32_and()
                .i32_or()
                .local_set(register(parent));
        }
        Operation::StoreByte { address, source } => {
            memory::store_byte(code, address, source, imports, exit_depth);
            store = true;
        }
        Operation::LoadWord {
            destination,
            address,
        } => {
            memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
            code.local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(destination));
        }
        Operation::StoreSystemDescriptor {
            address,
            limit,
            base,
        } => {
            crate::cpu::dbt::wasm::instructions::system_registers::store_descriptor(
                code, address, limit, base, imports, exit_depth,
            );
            store = true;
        }
        Operation::StoreWord { address, source } => {
            word_value(code, source);
            code.local_set(RESULT);
            memory::store_word_result(code, address, imports, exit_depth);
            store = true;
        }
        Operation::ExtendByteToWord {
            kind,
            destination,
            source,
        } => {
            byte_value(code, ByteValue::Register(source));
            extend_byte_to_word(code, kind, destination);
        }
        Operation::ReadExtendByteToWord {
            kind,
            destination,
            address,
        } => {
            memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
            extend_byte_to_word(code, kind, destination);
        }
        Operation::Extend {
            kind,
            destination,
            source,
        } => {
            let width = match source {
                SmallSource::Register {
                    register: source,
                    width,
                    high_byte,
                } => {
                    code.local_get(register(source));
                    if high_byte {
                        code.i32_const(8).i32_shr_u();
                    }
                    width
                }
                SmallSource::Memory { address, width } => {
                    memory::load_narrow_value(code, address, width, imports, exit_depth);
                    width
                }
            };
            extend_value(code, kind, width);
            code.local_set(register(destination));
        }
        Operation::ByteSwap { destination } => {
            let destination = register(destination);
            code.local_get(destination)
                .i32_const(24)
                .i32_shl()
                .local_get(destination)
                .i32_const(0xff00)
                .i32_and()
                .i32_const(8)
                .i32_shl()
                .i32_or()
                .local_get(destination)
                .i32_const(8)
                .i32_shr_u()
                .i32_const(0xff00)
                .i32_and()
                .i32_or()
                .local_get(destination)
                .i32_const(24)
                .i32_shr_u()
                .i32_or()
                .local_set(destination);
        }
        Operation::Lea {
            destination,
            address,
        } => {
            memory::offset_value(code, address);
            code.local_set(register(destination));
        }
        Operation::LeaWord {
            destination,
            address,
        } => {
            memory::offset_value(code, address);
            code.i32_const(0xffff)
                .i32_and()
                .local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(destination));
        }
        _ => unreachable!("instruction routed to moves emitter"),
    }
    store
}
