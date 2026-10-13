use super::*;
use crate::cpu::x86::{
    Register32,
    packed_strings::{PackedStringLength, PackedStringOutput},
    simd::{
        CryptoKind, IntegerLaneOperation, MultiplyOperation, PackedKind, PairwiseKind,
        RearrangeOperation, ScalarWidth, ShiftOperation, ShuffleKind,
    },
};
mod compare;
mod compound;
mod convert;
mod cross_lane;
mod crypto;
mod f16c;
mod floating;
mod fma;
mod integer;
mod logical;
mod masked;
mod moves;
mod multiply;
mod permute;
mod predicates;
mod rearrange;
mod scalar;
mod shift;
mod strings;

const D: u32 = 7;
const A: u32 = 7 << 3;
const B: u32 = 7 << 6;
const M: u32 = 7 << 9;
#[derive(Clone, Copy, PartialEq, Eq)]
enum MemoryPolicy {
    Forbidden,
    Optional,
    Required,
}
#[derive(Clone, Copy)]
struct Slots(u32);
impl Slots {
    fn source_memory(self) -> bool {
        self.0 & (1 << 12) != 0
    }
    fn target_memory(self) -> bool {
        self.0 & (1 << 13) != 0
    }
    fn index(self, slot: u32) -> u8 {
        ((self.0 >> (slot * 3)) & 7) as u8
    }
    fn vector(self, slot: u32) -> VectorRegister {
        VectorRegister(self.index(slot))
    }
    fn general(self, slot: u32) -> Register32 {
        [
            Register32::Eax,
            Register32::Ecx,
            Register32::Edx,
            Register32::Ebx,
            Register32::Esp,
            Register32::Ebp,
            Register32::Esi,
            Register32::Edi,
        ][usize::from(self.index(slot))]
    }
    fn source(self) -> MoveSource {
        if self.source_memory() {
            MoveSource::Memory
        } else {
            MoveSource::Register(self.vector(2))
        }
    }
    fn word_source(self) -> WordSource {
        if self.source_memory() {
            WordSource::Memory
        } else {
            WordSource::Register(self.general(2))
        }
    }
    fn word_target(self) -> WordTarget {
        if self.target_memory() {
            WordTarget::Memory
        } else {
            WordTarget::Register(self.general(0))
        }
    }
    fn block_target(self) -> BlockTarget {
        if self.target_memory() {
            BlockTarget::Memory
        } else {
            BlockTarget::Register(self.vector(0))
        }
    }
    fn validate(
        self,
        mut used: u32,
        source: MemoryPolicy,
        target: MemoryPolicy,
        controlled: bool,
        immediate: u8,
    ) -> Result<(), AvxDecodeError> {
        for (policy, present) in [
            (source, self.source_memory()),
            (target, self.target_memory()),
        ] {
            if policy == MemoryPolicy::Forbidden && present
                || policy == MemoryPolicy::Required && !present
            {
                return Err(AvxDecodeError::Operands);
            }
        }
        if self.source_memory() {
            used &= !(7 << 6);
        }
        if self.target_memory() {
            used &= !7;
        }
        if self.0 & 0xfff & !used != 0 {
            return Err(AvxDecodeError::Operands);
        }
        if !controlled && immediate != 0 {
            return Err(AvxDecodeError::Immediate);
        }
        Ok(())
    }
}
pub(super) fn decode(
    kind: u16,
    operands: u32,
    immediate: u8,
) -> Result<AvxSemantic, AvxDecodeError> {
    let slots = Slots(operands);
    match kind >> 8 {
        1 => floating::decode(kind as u8, slots, immediate),
        2 => logical::decode(kind as u8, slots, immediate),
        3 => moves::decode(kind as u8, slots, immediate),
        4 => permute::decode(kind as u8, slots, immediate),
        5 => integer::decode(kind as u8, slots, immediate),
        6 => rearrange::decode(kind as u8, slots, immediate),
        7 => shift::decode(kind as u8, slots, immediate),
        8 => multiply::decode(kind as u8, slots, immediate),
        9 => scalar::decode(kind as u8, slots, immediate),
        10 => compare::decode(kind as u8, slots, immediate),
        11 => convert::decode(kind as u8, slots, immediate),
        12 => compound::decode(kind as u8, slots, immediate),
        13 => masked::decode(kind as u8, slots, immediate),
        14 => cross_lane::decode(kind as u8, slots, immediate),
        15 => predicates::decode(kind as u8, slots, immediate),
        16 => crypto::decode(kind as u8, slots, immediate),
        17 => strings::decode(kind as u8, slots, immediate),
        18 => f16c::decode(kind as u8, slots, immediate),
        19 => fma::decode(kind as u8, slots, immediate),
        _ => Err(AvxDecodeError::Family),
    }
}
