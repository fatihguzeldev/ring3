use super::*;
use crate::{
    abi::{arena::X87_OFFSET, x86::X87_SIZE},
    cpu::x86::{
        State32, X87Binary, X87Format, X87Operation,
        ir::{EffectiveAddress, Operation},
    },
    memory::GuestAddress,
};

fn fixture() -> (EngineInstance, NumericBank, Context32) {
    let mut instance = EngineInstance::new(3, 7).unwrap();
    instance.map(0x2000, 1, 3).unwrap();
    let bank = NumericBank::from_instance(&instance).unwrap();
    (instance, bank, Context32::new(State32::default()))
}

fn instruction(operation: X87Operation, address: Option<u32>) -> NativeInstruction {
    NativeInstruction {
        pc: 0x1000,
        next: 0x1002,
        operation: Operation::X87 {
            operation,
            address: address.map(|displacement| EffectiveAddress {
                displacement,
                ..EffectiveAddress::default()
            }),
            waiting: true,
            opcode: 0x100,
        },
    }
}

#[test]
fn typed_x87_load_multiply_store_keeps_the_arena_cold() {
    let (mut instance, mut bank, mut context) = fixture();
    let before = instance.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE].to_vec();
    instance
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x2000), &3_f32.to_le_bytes())
        .unwrap();
    instance
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x2004), &2_f32.to_le_bytes())
        .unwrap();
    for op in [
        instruction(X87Operation::Load(X87Format::Float32), Some(0x2000)),
        instruction(
            X87Operation::BinaryMemory {
                kind: X87Binary::Multiply,
                format: X87Format::Float32,
            },
            Some(0x2004),
        ),
        instruction(
            X87Operation::Store {
                format: X87Format::Float32,
                pop: true,
                truncate: false,
            },
            Some(0x2008),
        ),
    ] {
        assert_eq!(bank.execute(&mut instance, &mut context, &op), 0);
    }
    let mut result = [0; 4];
    instance
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x2008), &mut result)
        .unwrap();
    assert_eq!(result, 6_f32.to_le_bytes());
    assert_eq!(bank.x87.tag, 0xffff);
    assert_eq!(bank.x87.status & 0x3800, 0);
    assert_eq!(
        (
            bank.x87.instruction_pointer,
            bank.x87.data_pointer,
            bank.x87.code_selector,
            bank.x87.data_selector
        ),
        (0x1000, 0x2008, 0x1b, 0x23)
    );
    assert_eq!(bank.x87.registers[7], [0, 0, 0, 0, 0, 0, 0, 0xc0, 1, 0x40]);
    assert_eq!(&instance.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE], before);
    bank.publish_to_instance(&mut instance).unwrap();
    assert_ne!(&instance.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE], before);
}

#[test]
fn x87_store_fault_preserves_the_typed_stack_and_general_state() {
    let (mut instance, mut bank, mut context) = fixture();
    instance
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x2000), &3_f32.to_le_bytes())
        .unwrap();
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &instruction(X87Operation::Load(X87Format::Float32), Some(0x2000))
        ),
        0
    );
    let before = bank.x87;
    let general = context.state();
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &instruction(
                X87Operation::Store {
                    format: X87Format::Float32,
                    pop: true,
                    truncate: false
                },
                Some(0x3000)
            )
        ),
        StopReason::MemoryFault as u32
    );
    assert_eq!(bank.x87, before);
    assert_eq!(context.state(), general);
    assert_eq!(
        u32::from_le_bytes(context.bytes()[48..52].try_into().unwrap()),
        0x3000
    );
}

#[test]
fn waiting_x87_fault_precedes_missing_operand_memory() {
    let (mut instance, mut bank, mut context) = fixture();
    bank.x87.control &= !1;
    bank.x87.status = 1;
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &instruction(X87Operation::Load(X87Format::Float32), Some(0x3000))
        ),
        StopReason::CpuFault as u32
    );
    assert_eq!(bank.x87.status, 0x8081);
    assert_eq!(
        u32::from_le_bytes(context.bytes()[116..120].try_into().unwrap()),
        16
    );
}

#[test]
fn newly_unmasked_x87_exception_is_deferred_until_a_waiting_instruction() {
    let (mut instance, mut bank, mut context) = fixture();
    bank.x87.control &= !1;
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &instruction(X87Operation::Load(X87Format::Float32), Some(0x2000))
        ),
        0
    );
    let payload = bank.x87.registers[7];
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &instruction(
                X87Operation::BinaryMemory {
                    kind: X87Binary::Divide,
                    format: X87Format::Float32
                },
                Some(0x2000)
            )
        ),
        0
    );
    assert_eq!(bank.x87.status & 0x8081, 0x8081);
    assert_eq!(bank.x87.registers[7], payload);
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &instruction(X87Operation::Wait, None)
        ),
        9
    );
    assert_eq!(&context.bytes()[116..120], &16_u32.to_le_bytes());
}

#[test]
fn cold_publication_validates_all_banks_before_changing_the_arena() {
    let (mut instance, mut bank, _) = fixture();
    let before = instance.arena().to_vec();
    bank.x87.control = 0x027f;
    bank.vectors.simd.mxcsr = u32::MAX;
    assert!(bank.publish_to_instance(&mut instance).is_err());
    assert_eq!(instance.arena(), before);
}

fn vector_operation(kind: crate::cpu::x86::simd::SimdKind) -> crate::cpu::x86::simd::SimdOperation {
    use crate::cpu::x86::simd::{SimdKind, SimdOperand, SimdOperation};
    let state_only = matches!(
        kind,
        SimdKind::EmptyMmx | SimdKind::SaveExtended | SimdKind::RestoreExtended
    );
    SimdOperation {
        kind,
        destination: SimdOperand::None,
        source: SimdOperand::None,
        vector_bytes: if state_only { 4 } else { 16 },
        lane_bytes: if state_only { 0 } else { 4 },
        memory_bytes: 0,
        immediate: None,
        aligned: false,
        scalar: false,
        implicit_address: None,
    }
}

fn native(operation: Operation) -> NativeInstruction {
    NativeInstruction {
        pc: 0x1000,
        next: 0x1003,
        operation,
    }
}

#[test]
fn simd_unmasked_invalid_publishes_only_sticky_mxcsr() {
    use crate::cpu::x86::simd::{FloatKind, SimdKind, SimdOperand, SimdRegister};
    let (mut instance, mut bank, mut context) = fixture();
    bank.vectors.simd.mxcsr = 0x1f00;
    bank.vectors.simd.xmm[0][4..].fill(0x5a);
    let destination = bank.vectors.simd.xmm[0];
    let mut op = vector_operation(SimdKind::Float(FloatKind::Divide));
    op.scalar = true;
    op.destination = SimdOperand::Register(SimdRegister::Xmm(0));
    op.source = SimdOperand::Register(SimdRegister::Xmm(1));
    assert_eq!(
        bank.execute(&mut instance, &mut context, &native(Operation::Simd(op))),
        9
    );
    assert_eq!(bank.vectors.simd.mxcsr, 0x1f01);
    assert_eq!(bank.vectors.simd.xmm[0], destination);
    assert_eq!(
        u32::from_le_bytes(context.bytes()[116..120].try_into().unwrap()),
        19
    );
}

#[test]
fn mmx_and_emms_share_the_physical_x87_bank() {
    use crate::cpu::x86::{
        Register32,
        simd::{SimdKind, SimdOperand, SimdRegister},
    };
    let (mut instance, mut bank, mut context) = fixture();
    let mut state = context.state();
    state.registers[Register32::Eax.index()] = 0x76543210;
    context.set_state(state);
    bank.x87.status = 0x3800;
    let mut op = vector_operation(SimdKind::Move);
    op.vector_bytes = 8;
    op.destination = SimdOperand::Register(SimdRegister::Mmx(0));
    op.source = SimdOperand::Register(SimdRegister::Gpr(Register32::Eax));
    assert_eq!(
        bank.execute(&mut instance, &mut context, &native(Operation::Simd(op))),
        0
    );
    assert_eq!(
        bank.x87.registers[0],
        [0x10, 0x32, 0x54, 0x76, 0, 0, 0, 0, 255, 255]
    );
    assert_eq!((bank.x87.tag, bank.x87.status & 0x3800), (0, 0));
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &native(Operation::Simd(vector_operation(SimdKind::EmptyMmx)))
        ),
        0
    );
    assert_eq!(bank.x87.tag, 0xffff);
    assert_eq!(&bank.x87.registers[0][..4], &[0x10, 0x32, 0x54, 0x76]);
}

#[test]
fn fxsave_preserves_reserved_tail_and_fxrstor_rejects_before_publication() {
    use crate::cpu::x86::simd::{SimdKind, SimdOperand};
    let (mut instance, mut bank, mut context) = fixture();
    instance
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x2000), &[0xa5; 512])
        .unwrap();
    bank.vectors.simd.xmm[0] = [0x33; 16];
    let address = EffectiveAddress {
        displacement: 0x2000,
        ..EffectiveAddress::default()
    };
    let mut save = vector_operation(SimdKind::SaveExtended);
    save.destination = SimdOperand::Memory(address);
    save.memory_bytes = 512;
    save.aligned = true;
    assert_eq!(
        bank.execute(&mut instance, &mut context, &native(Operation::Simd(save))),
        0
    );
    let mut image = [0; 512];
    instance
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x2000), &mut image)
        .unwrap();
    assert_eq!(&image[464..], &[0xa5; 48]);
    assert_eq!(&image[160..176], &[0x33; 16]);
    assert_eq!(&image[24..28], &0x1f80_u32.to_le_bytes());
    instance
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x2018), &0x8000_0000_u32.to_le_bytes())
        .unwrap();
    let x87 = bank.x87;
    let vectors = bank.vectors;
    let mut restore = vector_operation(SimdKind::RestoreExtended);
    restore.source = SimdOperand::Memory(address);
    restore.memory_bytes = 512;
    restore.aligned = true;
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &native(Operation::Simd(restore))
        ),
        9
    );
    assert_eq!(bank.x87, x87);
    assert_eq!(bank.vectors, vectors);
    assert_eq!(
        u32::from_le_bytes(context.bytes()[116..120].try_into().unwrap()),
        13
    );
}

#[test]
fn processor_queries_and_entry_boundaries_use_the_typed_context() {
    use crate::cpu::x86::{ControlOperation, Register32};
    let (mut instance, mut bank, mut context) = fixture();
    assert_eq!(
        bank.execute(&mut instance, &mut context, &native(Operation::CpuId)),
        0
    );
    let state = context.state();
    assert_eq!(state.registers[Register32::Eax.index()], 13);
    assert_eq!(
        state.registers[Register32::Ebx.index()],
        u32::from_le_bytes(*b"Ring")
    );
    let before = context.bytes().to_owned();
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &native(Operation::VirtualTimestamp)
        ),
        8
    );
    assert_eq!(context.bytes(), &before);
    instance.configure_cpu_clock(1_000_000_000, 100).unwrap();
    instance.observe_cpu_clock(117).unwrap();
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &native(Operation::VirtualTimestampAux)
        ),
        0
    );
    assert_eq!(context.state().registers[Register32::Eax.index()], 17);
    assert_eq!(context.state().registers[Register32::Ecx.index()], 0);
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &native(Operation::Control(ControlOperation::Sysenter))
        ),
        10
    );
    assert_eq!(&context.bytes()[116..120], &0x100_u32.to_le_bytes());
    assert_eq!(&context.bytes()[48..52], &0x1000_u32.to_le_bytes());
}

#[test]
fn packed_simd_uses_fs_translation_and_alignment_precedes_memory() {
    use crate::cpu::x86::{
        SegmentRegister,
        simd::{FloatKind, SimdKind, SimdOperand, SimdRegister},
    };
    let (mut instance, mut bank, mut context) = fixture();
    bank.segments.state.bases[SegmentRegister::Fs.index()] = 0x2000;
    let values = [1_f32, 2_f32, 3_f32, 4_f32].map(f32::to_le_bytes).concat();
    instance
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x2010), &values)
        .unwrap();
    let mut load = vector_operation(SimdKind::Move);
    load.lane_bytes = 16;
    load.destination = SimdOperand::Register(SimdRegister::Xmm(0));
    load.source = SimdOperand::Memory(EffectiveAddress {
        displacement: 16,
        segment: Some(SegmentRegister::Fs),
        ..EffectiveAddress::default()
    });
    load.memory_bytes = 16;
    load.aligned = true;
    assert_eq!(
        bank.execute(&mut instance, &mut context, &native(Operation::Simd(load))),
        0
    );
    assert_eq!(bank.vectors.simd.xmm[0].as_slice(), values);
    let mut add = vector_operation(SimdKind::Float(FloatKind::Add));
    add.source = SimdOperand::Register(SimdRegister::Xmm(0));
    add.destination = add.source;
    assert_eq!(
        bank.execute(&mut instance, &mut context, &native(Operation::Simd(add))),
        0
    );
    assert_eq!(
        bank.vectors.simd.xmm[0].as_slice(),
        [2_f32, 4_f32, 6_f32, 8_f32].map(f32::to_le_bytes).concat()
    );
    let vectors = bank.vectors;
    load.source = SimdOperand::Memory(EffectiveAddress {
        displacement: 0x1001,
        segment: Some(SegmentRegister::Fs),
        ..EffectiveAddress::default()
    });
    assert_eq!(
        bank.execute(&mut instance, &mut context, &native(Operation::Simd(load))),
        9
    );
    assert_eq!(bank.vectors, vectors);
    assert_eq!(&context.bytes()[116..120], &13_u32.to_le_bytes());
}

#[test]
fn xsave_xrstor_copies_selected_architectural_fields_and_preserves_other_bytes() {
    use crate::cpu::x86::{Register32, xstate::XStateOperation};
    let (mut instance, mut bank, mut context) = fixture();
    bank.x87.control = 0x027f;
    bank.x87.status = 0x3800;
    bank.x87.tag = 0x3fff;
    bank.x87.registers[7] = [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f];
    bank.vectors.simd.xmm[0] = [0x33; 16];
    bank.vectors.upper[0] = [0x55; 16];
    let x87 = bank.x87;
    let vectors = bank.vectors;
    let mut general = context.state();
    general.registers[Register32::Eax.index()] = 7;
    context.set_state(general);
    let memory = instance.memory.as_mut().unwrap();
    memory.write(GuestAddress(0x2000), &[0xa5; 832]).unwrap();
    memory.write(GuestAddress(0x2200), &[0; 24]).unwrap();
    let address = EffectiveAddress {
        displacement: 0x2000,
        ..EffectiveAddress::default()
    };
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &native(Operation::XState(XStateOperation::Save { address }))
        ),
        0
    );
    let mut image = [0; 832];
    instance
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x2000), &mut image)
        .unwrap();
    assert_eq!(&image[..8], &[0x7f, 2, 0, 0x38, 0x80, 0, 0, 0]);
    assert_eq!(&image[32..42], &[0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f]);
    assert_eq!(&image[160..176], &[0x33; 16]);
    assert_eq!(
        &image[512..536],
        &[
            7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
        ]
    );
    assert_eq!(&image[576..592], &[0x55; 16]);
    assert_eq!(&image[288..512], &[0xa5; 224]);
    assert_eq!(&image[704..832], &[0xa5; 128]);
    bank.x87 = X87State::default();
    bank.vectors = VectorState::default();
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &native(Operation::XState(XStateOperation::Restore { address }))
        ),
        0
    );
    assert_eq!(bank.x87, x87);
    assert_eq!(bank.vectors, vectors);
}

#[test]
fn x87_precision_controls_produce_literal_extended_results() {
    for (control, exponent, result_exponent) in [
        (0x007f, 25, None),
        (0x027f, 25, Some(0x3fe6_u16)),
        (0x037f, 25, Some(0x3fe6_u16)),
        (0x007f, 60, None),
        (0x027f, 60, None),
        (0x037f, 60, Some(0x3fc3_u16)),
    ] {
        let (mut instance, mut bank, mut context) = fixture();
        bank.x87.control = control;
        let memory = instance.memory.as_mut().unwrap();
        memory
            .write(GuestAddress(0x2000), &1_f64.to_le_bytes())
            .unwrap();
        let tiny: u64 = if exponent == 25 {
            0x3e60_0000_0000_0000
        } else {
            0x3c30_0000_0000_0000
        };
        memory
            .write(GuestAddress(0x2008), &tiny.to_le_bytes())
            .unwrap();
        for operation in [
            instruction(X87Operation::Load(X87Format::Float64), Some(0x2000)),
            instruction(
                X87Operation::BinaryMemory {
                    kind: X87Binary::Add,
                    format: X87Format::Float64,
                },
                Some(0x2008),
            ),
            instruction(
                X87Operation::BinaryMemory {
                    kind: X87Binary::Subtract,
                    format: X87Format::Float64,
                },
                Some(0x2000),
            ),
            instruction(
                X87Operation::Store {
                    format: X87Format::Extended80,
                    pop: true,
                    truncate: false,
                },
                Some(0x2020),
            ),
        ] {
            assert_eq!(bank.execute(&mut instance, &mut context, &operation), 0);
        }
        let mut result = [0; 10];
        instance
            .memory
            .as_ref()
            .unwrap()
            .read(GuestAddress(0x2020), &mut result)
            .unwrap();
        let mut expected = [0; 10];
        if let Some(exponent) = result_exponent {
            expected[7] = 0x80;
            expected[8..].copy_from_slice(&exponent.to_le_bytes());
        }
        assert_eq!(result, expected, "control={control:x} tiny=2^-{exponent}");
    }
}

#[test]
fn x87_store_rolls_back_after_successful_preflight_if_ram_cannot_commit() {
    let (mut instance, mut bank, mut context) = fixture();
    instance
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x2000), &3_f32.to_le_bytes())
        .unwrap();
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &instruction(X87Operation::Load(X87Format::Float32), Some(0x2000))
        ),
        0
    );
    let before = bank.x87;
    let state = context.state();
    instance
        .memory
        .as_mut()
        .unwrap()
        .exhaust_versions_for_test();
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &instruction(
                X87Operation::Store {
                    format: X87Format::Float32,
                    pop: true,
                    truncate: false
                },
                Some(0x2008)
            )
        ),
        8
    );
    assert_eq!(bank.x87, before);
    assert_eq!(context.state(), state);
    let mut result = [0; 4];
    instance
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x2008), &mut result)
        .unwrap();
    assert_eq!(result, [0; 4]);
}

#[test]
fn stack_push_all_saves_original_pointer_and_commits_one_complete_image() {
    use crate::cpu::x86::StackFrameKind;
    for kind in [StackFrameKind::PushAll16, StackFrameKind::PushAll32] {
        let (mut instance, mut bank, mut context) = fixture();
        instance
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x2000), &[0xa5; 4096])
            .unwrap();
        let registers = [
            0x11112222, 0x33334444, 0x55556666, 0x77778888, 0x2080, 0x9999aaaa, 0xbbbbcccc,
            0xddddeeee,
        ];
        let state = State32 {
            registers,
            eip: 0x1000,
            eflags: 0x893,
        };
        context.set_state(state);
        let width = kind.width() as usize;
        let mut expected = [0xa5; 4096];
        for (index, value) in registers.into_iter().enumerate() {
            let offset = 0x80 - (index + 1) * width;
            expected[offset..offset + width].copy_from_slice(&value.to_le_bytes()[..width]);
        }
        assert_eq!(
            bank.execute(
                &mut instance,
                &mut context,
                &native(Operation::StackFrame {
                    kind,
                    allocation: 0,
                    nesting: 0
                })
            ),
            0
        );
        let mut actual = [0; 4096];
        instance
            .memory
            .as_ref()
            .unwrap()
            .read(GuestAddress(0x2000), &mut actual)
            .unwrap();
        assert_eq!(actual, expected);
        let mut wanted = state;
        wanted.registers[4] -= (8 * width) as u32;
        assert_eq!(context.state(), wanted);
    }
}

#[test]
fn stack_late_push_fault_keeps_all_ram_and_registers_unmodified() {
    use crate::cpu::x86::StackFrameKind;
    for kind in [StackFrameKind::PushAll16, StackFrameKind::PushAll32] {
        let (mut instance, mut bank, mut context) = fixture();
        let width = kind.width();
        let state = State32 {
            registers: [1, 2, 3, 4, 0x2000 + width, 6, 7, 8],
            eip: 0x1000,
            eflags: 0x893,
        };
        context.set_state(state);
        assert_eq!(
            bank.execute(
                &mut instance,
                &mut context,
                &native(Operation::StackFrame {
                    kind,
                    allocation: 0,
                    nesting: 0
                })
            ),
            3
        );
        assert_eq!(context.state(), state);
        let mut actual = [1; 4096];
        instance
            .memory
            .as_ref()
            .unwrap()
            .read(GuestAddress(0x2000), &mut actual)
            .unwrap();
        assert_eq!(actual, [0; 4096]);
        assert_eq!(&context.bytes()[48..52], &(0x2000 - width).to_le_bytes());
        assert_eq!(
            u32::from_le_bytes(context.bytes()[52..56].try_into().unwrap()),
            width | 0x10200
        );
    }
}

#[test]
fn stack_enter_reads_the_staged_frame_and_checks_allocation_before_commit() {
    use crate::cpu::x86::StackFrameKind;
    let (mut instance, mut bank, mut context) = fixture();
    instance
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x2000), &[0xa5; 4096])
        .unwrap();
    let state = State32 {
        registers: [1, 2, 3, 4, 0x2080, 0x2080, 7, 8],
        eip: 0x1000,
        eflags: 0x893,
    };
    context.set_state(state);
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &native(Operation::StackFrame {
                kind: StackFrameKind::Enter32,
                allocation: 16,
                nesting: 3
            })
        ),
        0
    );
    assert_eq!(
        context.state().registers,
        [1, 2, 3, 4, 0x2060, 0x207c, 7, 8]
    );
    assert_eq!(context.state().eflags, 0x893);
    let mut actual = [0; 32];
    instance
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x2060), &mut actual)
        .unwrap();
    let mut expected = [0xa5; 32];
    for (offset, value) in [(16, 0x207c_u32), (20, 0x2080), (24, 0x2080), (28, 0x2080)] {
        expected[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(actual, expected);
    let before_page = {
        let mut page = [0; 4096];
        instance
            .memory
            .as_ref()
            .unwrap()
            .read(GuestAddress(0x2000), &mut page)
            .unwrap();
        page
    };
    let mut failed = state;
    failed.registers[4] = 0x2004;
    context.set_state(failed);
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &native(Operation::StackFrame {
                kind: StackFrameKind::Enter32,
                allocation: 8,
                nesting: 0
            })
        ),
        3
    );
    assert_eq!(context.state(), failed);
    let mut after = [0; 4096];
    instance
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x2000), &mut after)
        .unwrap();
    assert_eq!(after, before_page);
    assert_eq!(&context.bytes()[48..52], &0x1ff8_u32.to_le_bytes());
}

#[test]
fn stack_code_write_reports_the_completed_transaction() {
    use crate::cpu::x86::StackFrameKind;
    let (mut instance, mut bank, mut context) = fixture();
    instance.map(0x1000, 1, 7).unwrap();
    let state = State32 {
        registers: [1, 2, 3, 4, 0x1100, 6, 7, 8],
        eip: 0x1000,
        eflags: 0x893,
    };
    context.set_state(state);
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &native(Operation::StackFrame {
                kind: StackFrameKind::PushAll32,
                allocation: 0,
                nesting: 0
            })
        ),
        6
    );
    assert_eq!(context.state().registers, [1, 2, 3, 4, 0x10e0, 6, 7, 8]);
    let mut actual = [0; 32];
    instance
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x10e0), &mut actual)
        .unwrap();
    let expected = [8_u32, 7, 6, 0x1100, 4, 3, 2, 1]
        .map(u32::to_le_bytes)
        .concat();
    assert_eq!(actual.as_slice(), expected);
}

#[test]
fn x87_sparse_context_uses_wrapped_fs_operands_without_publishing_general_state() {
    use crate::cpu::x86::{Register32, SegmentRegister, ir::AddressSize};
    let (mut instance, mut bank, mut context) = fixture();
    context.set_state(State32 {
        registers: [
            0xa5a55a5a, 0x11112222, 0x33334444, 0x1234fff0, 0x55556666, 0x77778888, 0xabcd0030,
            0x9999aaaa,
        ],
        eip: 0x12345678,
        eflags: 0xad7,
    });
    context.bytes_mut()[40..].fill(0xa5);
    bank.segments.state.bases[SegmentRegister::Fs.index()] = 0x2000;
    instance
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x2030), &3_f32.to_le_bytes())
        .unwrap();
    let before = *context.bytes();
    let arena = instance.arena().to_vec();
    let operation = native(Operation::X87 {
        operation: X87Operation::Load(X87Format::Float32),
        address: Some(EffectiveAddress {
            base: Some(Register32::Ebx),
            index: Some(Register32::Esi),
            scale: 1,
            displacement: 0x10,
            segment: Some(SegmentRegister::Fs),
            address_size: AddressSize::Bits16,
        }),
        waiting: true,
        opcode: 0x123,
    });
    assert_eq!(bank.execute(&mut instance, &mut context, &operation), 0);
    let mut expected = X87State {
        status: 0x3800,
        tag: 0x3fff,
        instruction_pointer: 0x1000,
        data_pointer: 0x30,
        opcode: 0x123,
        code_selector: 0x1b,
        data_selector: 0x3b,
        ..X87State::default()
    };
    expected.registers[7] = [0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0x40];
    assert_eq!(bank.x87, expected);
    assert_eq!(context.bytes(), &before);
    assert_eq!(instance.arena(), arena);
}

#[test]
fn x87_sparse_outputs_preserve_flags_and_status_to_ax_high_half() {
    use crate::cpu::x86::X87Condition;
    let (mut instance, mut bank, mut context) = fixture();
    context.set_state(State32 {
        registers: [0xa5a55a5a, 2, 3, 4, 5, 6, 7, 8],
        eip: 0x12345678,
        eflags: 0xad7,
    });
    context.bytes_mut()[40..].fill(0xa5);
    let mut expected = *context.bytes();
    bank.x87.status = 0x4500;
    bank.x87.tag = 0xfff0;
    bank.x87.registers[0] = [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f];
    bank.x87.registers[1] = [0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0x40];
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &instruction(
                X87Operation::ConditionalMove {
                    index: 1,
                    condition: X87Condition::Below,
                },
                None,
            ),
        ),
        0
    );
    assert_eq!(bank.x87.registers[0], bank.x87.registers[1]);
    assert_eq!(context.bytes(), &expected);
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &instruction(
                X87Operation::Compare {
                    index: 1,
                    quiet: false,
                    pop: 0,
                    integer_flags: true,
                },
                None,
            ),
        ),
        0
    );
    expected[36..40].copy_from_slice(&0x242_u32.to_le_bytes());
    assert_eq!(context.bytes(), &expected);
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &instruction(X87Operation::StatusToAx, None),
        ),
        0
    );
    expected[..4].copy_from_slice(&0xa5a54500_u32.to_le_bytes());
    assert_eq!(context.bytes(), &expected);
    bank.x87.control = 0x037e;
    bank.x87.registers[0] = [0, 0, 0, 0, 0, 0, 0, 0xc0, 0xff, 0xff];
    let payload = bank.x87.registers;
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &instruction(
                X87Operation::Compare {
                    index: 1,
                    quiet: false,
                    pop: 1,
                    integer_flags: true,
                },
                None,
            ),
        ),
        0
    );
    assert_eq!(context.bytes(), &expected);
    assert_eq!(bank.x87.status, 0xc581);
    assert_eq!(bank.x87.tag, 0xfff0);
    assert_eq!(bank.x87.registers, payload);
}

#[test]
fn x87_evaluator_failure_restores_instruction_metadata_as_well_as_the_stack() {
    let (mut instance, mut bank, mut context) = fixture();
    bank.x87.instruction_pointer = 0x11223344;
    bank.x87.data_pointer = 0x55667788;
    bank.x87.opcode = 0x321;
    bank.x87.code_selector = 0x43;
    bank.x87.data_selector = 0x4b;
    bank.x87.status = 0x3800;
    bank.x87.tag = 0x3fff;
    bank.x87.registers[7] = [0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0x40];
    let before = bank.x87;
    let general = *context.bytes();
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &instruction(
                X87Operation::Binary {
                    kind: X87Binary::Add,
                    destination: 0,
                    source: 8,
                    pop: false,
                },
                None,
            ),
        ),
        8
    );
    assert_eq!(bank.x87, before);
    assert_eq!(context.bytes(), &general);
}

#[test]
fn x87_full_save_restore_uses_the_complete_108_byte_image() {
    let (mut instance, mut bank, mut context) = fixture();
    bank.x87.control = 0x027f;
    bank.x87.status = 0x3800;
    bank.x87.tag = 0x3fff;
    bank.x87.instruction_pointer = 0x11223344;
    bank.x87.data_pointer = 0x55667788;
    bank.x87.opcode = 0x321;
    bank.x87.code_selector = 0x43;
    bank.x87.data_selector = 0x4b;
    for (index, register) in bank.x87.registers.iter_mut().enumerate() {
        register.fill(index as u8 + 1);
    }
    let before = bank.x87;
    let general = *context.bytes();
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &instruction(X87Operation::Save { operand16: false }, Some(0x2000)),
        ),
        0
    );
    let mut image = [0; 108];
    instance
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x2000), &mut image)
        .unwrap();
    assert_eq!(
        &image[..28],
        &[
            0x7f, 2, 0xff, 0xff, 0, 0x38, 0xff, 0xff, 0xff, 0x3f, 0xff, 0xff, 0x44, 0x33, 0x22,
            0x11, 0x43, 0, 0x21, 3, 0x88, 0x77, 0x66, 0x55, 0x4b, 0, 0, 0,
        ]
    );
    for (index, value) in [8, 1, 2, 3, 4, 5, 6, 7].into_iter().enumerate() {
        assert_eq!(image[28 + index * 10..38 + index * 10], [value; 10]);
    }
    assert_eq!(bank.x87.control, 0x037f);
    assert_eq!(bank.x87.registers, before.registers);
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &instruction(X87Operation::Restore { operand16: false }, Some(0x2000)),
        ),
        0
    );
    assert_eq!(bank.x87, before);
    assert_eq!(context.bytes(), &general);
}

#[test]
fn x87_environment_outputs_roll_back_control_tags_metadata_and_every_payload_on_late_write_failure()
{
    for operation in [
        X87Operation::StoreEnvironment { operand16: true },
        X87Operation::StoreEnvironment { operand16: false },
        X87Operation::Save { operand16: true },
        X87Operation::Save { operand16: false },
    ] {
        let (mut instance, mut bank, mut context) = fixture();
        bank.x87.control = 0x2340;
        bank.x87.status = 0x5f00;
        bank.x87.tag = 0xff3f;
        bank.x87.instruction_pointer = 0x1122_3344;
        bank.x87.data_pointer = 0x5566_7788;
        bank.x87.opcode = 0x321;
        bank.x87.code_selector = 0x43;
        bank.x87.data_selector = 0x4b;
        for (index, register) in bank.x87.registers.iter_mut().enumerate() {
            register.fill(index as u8 + 1);
        }
        let before = bank.x87;
        let general = *context.bytes();
        let arena = instance.arena().to_vec();
        let memory = instance.memory.as_mut().unwrap();
        memory.write(GuestAddress(0x2000), &[0xa5; 4096]).unwrap();
        memory.exhaust_versions_for_test();
        assert_eq!(
            bank.execute(
                &mut instance,
                &mut context,
                &instruction(operation, Some(0x2000)),
            ),
            8,
            "{operation:?}",
        );
        assert_eq!(bank.x87, before, "{operation:?}");
        assert_eq!(context.bytes(), &general, "{operation:?}");
        assert_eq!(instance.arena(), arena, "{operation:?}");
        let mut actual = [0; 4096];
        instance
            .memory
            .as_ref()
            .unwrap()
            .read(GuestAddress(0x2000), &mut actual)
            .unwrap();
        assert_eq!(actual, [0xa5; 4096], "{operation:?}");
    }
}
