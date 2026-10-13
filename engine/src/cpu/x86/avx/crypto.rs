use super::{ArithmeticError, MemoryRequirement, MoveSource, VectorRegister, VectorState};
use crate::{
    cpu::x86::simd::{CryptoKind, crypto_value},
    memory::Access,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CryptoInstruction {
    kind: CryptoKind,
    destination: VectorRegister,
    first: Option<VectorRegister>,
    second: MoveSource,
    immediate: Option<u8>,
}
impl CryptoInstruction {
    pub(crate) fn new(
        kind: CryptoKind,
        destination: VectorRegister,
        first: Option<VectorRegister>,
        second: MoveSource,
        immediate: Option<u8>,
    ) -> Option<Self> {
        let binary = matches!(
            kind,
            CryptoKind::EncryptRound
                | CryptoKind::EncryptLast
                | CryptoKind::DecryptRound
                | CryptoKind::DecryptLast
                | CryptoKind::CarrylessMultiply
        );
        let controlled = matches!(kind, CryptoKind::KeyAssist | CryptoKind::CarrylessMultiply);
        (first.is_some() == binary && immediate.is_some() == controlled).then_some(Self {
            kind,
            destination,
            first,
            second,
            immediate,
        })
    }
    pub(crate) fn memory(self) -> Option<MemoryRequirement> {
        (self.second == MoveSource::Memory).then_some(MemoryRequirement {
            access: Access::Read,
            bytes: 16,
            alignment: 1,
        })
    }
}
impl VectorState {
    pub(crate) fn execute_crypto(
        &mut self,
        instruction: CryptoInstruction,
        operand: &[u8],
    ) -> Result<(), ArithmeticError> {
        if operand.len() != instruction.memory().map_or(0, |r| usize::from(r.bytes)) {
            return Err(ArithmeticError::OperandLength);
        }
        if self.simd.mxcsr & !0xffff != 0 {
            return Err(ArithmeticError::InvalidState);
        }
        let first = instruction
            .first
            .map_or([0; 16], |r| self.simd.xmm[r.index()]);
        let second = match instruction.second {
            MoveSource::Register(r) => self.simd.xmm[r.index()],
            MoveSource::Memory => operand.try_into().unwrap(),
        };
        let mut value = [0; 32];
        value[..16].copy_from_slice(&crypto_value(
            instruction.kind,
            first,
            second,
            instruction.immediate,
        ));
        self.publish(instruction.destination, value);
        Ok(())
    }
}
#[cfg(test)]
#[path = "crypto/native.rs"]
mod tests;
