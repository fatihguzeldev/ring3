use super::*;
use crate::cpu::{
    core::fp_pool::{FpLeasePool, FpLeaseRecord},
    x86::{Register32, StackFrameKind, State32, ir::EffectiveAddress},
};

fn native(operation: Operation) -> NativeInstruction {
    NativeInstruction {
        pc: 0x1234,
        next: 0x1237,
        operation,
    }
}

fn put(image: &mut [u8], offset: usize, value: u32) {
    image[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

// Serialize every defined pool field as raw bits, including signed zero and
// stale raw limbs behind native records. Reserved fields are not a CPU value.
fn pool_image(pool: &FpLeasePool) -> Vec<u8> {
    let mut image = vec![0; 384];
    for (offset, value) in [
        (0, pool.active),
        (4, pool.control),
        (8, pool.status),
        (12, pool.tag),
        (16, pool.top),
        (20, pool.fip),
        (24, pool.fdp),
        (28, pool.opcode),
        (32, pool.cs),
        (36, pool.ds),
        (40, pool.pending),
        (76, pool.changed),
    ] {
        put(&mut image, offset, value);
    }
    for (offset, value) in [
        (48, pool.pending_left),
        (56, pool.pending_right),
        (64, pool.pending_result),
    ] {
        image[offset..offset + 8].copy_from_slice(&value.to_bits().to_le_bytes());
    }
    for (index, record) in pool.records.iter().enumerate() {
        let offset = 128 + index * 32;
        image[offset..offset + 8].copy_from_slice(&record.native.to_bits().to_le_bytes());
        image[offset + 8..offset + 16].copy_from_slice(&record.low.to_le_bytes());
        for (field, value) in [
            (16, record.high),
            (20, record.valid),
            (24, record.proofs),
            (28, record.control),
        ] {
            put(&mut image, offset + field, value);
        }
    }
    image
}

fn fixture() -> (EngineInstance, NumericBank, Context32) {
    let mut instance = EngineInstance::new(3, 7).unwrap();
    instance.map(0x2000, 1, 3).unwrap();
    instance
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x2000), &[0xa5; 4096])
        .unwrap();
    let mut bank = NumericBank::from_instance(&instance).unwrap();
    // This canonical image deliberately differs from the active private pool.
    // A preserving callback must neither consult it nor publish into it.
    bank.x87.control = 0x037f;
    bank.x87.status = 0x0841;
    bank.x87.tag = 0xfff3;
    bank.x87.opcode = 0x321;
    bank.x87.instruction_pointer = 0x55667788;
    bank.x87.data_pointer = 0x11223344;
    bank.x87.code_selector = 0x1b;
    bank.x87.data_selector = 0x23;
    bank.x87.registers = [[0x5a; 10]; 8];
    bank.vectors.simd.xmm = [[0x39; 16]; 8];
    bank.vectors.upper = [[0xc6; 16]; 8];
    let pool = &mut bank.pool;
    pool.active = 1;
    pool.control = 0x027f;
    pool.status = 0x20;
    pool.tag = 0x3ff0;
    pool.top = 7;
    pool.fip = 0x12345678;
    pool.fdp = 0x87654321;
    pool.opcode = 0x43;
    pool.cs = 0x1b;
    pool.ds = 0x23;
    pool.pending = 1;
    // 1+2^-52 plus half an ulp rounds upward to 1+2^-51 (C1=1).
    pool.pending_left = f64::from_bits(0x3ff0000000000001);
    pool.pending_right = f64::from_bits(0x3ca0000000000000);
    pool.pending_result = f64::from_bits(0x3ff0000000000002);
    pool.changed = 0x85;
    pool.records = [
        FpLeaseRecord {
            native: 3.5,
            low: 0x0102030405060708,
            high: 0x3456,
            valid: 1,
            proofs: 15,
            control: 0x027f,
        },
        FpLeaseRecord {
            native: 0.0,
            low: 0xc000000000000001,
            high: 0x4000,
            valid: 0,
            proofs: 0,
            control: 0,
        },
        FpLeaseRecord {
            native: -0.0,
            low: 0,
            high: 0x8000,
            valid: 1,
            proofs: 15,
            control: 0x027f,
        },
        FpLeaseRecord {
            native: 0.0,
            low: 0,
            high: 0x4000,
            valid: 0,
            proofs: 0,
            control: 0,
        },
        FpLeaseRecord {
            native: 2.0,
            low: 0x8000000000000000,
            high: 0x4000,
            valid: 1,
            proofs: 11,
            control: 0x027f,
        },
        FpLeaseRecord {
            native: 0.0,
            low: 0xc000000000000000,
            high: 0x7fff,
            valid: 0,
            proofs: 0,
            control: 0,
        },
        FpLeaseRecord {
            native: 5.0,
            low: 0xa000000000000000,
            high: 0x4001,
            valid: 1,
            proofs: 3,
            control: 0x027f,
        },
        FpLeaseRecord {
            native: f64::from_bits(0x3ff0000000000002),
            low: 0x8000000000001000,
            high: 0x3fff,
            valid: 1,
            proofs: 11,
            control: 0x027f,
        },
    ];
    let mut context = Context32::new(State32 {
        registers: [
            u32::MAX,
            0x12345678,
            0x87654321,
            0x31415926,
            0x2080,
            0x2070,
            0x44556677,
            0x8899aabb,
        ],
        eip: 0x1234,
        eflags: 0xc97,
    });
    for offset in (40..128).step_by(4) {
        context.set_word(offset, 0x10203000 + offset as u32);
    }
    for offset in [48, 52, 116] {
        context.set_word(offset, 0);
    }
    (instance, bank, context)
}

fn page(instance: &EngineInstance) -> [u8; 4096] {
    let mut bytes = [0; 4096];
    instance
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x2000), &mut bytes)
        .unwrap();
    bytes
}

#[test]
fn pure_cpuid_preserves_an_active_pool_with_pending_status_and_empty_ghosts() {
    let (mut instance, mut bank, mut context) = fixture();
    let pool = pool_image(&bank.pool);
    let architectural = bank.architectural_state();
    let arena = instance.arena().to_vec();
    let revision = instance.memory.as_ref().unwrap().transaction_version();
    let mut expected = *context.bytes();
    for offset in [0, 4, 8, 12] {
        put(&mut expected, offset, 0);
    }
    assert_eq!(
        bank.execute(&mut instance, &mut context, &native(Operation::CpuId)),
        0
    );
    assert_eq!(context.bytes(), &expected);
    assert_eq!(bank.architectural_state(), architectural);
    assert_eq!(pool_image(&bank.pool), pool);
    assert_eq!(instance.arena(), arena);
    assert_eq!(page(&instance), [0xa5; 4096]);
    assert_eq!(
        instance.memory.as_ref().unwrap().transaction_version(),
        revision
    );
}

#[test]
fn pure_processor_clock_success_and_infrastructure_fault_preserve_the_active_pool() {
    let (mut instance, mut bank, mut context) = fixture();
    let pool = pool_image(&bank.pool);
    let architectural = bank.architectural_state();
    let initial = *context.bytes();
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &native(Operation::VirtualTimestamp)
        ),
        StopReason::Infrastructure as u32
    );
    assert_eq!(context.bytes(), &initial);
    assert_eq!(pool_image(&bank.pool), pool);
    instance.configure_cpu_clock(1_000_000_000, 100).unwrap();
    instance.observe_cpu_clock(117).unwrap();
    let mut expected = initial;
    put(&mut expected, 0, 17);
    put(&mut expected, 4, 0);
    put(&mut expected, 8, 0);
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &native(Operation::VirtualTimestampAux)
        ),
        0
    );
    assert_eq!(context.bytes(), &expected);
    assert_eq!(pool_image(&bank.pool), pool);
    put(&mut expected, 28, 0);
    assert_eq!(
        bank.execute(
            &mut instance,
            &mut context,
            &native(Operation::ReadProcessorId {
                destination: Register32::Edi
            })
        ),
        0
    );
    assert_eq!(context.bytes(), &expected);
    assert_eq!(bank.architectural_state(), architectural);
    assert_eq!(page(&instance), [0xa5; 4096]);
}

#[test]
fn pure_stack_callback_commits_only_its_complete_ram_and_context_image() {
    let (mut instance, mut bank, mut context) = fixture();
    let pool = pool_image(&bank.pool);
    let architectural = bank.architectural_state();
    let revision = instance.memory.as_ref().unwrap().transaction_version();
    let mut expected_context = *context.bytes();
    put(&mut expected_context, 16, 0x2060);
    let mut expected_page = [0xa5; 4096];
    for (index, value) in [
        u32::MAX,
        0x12345678,
        0x87654321,
        0x31415926,
        0x2080,
        0x2070,
        0x44556677,
        0x8899aabb,
    ]
    .into_iter()
    .enumerate()
    {
        let offset = 0x80 - (index + 1) * 4;
        expected_page[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
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
        0
    );
    assert_eq!(context.bytes(), &expected_context);
    assert_eq!(page(&instance), expected_page);
    assert_eq!(
        instance.memory.as_ref().unwrap().transaction_version(),
        revision + 1
    );
    assert_eq!(bank.architectural_state(), architectural);
    assert_eq!(pool_image(&bank.pool), pool);
}

#[test]
fn pure_stack_frame_fault_preserves_pool_and_entire_preinstruction_state() {
    let (mut instance, mut bank, mut context) = fixture();
    context.set_word(16, 0x2004);
    let pool = pool_image(&bank.pool);
    let architectural = bank.architectural_state();
    let arena = instance.arena().to_vec();
    let revision = instance.memory.as_ref().unwrap().transaction_version();
    let mut expected = *context.bytes();
    put(&mut expected, 48, 0x1ff8);
    put(&mut expected, 52, 0x10204);
    put(&mut expected, 116, 0x40e);
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
        StopReason::MemoryFault as u32
    );
    assert_eq!(context.bytes(), &expected);
    assert_eq!(page(&instance), [0xa5; 4096]);
    assert_eq!(
        instance.memory.as_ref().unwrap().transaction_version(),
        revision
    );
    assert_eq!(instance.arena(), arena);
    assert_eq!(bank.architectural_state(), architectural);
    assert_eq!(pool_image(&bank.pool), pool);
}

#[test]
fn canonical_status_and_control_callbacks_revoke_old_pool_certificates() {
    for status in [true, false] {
        let (mut instance, mut bank, mut context) = fixture();
        // The Wasm caller materializes before a true x87 observer. Model that
        // ownership contract: the canonical image is current, pool inactive.
        bank.pool.active = 0;
        bank.pool.pending = 0;
        bank.pool.changed = 0;
        bank.x87.status = 0x3820;
        bank.x87.control = 0x027f;
        let mut expected_x87 = bank.x87;
        let mut expected_context = *context.bytes();
        let operation = if status {
            put(&mut expected_context, 0, 0xffff3820);
            Operation::X87StatusToAx
        } else {
            instance
                .memory
                .as_mut()
                .unwrap()
                .write(GuestAddress(0x2000), &0x007f_u16.to_le_bytes())
                .unwrap();
            expected_x87.control = 0x007f;
            Operation::X87 {
                operation: X87Operation::LoadControl,
                address: Some(EffectiveAddress {
                    displacement: 0x2000,
                    ..EffectiveAddress::default()
                }),
                waiting: false,
                opcode: 0x12d,
            }
        };
        let revision = instance.memory.as_ref().unwrap().transaction_version();
        let before_page = page(&instance);
        let vectors = bank.vectors;
        assert_eq!(
            bank.execute(&mut instance, &mut context, &native(operation)),
            0
        );
        assert_eq!(context.bytes(), &expected_context);
        assert_eq!(bank.x87, expected_x87);
        assert_eq!(bank.vectors, vectors);
        assert_eq!(pool_image(&bank.pool), vec![0; 384]);
        assert_eq!(page(&instance), before_page);
        assert_eq!(
            instance.memory.as_ref().unwrap().transaction_version(),
            revision
        );
    }
}
