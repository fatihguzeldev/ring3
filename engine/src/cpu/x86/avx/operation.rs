use super::*;
use crate::cpu::x86::{Register32, ir::EffectiveAddress};

mod factory;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AvxSemantic {
    Floating(FloatInstruction),
    Logical(LogicalInstruction),
    Move(MoveInstruction),
    Permute(PermuteInstruction),
    Integer(IntegerInstruction),
    Rearrange(RearrangeInstruction),
    Shift(ShiftInstruction),
    Multiply(MultiplyInstruction),
    Scalar(ScalarInstruction),
    Compare(CompareInstruction),
    Convert(ConvertInstruction),
    Compound(CompoundInstruction),
    Masked(MaskedInstruction),
    CrossLane(CrossLaneInstruction),
    Test(TestInstruction),
    Crypto(CryptoInstruction),
    String(StringInstruction),
    F16c(F16cInstruction),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MemoryEffect {
    None,
    Full(MemoryRequirement),
    Selected(MaskedInstruction),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AvxDecodeError {
    Version,
    ReservedBits,
    Family,
    Operands,
    Immediate,
    Address,
}

// only the canonical descriptor factory can construct an instruction. the typed
// semantic and encoded transport therefore cannot disagree after validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AvxInstruction {
    semantic: AvxSemantic,
    descriptor: u32,
    operands: u32,
    immediate: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AvxOperation {
    instruction: AvxInstruction,
    address: Option<EffectiveAddress>,
}

impl AvxOperation {
    pub(crate) fn new(
        instruction: AvxInstruction,
        address: Option<EffectiveAddress>,
    ) -> Result<Self, AvxDecodeError> {
        if (instruction.memory() != MemoryEffect::None) != address.is_some() {
            return Err(AvxDecodeError::Address);
        }
        Ok(Self {
            instruction,
            address,
        })
    }
    pub(crate) fn instruction(self) -> AvxInstruction {
        self.instruction
    }
    pub(crate) fn address(self) -> Option<EffectiveAddress> {
        self.address
    }
}

impl AvxInstruction {
    pub(crate) fn decode_descriptor(
        descriptor: u32,
        operands: u32,
        immediate: u32,
    ) -> Result<Self, AvxDecodeError> {
        if descriptor >> 24 != 1 {
            return Err(AvxDecodeError::Version);
        }
        if descriptor & 0x00ff_0000 != 0 || operands & !0x3fff != 0 {
            return Err(AvxDecodeError::ReservedBits);
        }
        if immediate > 255 {
            return Err(AvxDecodeError::Immediate);
        }
        let semantic = factory::decode(descriptor as u16, operands, immediate as u8)?;
        Ok(Self {
            semantic,
            descriptor,
            operands,
            immediate: immediate as u8,
        })
    }
    pub(crate) fn encode(self) -> (u32, u32, u32) {
        (self.descriptor, self.operands, u32::from(self.immediate))
    }
    pub(crate) fn semantic(self) -> AvxSemantic {
        self.semantic
    }
    pub(crate) fn memory(self) -> MemoryEffect {
        use AvxSemantic::*;
        let requirement = match self.semantic {
            Floating(value) => value.memory(),
            Logical(value) => value.memory(),
            Move(value) => value.memory(),
            Permute(value) => value.memory(),
            Integer(value) => value.memory(),
            Rearrange(value) => value.memory(),
            Shift(value) => value.memory(),
            Multiply(value) => value.memory(),
            Scalar(value) => value.memory(),
            Compare(value) => value.memory(),
            Convert(value) => value.memory(),
            Compound(value) => value.memory(),
            CrossLane(value) => value.memory(),
            Test(value) => value.memory(),
            Crypto(value) => value.memory(),
            String(value) => value.memory(),
            F16c(value) => value.memory(),
            Masked(value) => return MemoryEffect::Selected(value),
        };
        requirement.map_or(MemoryEffect::None, MemoryEffect::Full)
    }
    pub(crate) fn inputs(self) -> [Option<Register32>; 2] {
        use AvxSemantic::*;
        let input = match self.semantic {
            Move(MoveInstruction::DwordLoad {
                source: WordSource::Register(register),
                ..
            })
            | Scalar(ScalarInstruction::Insert {
                source: WordSource::Register(register),
                ..
            })
            | Convert(ConvertInstruction::FromInteger {
                source: WordSource::Register(register),
                ..
            }) => Some(register),
            String(value)
                if value.length()
                    == crate::cpu::x86::packed_strings::PackedStringLength::Explicit =>
            {
                return [Some(Register32::Eax), Some(Register32::Edx)];
            }
            _ => None,
        };
        [input, None]
    }
    pub(crate) fn output(self) -> Option<Register32> {
        use AvxSemantic::*;
        match self.semantic {
            Move(MoveInstruction::DwordStore {
                destination: WordTarget::Register(register),
                ..
            })
            | Move(MoveInstruction::Mask {
                destination: register,
                ..
            })
            | Permute(PermuteInstruction::ExtractSingle {
                destination: WordTarget::Register(register),
                ..
            })
            | Scalar(ScalarInstruction::Extract {
                destination: WordTarget::Register(register),
                ..
            })
            | Scalar(ScalarInstruction::Mask {
                destination: register,
                ..
            })
            | Convert(ConvertInstruction::ToInteger {
                destination: register,
                ..
            }) => Some(register),
            String(value)
                if value.output() == crate::cpu::x86::packed_strings::PackedStringOutput::Index =>
            {
                Some(Register32::Ecx)
            }
            _ => None,
        }
    }
    pub(crate) fn writes_flags(self) -> bool {
        matches!(
            self.semantic,
            AvxSemantic::Compare(CompareInstruction::Flags { .. })
                | AvxSemantic::Test(_)
                | AvxSemantic::String(_)
        )
    }
}
