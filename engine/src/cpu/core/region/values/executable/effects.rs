use crate::cpu::{
    core::{effects, flags},
    x86::{Register32, X87Condition, X87Operation, ir::*},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum FpEffectClass {
    Preserve,
    Observe,
    Barrier,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum MemoryEffect {
    None,
    Read,
    Write,
    ReadWrite,
    Barrier,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum FaultRecovery {
    Before,
    /// the exact kernel owns partial progress of its declared definitions.
    Kernel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct ExactDefUse {
    pub gpr_reads: u8,
    pub gpr_writes: u8,
    pub flags_read: u32,
    pub flags_write: u32,
    pub flags_undefined: u32,
    pub fp: FpEffectClass,
    pub memory: MemoryEffect,
    pub can_fault: bool,
    pub can_stop_after: bool,
    pub fault_recovery: FaultRecovery,
}

impl ExactDefUse {
    fn known(reads: u8, writes: u8) -> Self {
        Self {
            gpr_reads: reads,
            gpr_writes: writes,
            flags_read: 0,
            flags_write: 0,
            flags_undefined: 0,
            fp: FpEffectClass::Preserve,
            memory: MemoryEffect::None,
            can_fault: false,
            can_stop_after: false,
            fault_recovery: FaultRecovery::Before,
        }
    }

    fn opaque() -> Self {
        Self {
            gpr_reads: u8::MAX,
            gpr_writes: u8::MAX,
            flags_read: u32::MAX,
            flags_write: u32::MAX,
            flags_undefined: 0,
            fp: FpEffectClass::Barrier,
            memory: MemoryEffect::Barrier,
            can_fault: true,
            can_stop_after: true,
            fault_recovery: FaultRecovery::Kernel,
        }
    }
}

fn reg(register: Register32) -> u8 {
    1 << register.index()
}

fn byte(register: ByteRegister) -> u8 {
    1 << match register {
        ByteRegister::Al | ByteRegister::Ah => 0,
        ByteRegister::Cl | ByteRegister::Ch => 1,
        ByteRegister::Dl | ByteRegister::Dh => 2,
        ByteRegister::Bl | ByteRegister::Bh => 3,
    }
}

fn address(address: EffectiveAddress) -> u8 {
    address.base.map_or(0, reg) | address.index.map_or(0, reg)
}

fn location(location: Location32) -> u8 {
    match location {
        Location32::Register(register) => reg(register),
        Location32::Memory(ea) => address(ea),
    }
}

fn written(location: Location32) -> u8 {
    match location {
        Location32::Register(register) => reg(register),
        Location32::Memory(_) => 0,
    }
}

fn value(value: Value32) -> u8 {
    match value {
        Value32::Register(register) => reg(register),
        Value32::Memory(ea) => address(ea),
        Value32::Immediate(_) => 0,
    }
}

fn word(value: WordValue) -> u8 {
    match value {
        WordValue::Register(register) => reg(register),
        WordValue::Immediate(_) => 0,
    }
}

fn byte_value(value: ByteValue) -> u8 {
    match value {
        ByteValue::Register(register) => byte(register),
        ByteValue::Immediate(_) => 0,
    }
}

fn branch(target: BranchTarget) -> u8 {
    match target {
        BranchTarget::Direct(_) => 0,
        BranchTarget::Indirect(source) => location(source),
    }
}

fn opaque_for(operation: Operation) -> ExactDefUse {
    let mut result = ExactDefUse::opaque();
    // Unknown GPR/FLAGS definitions do not make an integer/control/string
    // kernel an x87 writer. Only the numerical namespace can change that
    // architectural bank; scalar fault/partial-progress contracts stay opaque.
    if !crate::cpu::core::emitter::is_native(&operation)
        || effects::numeric_preserves_fp(operation)
        || crate::cpu::core::emitter::preserves_fp_success(&operation)
    {
        result.fp = FpEffectClass::Preserve;
    }
    result
}

pub(super) fn classify(operation: Option<Operation>, gate: Option<u32>) -> ExactDefUse {
    let Some(operation) = operation.filter(|_| gate.is_none()) else {
        return ExactDefUse::opaque();
    };
    let mut result = match operation {
        Operation::Nop => ExactDefUse::known(0, 0),
        Operation::Move {
            destination,
            source,
        } => ExactDefUse::known(
            value(source)
                | if matches!(destination, Location32::Memory(_)) {
                    location(destination)
                } else {
                    0
                },
            written(destination),
        ),
        Operation::MoveByte {
            destination,
            source,
        } => ExactDefUse::known(byte(destination) | byte_value(source), byte(destination)),
        Operation::MoveWord {
            destination,
            source,
        } => ExactDefUse::known(reg(destination) | word(source), reg(destination)),
        Operation::LoadByte {
            destination,
            address: ea,
        } => ExactDefUse::known(byte(destination) | address(ea), byte(destination)),
        Operation::LoadWord {
            destination,
            address: ea,
        } => ExactDefUse::known(reg(destination) | address(ea), reg(destination)),
        Operation::StoreByte {
            address: ea,
            source,
        } => ExactDefUse::known(address(ea) | byte_value(source), 0),
        Operation::StoreWord {
            address: ea,
            source,
        } => ExactDefUse::known(address(ea) | word(source), 0),
        Operation::Lea {
            destination,
            address: ea,
        }
        | Operation::LeaWord {
            destination,
            address: ea,
        } => ExactDefUse::known(
            address(ea)
                | if matches!(operation, Operation::LeaWord { .. }) {
                    reg(destination)
                } else {
                    0
                },
            reg(destination),
        ),
        Operation::Binary {
            kind,
            destination,
            source,
        } => ExactDefUse::known(
            location(destination) | value(source),
            if matches!(kind, BinaryKind::Cmp | BinaryKind::Test) {
                0
            } else {
                written(destination)
            },
        ),
        Operation::Unary { destination, .. } => {
            ExactDefUse::known(location(destination), written(destination))
        }
        Operation::UnaryByte { destination, .. } => {
            ExactDefUse::known(byte(destination), byte(destination))
        }
        Operation::UnaryWord { destination, .. } => {
            ExactDefUse::known(reg(destination), reg(destination))
        }
        Operation::ArithmeticByte {
            destination,
            source,
            ..
        }
        | Operation::LogicalByte {
            destination,
            source,
            ..
        } => ExactDefUse::known(byte(destination) | byte_value(source), byte(destination)),
        Operation::ArithmeticWord {
            destination,
            source,
            ..
        }
        | Operation::LogicalWord {
            destination,
            source,
            ..
        } => ExactDefUse::known(reg(destination) | word(source), reg(destination)),
        Operation::CompareByte { left, right } | Operation::TestByte { left, right } => {
            ExactDefUse::known(byte(left) | byte_value(right), 0)
        }
        Operation::CompareWord { left, right } | Operation::TestWord { left, right } => {
            ExactDefUse::known(reg(left) | word(right), 0)
        }
        Operation::SetByte { destination, .. } => {
            ExactDefUse::known(byte(destination), byte(destination))
        }
        Operation::MemorySetByte { address: ea, .. } => ExactDefUse::known(address(ea), 0),
        Operation::ConditionalMove {
            destination,
            source,
            ..
        }
        | Operation::ConditionalMoveWord {
            destination,
            source,
            ..
        } => ExactDefUse::known(reg(destination) | reg(source), reg(destination)),
        Operation::ReadConditionalMove {
            destination,
            address: ea,
            ..
        }
        | Operation::ReadConditionalMoveWord {
            destination,
            address: ea,
            ..
        } => ExactDefUse::known(reg(destination) | address(ea), reg(destination)),
        Operation::Exchange { left, right } | Operation::ExchangeWord { left, right } => {
            ExactDefUse::known(reg(left) | reg(right), reg(left) | reg(right))
        }
        Operation::ExchangeByte { left, right } => {
            ExactDefUse::known(byte(left) | byte(right), byte(left) | byte(right))
        }
        Operation::MemoryExchange {
            address: ea,
            source,
        }
        | Operation::MemoryExchangeWord {
            address: ea,
            source,
        } => ExactDefUse::known(address(ea) | reg(source), reg(source)),
        Operation::MemoryExchangeByte {
            address: ea,
            source,
        } => ExactDefUse::known(address(ea) | byte(source), byte(source)),
        Operation::Carry { .. } | Operation::Direction { .. } => ExactDefUse::known(0, 0),
        Operation::FlagsToAh | Operation::SetAlFromCarry => {
            ExactDefUse::known(reg(Register32::Eax), reg(Register32::Eax))
        }
        Operation::AhToFlags => ExactDefUse::known(reg(Register32::Eax), 0),
        Operation::Jump { target } => ExactDefUse::known(branch(target), 0),
        Operation::JumpWord { source } => ExactDefUse::known(reg(source), 0),
        Operation::ReadJumpWord { address: ea } => ExactDefUse::known(address(ea), 0),
        Operation::ConditionalJump { .. } => ExactDefUse::known(0, 0),
        Operation::CountBranch { kind, .. } | Operation::CountBranchWord { kind, .. } => {
            ExactDefUse::known(
                reg(Register32::Ecx),
                if kind == CountBranchKind::EcxZero {
                    0
                } else {
                    reg(Register32::Ecx)
                },
            )
        }
        Operation::Call { target } => {
            ExactDefUse::known(reg(Register32::Esp) | branch(target), reg(Register32::Esp))
        }
        Operation::CallWord { .. } => {
            ExactDefUse::known(reg(Register32::Esp), reg(Register32::Esp))
        }
        Operation::CallRegisterWord { source } => {
            ExactDefUse::known(reg(Register32::Esp) | reg(source), reg(Register32::Esp))
        }
        Operation::ReadCallWord { address: ea } => {
            ExactDefUse::known(reg(Register32::Esp) | address(ea), reg(Register32::Esp))
        }
        Operation::Return { .. } | Operation::ReturnWord { .. } => {
            ExactDefUse::known(reg(Register32::Esp), reg(Register32::Esp))
        }
        Operation::Push { source } => {
            ExactDefUse::known(reg(Register32::Esp) | value(source), reg(Register32::Esp))
        }
        Operation::PushWord { source } => {
            ExactDefUse::known(reg(Register32::Esp) | word(source), reg(Register32::Esp))
        }
        Operation::PushMemoryWord { source } => {
            ExactDefUse::known(reg(Register32::Esp) | address(source), reg(Register32::Esp))
        }
        Operation::PushFlags | Operation::PushFlagsWord => {
            ExactDefUse::known(reg(Register32::Esp), reg(Register32::Esp))
        }
        Operation::Pop { destination } => ExactDefUse::known(
            reg(Register32::Esp)
                | if matches!(destination, Location32::Memory(_)) {
                    location(destination)
                } else {
                    0
                },
            reg(Register32::Esp) | written(destination),
        ),
        Operation::PopWord { destination } => ExactDefUse::known(
            reg(Register32::Esp) | reg(destination),
            reg(Register32::Esp) | reg(destination),
        ),
        Operation::PopMemoryWord { destination } => ExactDefUse::known(
            reg(Register32::Esp) | address(destination),
            reg(Register32::Esp),
        ),
        Operation::EnterZero
        | Operation::EnterZeroWord
        | Operation::Leave
        | Operation::LeaveWord
        | Operation::StackFrame { .. } => ExactDefUse::known(
            reg(Register32::Esp) | reg(Register32::Ebp),
            reg(Register32::Esp) | reg(Register32::Ebp),
        ),
        Operation::CpuId => ExactDefUse::known(reg(Register32::Eax) | reg(Register32::Ecx), 0x0f),
        Operation::VirtualTimestamp => {
            ExactDefUse::known(0, reg(Register32::Eax) | reg(Register32::Edx))
        }
        Operation::VirtualTimestampAux => ExactDefUse::known(
            0,
            reg(Register32::Eax) | reg(Register32::Ecx) | reg(Register32::Edx),
        ),
        Operation::ReadProcessorId { destination } => ExactDefUse::known(0, reg(destination)),
        Operation::X87 {
            operation: fp,
            address: ea,
            ..
        } => {
            let mut contract = ExactDefUse::known(
                ea.map_or(0, address),
                if matches!(fp, X87Operation::StatusToAx) {
                    reg(Register32::Eax)
                } else {
                    0
                },
            );
            if matches!(fp, X87Operation::StatusToAx) {
                contract.gpr_reads |= reg(Register32::Eax);
            }
            contract.fp = if matches!(
                operation,
                Operation::X87 {
                    operation: X87Operation::Wait,
                    address: None,
                    waiting: true,
                    ..
                }
            ) {
                FpEffectClass::Observe
            } else {
                FpEffectClass::Barrier
            };
            contract.can_fault = true;
            contract
        }
        Operation::X87StatusToAx => {
            let mut contract = ExactDefUse::known(reg(Register32::Eax), reg(Register32::Eax));
            contract.fp = FpEffectClass::Observe;
            contract
        }
        Operation::X87StatusToMemory { address: ea }
        | Operation::X87ControlToMemory { address: ea } => {
            let mut contract = ExactDefUse::known(address(ea), 0);
            contract.fp = FpEffectClass::Observe;
            contract
        }
        Operation::InitializeX87 | Operation::ClearX87Exceptions => {
            let mut contract = ExactDefUse::known(0, 0);
            contract.fp = FpEffectClass::Barrier;
            contract
        }
        _ => return opaque_for(operation),
    };
    if let Some(effect) = effects::analyze(operation) {
        result.flags_read = effect.read;
        result.flags_write = effect.write;
        result.flags_undefined = effect.undefined;
    } else if !matches!(
        operation,
        Operation::X87 { .. }
            | Operation::X87StatusToAx
            | Operation::X87StatusToMemory { .. }
            | Operation::X87ControlToMemory { .. }
            | Operation::InitializeX87
            | Operation::ClearX87Exceptions
            | Operation::CpuId
            | Operation::VirtualTimestamp
            | Operation::VirtualTimestampAux
            | Operation::ReadProcessorId { .. }
            | Operation::StackFrame { .. }
    ) {
        // an audited register contract does not imply an audited flags contract.
        result.flags_read = u32::MAX;
        result.flags_write = u32::MAX;
    }
    if matches!(operation, Operation::Direction { .. }) {
        result.flags_write |= 0x400;
    }
    match operation {
        Operation::X87 {
            operation:
                X87Operation::Compare {
                    integer_flags: true,
                    ..
                },
            ..
        } => {
            result.flags_write = flags::ARITHMETIC;
        }
        Operation::X87 {
            operation: X87Operation::ConditionalMove { condition, .. },
            ..
        } => {
            result.flags_read = match condition {
                X87Condition::Below | X87Condition::NotBelow => flags::CF,
                X87Condition::Equal | X87Condition::NotEqual => flags::ZF,
                X87Condition::BelowOrEqual | X87Condition::Above => flags::CF | flags::ZF,
                X87Condition::Unordered | X87Condition::Ordered => flags::PF,
            };
        }
        _ => {}
    }
    let (faults, writes) = effects::memory_barrier(operation);
    result.can_fault |= faults;
    if faults || writes {
        result.memory = if writes {
            MemoryEffect::ReadWrite
        } else {
            MemoryEffect::Read
        };
    }
    result.can_stop_after |= writes;
    result
}
