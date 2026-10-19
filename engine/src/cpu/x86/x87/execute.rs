use core::cmp::Ordering;

use super::X87State;
use super::value::{DENORMAL, Ext80, INVALID, Kind, Rounded, X87Rounding};

#[cfg(test)]
#[path = "execute/tests/division.rs"]
mod division_tests;

#[cfg(test)]
#[path = "execute/tests/output.rs"]
mod output_tests;

#[cfg(test)]
#[path = "execute/tests/transaction.rs"]
mod transaction_tests;

const C0: u16 = 1 << 8;
const C1: u16 = 1 << 9;
const C2: u16 = 1 << 10;
const C3: u16 = 1 << 14;
const CONDITIONS: u16 = C0 | C1 | C2 | C3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum X87Format {
    Float32,
    Float64,
    Extended80,
    Integer16,
    Integer32,
    Integer64,
    PackedBcd,
}

impl X87Format {
    pub const fn size(self) -> usize {
        match self {
            Self::Integer16 => 2,
            Self::Float32 | Self::Integer32 => 4,
            Self::Float64 | Self::Integer64 => 8,
            Self::Extended80 | Self::PackedBcd => 10,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum X87Binary {
    Add,
    Multiply,
    Subtract,
    ReverseSubtract,
    Divide,
    ReverseDivide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum X87Condition {
    Below,
    Equal,
    BelowOrEqual,
    Unordered,
    NotBelow,
    NotEqual,
    Above,
    Ordered,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum X87Operation {
    Initialize,
    ClearExceptions,
    Wait,
    Load(X87Format),
    Store {
        format: X87Format,
        pop: bool,
        truncate: bool,
    },
    LoadRegister {
        index: u8,
    },
    StoreRegister {
        index: u8,
        pop: bool,
    },
    StoreRegisterNoUnderflow {
        index: u8,
    },
    Exchange {
        index: u8,
    },
    Binary {
        kind: X87Binary,
        destination: u8,
        source: u8,
        pop: bool,
    },
    BinaryMemory {
        kind: X87Binary,
        format: X87Format,
    },
    Compare {
        index: u8,
        quiet: bool,
        pop: u8,
        integer_flags: bool,
    },
    CompareMemory {
        format: X87Format,
        pop: bool,
    },
    ConditionalMove {
        index: u8,
        condition: X87Condition,
    },
    Test,
    Examine,
    ChangeSign,
    Absolute,
    SquareRoot,
    RoundToInteger,
    Scale,
    Extract,
    Remainder {
        nearest: bool,
    },
    LoadConstant {
        index: u8,
    },
    Free {
        index: u8,
        pop: bool,
    },
    AdjustTop {
        increment: bool,
    },
    LoadControl,
    StoreControl,
    StoreStatus,
    StatusToAx,
    LoadEnvironment {
        operand16: bool,
    },
    StoreEnvironment {
        operand16: bool,
    },
    Restore {
        operand16: bool,
    },
    Save {
        operand16: bool,
    },
    NoOperation,
    Exp2MinusOne,
    Log2Multiply {
        plus_one: bool,
    },
    PartialAtan,
    Trigonometric {
        function: u8,
    },
}

impl X87Operation {
    pub fn encode(self) -> u32 {
        let (tag, a, b, c) = match self {
            Self::Initialize => (0, 0, 0, 0),
            Self::ClearExceptions => (1, 0, 0, 0),
            Self::Wait => (2, 0, 0, 0),
            Self::Load(f) => (3, f as u32, 0, 0),
            Self::Store {
                format,
                pop,
                truncate,
            } => (4, format as u32, u32::from(pop), u32::from(truncate)),
            Self::LoadRegister { index } => (5, u32::from(index), 0, 0),
            Self::StoreRegister { index, pop } => (6, u32::from(index), u32::from(pop), 0),
            Self::StoreRegisterNoUnderflow { index } => (38, u32::from(index), 0, 0),
            Self::Exchange { index } => (7, u32::from(index), 0, 0),
            Self::Binary {
                kind,
                destination,
                source,
                pop,
            } => (
                8,
                kind as u32,
                u32::from(destination) | (u32::from(source) << 4),
                u32::from(pop),
            ),
            Self::BinaryMemory { kind, format } => (9, kind as u32, format as u32, 0),
            Self::Compare {
                index,
                quiet,
                pop,
                integer_flags,
            } => (
                10,
                u32::from(index),
                u32::from(quiet) | (u32::from(integer_flags) << 1),
                u32::from(pop),
            ),
            Self::CompareMemory { format, pop } => (11, format as u32, u32::from(pop), 0),
            Self::ConditionalMove { index, condition } => {
                (12, u32::from(index), condition as u32, 0)
            }
            Self::Test => (13, 0, 0, 0),
            Self::Examine => (14, 0, 0, 0),
            Self::ChangeSign => (15, 0, 0, 0),
            Self::Absolute => (16, 0, 0, 0),
            Self::SquareRoot => (17, 0, 0, 0),
            Self::RoundToInteger => (18, 0, 0, 0),
            Self::Scale => (19, 0, 0, 0),
            Self::Extract => (20, 0, 0, 0),
            Self::Remainder { nearest } => (21, u32::from(nearest), 0, 0),
            Self::LoadConstant { index } => (22, u32::from(index), 0, 0),
            Self::Free { index, pop } => (23, u32::from(index), u32::from(pop), 0),
            Self::AdjustTop { increment } => (24, u32::from(increment), 0, 0),
            Self::LoadControl => (25, 0, 0, 0),
            Self::StoreControl => (26, 0, 0, 0),
            Self::StoreStatus => (27, 0, 0, 0),
            Self::LoadEnvironment { operand16 } => (28, u32::from(operand16), 0, 0),
            Self::StoreEnvironment { operand16 } => (29, u32::from(operand16), 0, 0),
            Self::Restore { operand16 } => (30, u32::from(operand16), 0, 0),
            Self::Save { operand16 } => (31, u32::from(operand16), 0, 0),
            Self::StatusToAx => (37, 0, 0, 0),
            Self::NoOperation => (32, 0, 0, 0),
            Self::Exp2MinusOne => (33, 0, 0, 0),
            Self::Log2Multiply { plus_one } => (34, u32::from(plus_one), 0, 0),
            Self::PartialAtan => (35, 0, 0, 0),
            Self::Trigonometric { function } => (36, u32::from(function), 0, 0),
        };
        tag | (a << 8) | (b << 16) | (c << 24)
    }

    pub fn decode(word: u32) -> Option<Self> {
        let tag = word & 255;
        let a = ((word >> 8) & 255) as u8;
        let b = ((word >> 16) & 255) as u8;
        let c = (word >> 24) as u8;
        let format = |f| match f {
            0 => Some(X87Format::Float32),
            1 => Some(X87Format::Float64),
            2 => Some(X87Format::Extended80),
            3 => Some(X87Format::Integer16),
            4 => Some(X87Format::Integer32),
            5 => Some(X87Format::Integer64),
            6 => Some(X87Format::PackedBcd),
            _ => None,
        };
        let binary = |k| match k {
            0 => Some(X87Binary::Add),
            1 => Some(X87Binary::Multiply),
            2 => Some(X87Binary::Subtract),
            3 => Some(X87Binary::ReverseSubtract),
            4 => Some(X87Binary::Divide),
            5 => Some(X87Binary::ReverseDivide),
            _ => None,
        };
        let condition = |k| match k {
            0 => Some(X87Condition::Below),
            1 => Some(X87Condition::Equal),
            2 => Some(X87Condition::BelowOrEqual),
            3 => Some(X87Condition::Unordered),
            4 => Some(X87Condition::NotBelow),
            5 => Some(X87Condition::NotEqual),
            6 => Some(X87Condition::Above),
            7 => Some(X87Condition::Ordered),
            _ => None,
        };
        let operation = match tag {
            0 => Self::Initialize,
            1 => Self::ClearExceptions,
            2 => Self::Wait,
            3 => Self::Load(format(a)?),
            4 => Self::Store {
                format: format(a)?,
                pop: b != 0,
                truncate: c != 0,
            },
            5 => Self::LoadRegister { index: a },
            6 => Self::StoreRegister {
                index: a,
                pop: b != 0,
            },
            7 => Self::Exchange { index: a },
            8 => Self::Binary {
                kind: binary(a)?,
                destination: b & 15,
                source: b >> 4,
                pop: c != 0,
            },
            9 => Self::BinaryMemory {
                kind: binary(a)?,
                format: format(b)?,
            },
            10 => Self::Compare {
                index: a,
                quiet: b & 1 != 0,
                pop: c,
                integer_flags: b & 2 != 0,
            },
            11 => Self::CompareMemory {
                format: format(a)?,
                pop: b != 0,
            },
            12 => Self::ConditionalMove {
                index: a,
                condition: condition(b)?,
            },
            13 => Self::Test,
            14 => Self::Examine,
            15 => Self::ChangeSign,
            16 => Self::Absolute,
            17 => Self::SquareRoot,
            18 => Self::RoundToInteger,
            19 => Self::Scale,
            20 => Self::Extract,
            21 => Self::Remainder { nearest: a != 0 },
            22 => Self::LoadConstant { index: a },
            23 => Self::Free {
                index: a,
                pop: b != 0,
            },
            24 => Self::AdjustTop { increment: a != 0 },
            25 => Self::LoadControl,
            26 => Self::StoreControl,
            27 => Self::StoreStatus,
            28 => Self::LoadEnvironment { operand16: a != 0 },
            29 => Self::StoreEnvironment { operand16: a != 0 },
            30 => Self::Restore { operand16: a != 0 },
            31 => Self::Save { operand16: a != 0 },
            32 => Self::NoOperation,
            33 => Self::Exp2MinusOne,
            34 => Self::Log2Multiply { plus_one: a != 0 },
            35 => Self::PartialAtan,
            36 => Self::Trigonometric { function: a },
            37 => Self::StatusToAx,
            38 => Self::StoreRegisterNoUnderflow { index: a },
            _ => return None,
        };
        if operation.encode() != word {
            return None;
        }
        match operation {
            Self::LoadRegister { index }
            | Self::StoreRegister { index, .. }
            | Self::StoreRegisterNoUnderflow { index }
            | Self::Exchange { index }
            | Self::Free { index, .. }
            | Self::ConditionalMove { index, .. }
            | Self::Compare { index, .. }
                if index >= 8 =>
            {
                None
            }
            Self::Binary {
                destination,
                source,
                ..
            } if destination >= 8 || source >= 8 => None,
            Self::Compare { pop, .. } if pop > 2 => None,
            Self::LoadConstant { index } if index > 6 => None,
            Self::Trigonometric { function } if function > 3 => None,
            _ => Some(operation),
        }
    }

    pub const fn read_size(self) -> usize {
        match self {
            Self::Load(format)
            | Self::BinaryMemory { format, .. }
            | Self::CompareMemory { format, .. } => format.size(),
            Self::LoadControl => 2,
            Self::LoadEnvironment { operand16 } => {
                if operand16 {
                    14
                } else {
                    28
                }
            }
            Self::Restore { operand16 } => {
                if operand16 {
                    94
                } else {
                    108
                }
            }
            _ => 0,
        }
    }

    pub const fn write_size(self) -> usize {
        match self {
            Self::Store { format, .. } => format.size(),
            Self::StoreControl | Self::StoreStatus => 2,
            Self::StoreEnvironment { operand16 } => {
                if operand16 {
                    14
                } else {
                    28
                }
            }
            Self::Save { operand16 } => {
                if operand16 {
                    94
                } else {
                    108
                }
            }
            _ => 0,
        }
    }

    pub(crate) const fn is_computation(self) -> bool {
        !matches!(
            self,
            Self::Initialize
                | Self::ClearExceptions
                | Self::Wait
                | Self::NoOperation
                | Self::LoadControl
                | Self::StoreControl
                | Self::StoreStatus
                | Self::StatusToAx
                | Self::LoadEnvironment { .. }
                | Self::StoreEnvironment { .. }
                | Self::Restore { .. }
                | Self::Save { .. }
        )
    }

    pub const fn ignores_pending_exception(self) -> bool {
        matches!(
            self,
            Self::Initialize
                | Self::ClearExceptions
                | Self::StoreControl
                | Self::StoreStatus
                | Self::StatusToAx
                | Self::StoreEnvironment { .. }
                | Self::Save { .. }
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum X87Error {
    InvalidOperand,
    InvalidInputLength,
    NumericalFailure,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct X87Outcome {
    pub state: X87State,
    pub memory: [u8; 108],
    pub memory_len: usize,
    pub eflags: Option<u32>,
    pub unmasked_exception: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct X87Effects {
    pub(crate) memory_len: usize,
    pub(crate) eflags: Option<u32>,
    pub(crate) unmasked_exception: bool,
}

struct Evaluation<'state, 'output> {
    state: &'state mut X87State,
    memory: Option<&'output mut [u8; 108]>,
    memory_len: usize,
    eflags: Option<u32>,
    unmasked_exception: bool,
}

impl X87State {
    pub fn top(&self) -> usize {
        usize::from((self.status >> 11) & 7)
    }

    pub fn stack(&self, index: u8) -> Option<Ext80> {
        if index >= 8 {
            return None;
        }
        let physical = (self.top() + usize::from(index)) & 7;
        if (self.tag >> (physical * 2)) & 3 == 3 {
            None
        } else {
            Some(Ext80(self.registers[physical]))
        }
    }

    pub fn evaluate(
        &self,
        operation: X87Operation,
        input: &[u8],
        eflags: u32,
    ) -> Result<X87Outcome, X87Error> {
        let mut state = *self;
        let mut memory = [0; 108];
        let X87Effects {
            memory_len,
            eflags,
            unmasked_exception,
        } = state.evaluate_in_place(
            operation,
            input,
            eflags,
            Some(&mut memory),
            &mut Default::default(),
        )?;
        Ok(X87Outcome {
            state,
            memory,
            memory_len,
            eflags,
            unmasked_exception,
        })
    }

    pub(crate) fn evaluate_in_place(
        &mut self,
        operation: X87Operation,
        input: &[u8],
        eflags: u32,
        memory: Option<&mut [u8; 108]>,
        workspace: &mut crate::math::NumericalWorkspace,
    ) -> Result<X87Effects, X87Error> {
        if input.len() != operation.read_size() || operation.write_size() != 0 && memory.is_none() {
            return Err(X87Error::InvalidInputLength);
        }
        let original = *self;
        let mut outcome = Evaluation {
            state: self,
            memory,
            memory_len: 0,
            eflags: None,
            unmasked_exception: false,
        };
        if !operation.ignores_pending_exception() && original.status & 0x3f & !original.control != 0
        {
            outcome.state.status |= 0x8080;
            outcome.unmasked_exception = true;
            return Ok(outcome.into_effects());
        }
        let computation = operation.is_computation();
        if computation {
            outcome.state.status &= !0x3f;
        }
        if let Err(error) = outcome.run(operation, input, eflags, workspace) {
            *outcome.state = original;
            return Err(error);
        }
        if computation {
            let mut raised = outcome.state.status & 0x3f;
            if raised & INVALID != 0 {
                raised = INVALID;
            } else if raised & 4 != 0 {
                raised = 4;
            } else if raised & DENORMAL & !outcome.state.control != 0 {
                raised = DENORMAL;
            }
            let unmasked = raised & !outcome.state.control;
            let suppress = unmasked & 7 != 0 || operation.write_size() != 0 && unmasked & 0x18 != 0;
            if operation.write_size() != 0 && unmasked & 0x18 != 0 {
                raised &= !32;
                outcome.state.status &= !C1;
            }
            let exception_status =
                (outcome.state.status & !0x3f) | (original.status & 0x3f) | raised;
            if suppress {
                *outcome.state = original;
                outcome.state.status = (original.status & !0x82ff) | (exception_status & 0x82ff);
                outcome.memory_len = 0;
                outcome.eflags = None;
            } else {
                outcome.state.status = exception_status;
            }
            if unmasked != 0 {
                outcome.state.status |= 0x8080;
                outcome.unmasked_exception = true;
            }
        }
        Ok(outcome.into_effects())
    }
}

impl Evaluation<'_, '_> {
    fn into_effects(self) -> X87Effects {
        X87Effects {
            memory_len: self.memory_len,
            eflags: self.eflags,
            unmasked_exception: self.unmasked_exception,
        }
    }

    fn physical(&self, index: u8) -> Result<usize, X87Error> {
        if index >= 8 {
            Err(X87Error::InvalidOperand)
        } else {
            Ok((self.state.top() + usize::from(index)) & 7)
        }
    }

    fn read(&mut self, index: u8) -> Result<Ext80, X87Error> {
        let physical = self.physical(index)?;
        if (self.state.tag >> (physical * 2)) & 3 == 3 {
            self.stack_fault(false);
            Ok(Ext80::INDEFINITE)
        } else {
            Ok(Ext80(self.state.registers[physical]))
        }
    }

    fn write(&mut self, index: u8, value: Ext80) -> Result<(), X87Error> {
        let physical = self.physical(index)?;
        self.state.registers[physical] = value.0;
        self.state.tag =
            (self.state.tag & !(3 << (physical * 2))) | (value.tag() << (physical * 2));
        Ok(())
    }

    fn set_top(&mut self, top: usize) {
        self.state.status = (self.state.status & !0x3800) | (((top & 7) as u16) << 11);
    }

    fn push(&mut self, value: Ext80) {
        let top = self.state.top().wrapping_sub(1) & 7;
        let occupied = (self.state.tag >> (top * 2)) & 3 != 3;
        if occupied {
            self.stack_fault(true);
        }
        self.set_top(top);
        let _ = self.write(0, if occupied { Ext80::INDEFINITE } else { value });
    }

    fn pop(&mut self) {
        let top = self.state.top();
        self.state.tag |= 3 << (top * 2);
        self.set_top(top + 1);
    }

    fn stack_fault(&mut self, overflow: bool) {
        self.state.status =
            (self.state.status & !C1) | INVALID | 0x40 | if overflow { C1 } else { 0 };
    }

    fn rounded(&mut self, result: Rounded) -> Ext80 {
        self.state.status =
            (self.state.status & !C1) | result.exceptions | if result.incremented { C1 } else { 0 };
        result.value
    }

    fn run(
        &mut self,
        operation: X87Operation,
        input: &[u8],
        eflags: u32,
        workspace: &mut crate::math::NumericalWorkspace,
    ) -> Result<(), X87Error> {
        match operation {
            X87Operation::Initialize => {
                let registers = self.state.registers;
                *self.state = X87State::default();
                self.state.registers = registers;
            }
            X87Operation::ClearExceptions => self.state.status &= 0x7f00,
            X87Operation::Wait | X87Operation::NoOperation | X87Operation::StatusToAx => {}
            X87Operation::Load(format) => {
                let (value, exceptions) = load(format, input);
                self.state.status &= !C1;
                self.state.status |= exceptions;
                self.push(value);
            }
            X87Operation::Store {
                format,
                pop,
                truncate,
            } => {
                let value = self.read(0)?;
                let (bytes, exceptions, incremented) = store(
                    format,
                    value,
                    if truncate {
                        (self.state.control & !0xc00) | 0xc00
                    } else {
                        self.state.control
                    },
                );
                self.state.status =
                    (self.state.status & !C1) | exceptions | if incremented { C1 } else { 0 };
                self.memory.as_deref_mut().expect("validated x87 output")[..format.size()]
                    .copy_from_slice(&bytes[..format.size()]);
                self.memory_len = format.size();
                if pop {
                    self.pop();
                }
            }
            X87Operation::LoadRegister { index } => {
                let value = self.read(index)?;
                self.push(value);
            }
            X87Operation::StoreRegister { index, pop } => {
                let value = self.read(0)?;
                self.write(index, value)?;
                if pop {
                    self.pop();
                }
            }
            X87Operation::StoreRegisterNoUnderflow { index } => {
                // empty payloads are unspecified; use indefinite without raising stack underflow.
                let value = self.state.stack(0).unwrap_or(Ext80::INDEFINITE);
                self.write(index, value)?;
                self.pop();
            }
            X87Operation::Exchange { index } => {
                let a = self.read(0)?;
                let b = self.read(index)?;
                self.write(0, b)?;
                self.write(index, a)?;
                self.state.status &= !C1;
            }
            X87Operation::Binary {
                kind,
                destination,
                source,
                pop,
            } => {
                let a = self.read(destination)?;
                let b = self.read(source)?;
                let result = self.rounded(binary(kind, a, b, self.state.control));
                self.write(destination, result)?;
                if pop {
                    self.pop();
                }
            }
            X87Operation::BinaryMemory { kind, format } => {
                let a = self.read(0)?;
                let (b, exceptions) = load(format, input);
                if !matches!(a.kind(), Kind::NaN { .. }) || exceptions & INVALID != 0 {
                    self.state.status |= exceptions;
                }
                let result = self.rounded(binary(kind, a, b, self.state.control));
                self.write(0, result)?;
            }
            X87Operation::Compare {
                index,
                quiet,
                pop,
                integer_flags,
            } => {
                let a = self.read(0)?;
                let b = self.read(index)?;
                self.compare(a, b, quiet, integer_flags, eflags);
                for _ in 0..pop.min(2) {
                    self.pop();
                }
            }
            X87Operation::CompareMemory { format, pop } => {
                let a = self.read(0)?;
                let (b, exceptions) = load(format, input);
                if !matches!(a.kind(), Kind::NaN { .. }) || exceptions & INVALID != 0 {
                    self.state.status |= exceptions;
                }
                self.compare(a, b, false, false, eflags);
                if pop {
                    self.pop();
                }
            }
            X87Operation::ConditionalMove { index, condition } => {
                let a = self.read(0)?;
                let b = self.read(index)?;
                let cf = eflags & 1 != 0;
                let pf = eflags & 4 != 0;
                let zf = eflags & 0x40 != 0;
                let take = match condition {
                    X87Condition::Below => cf,
                    X87Condition::Equal => zf,
                    X87Condition::BelowOrEqual => cf || zf,
                    X87Condition::Unordered => pf,
                    X87Condition::NotBelow => !cf,
                    X87Condition::NotEqual => !zf,
                    X87Condition::Above => !cf && !zf,
                    X87Condition::Ordered => !pf,
                };
                self.write(0, if take { b } else { a })?;
            }
            X87Operation::Test => {
                let a = self.read(0)?;
                self.compare(a, Ext80::ZERO, false, false, eflags);
            }
            X87Operation::Examine => {
                let value = self.state.stack(0);
                let class = match value.map(Ext80::kind) {
                    None => C0 | C3,
                    Some(Kind::Zero) => C3,
                    Some(Kind::Infinity) => C0 | C2,
                    Some(Kind::NaN { .. }) => C0,
                    Some(Kind::Unsupported) => 0,
                    Some(Kind::Finite { .. }) if value.is_some_and(|v| v.exponent_bits() == 0) => {
                        C2 | C3
                    }
                    Some(Kind::Finite { .. }) => C2,
                };
                self.state.status = (self.state.status & !CONDITIONS)
                    | class
                    | if value.is_some_and(Ext80::negative) {
                        C1
                    } else {
                        0
                    };
            }
            X87Operation::ChangeSign | X87Operation::Absolute => {
                let a = self.read(0)?;
                self.write(
                    0,
                    if matches!(operation, X87Operation::ChangeSign) {
                        a.negated()
                    } else {
                        a.absolute()
                    },
                )?;
                self.state.status &= !C1;
            }
            X87Operation::SquareRoot => {
                let a = self.read(0)?;
                let value = self.rounded(a.sqrt(self.state.control));
                self.write(0, value)?;
            }
            X87Operation::RoundToInteger => {
                let a = self.read(0)?;
                let value = self.rounded(a.round_integer(self.state.control));
                self.write(0, value)?;
            }
            X87Operation::Scale => {
                let a = self.read(0)?;
                let b = self.read(1)?;
                let value = self.rounded(a.scaled(b, self.state.control));
                self.write(0, value)?;
            }
            X87Operation::Extract => {
                let a = self.read(0)?;
                if let Some(result) = Ext80::special(a, Ext80::ZERO) {
                    let value = self.rounded(result);
                    self.write(0, value)?;
                    self.push(value);
                    return Ok(());
                }
                self.state.status &= !C1;
                self.state.status |= a.operand_exceptions();
                match a.kind() {
                    Kind::Finite {
                        significand,
                        exponent,
                    } => {
                        self.write(0, Ext80::from_i64(i64::from(exponent)))?;
                        self.push(Ext80::from_parts(a.negative(), 16383, significand));
                    }
                    Kind::Zero => {
                        self.state.status |= 4;
                        self.write(0, Ext80::from_parts(true, 0x7fff, 1 << 63))?;
                        self.push(a);
                    }
                    Kind::Infinity => {
                        self.write(0, Ext80::from_parts(false, 0x7fff, 1 << 63))?;
                        self.push(a);
                    }
                    _ => {
                        self.state.status |= INVALID;
                        self.write(0, Ext80::INDEFINITE)?;
                        self.push(Ext80::INDEFINITE);
                    }
                }
            }
            X87Operation::Remainder { nearest } => self.remainder(nearest)?,
            X87Operation::Exp2MinusOne => {
                let x = self.read(0)?;
                // outside the documented domain the instruction result is undefined;
                // this profile retains the source instead of evaluating an unbounded exp.
                if matches!(x.kind(), Kind::Finite { .. })
                    && x.absolute().compare(Ext80::ONE, true).0 == Some(Ordering::Greater)
                {
                    self.state.status &= !C1;
                    return Ok(());
                }
                let result = super::transcendental::evaluate(
                    super::transcendental::Function::Exp2MinusOne,
                    x,
                    Ext80::ZERO,
                    self.state.control,
                    workspace,
                )?;
                let value = self.rounded(result);
                self.write(0, value)?;
            }
            X87Operation::Log2Multiply { .. } | X87Operation::PartialAtan => {
                let x = self.read(0)?;
                let y = self.read(1)?;
                let function = match operation {
                    X87Operation::PartialAtan => super::transcendental::Function::Atan2,
                    X87Operation::Log2Multiply { plus_one: true } => {
                        super::transcendental::Function::Log2PlusOneMultiply
                    }
                    _ => super::transcendental::Function::Log2Multiply,
                };
                let result =
                    super::transcendental::evaluate(function, x, y, self.state.control, workspace)?;
                let value = self.rounded(result);
                self.write(1, value)?;
                self.pop();
            }
            X87Operation::Trigonometric { function } => {
                let x = self.read(0)?;
                if matches!(x.kind(),Kind::Finite{exponent,..}if exponent>=63) {
                    self.state.status |= C2;
                    return Ok(());
                }
                self.state.status &= !C2;
                let kind = match function {
                    0 | 3 => super::transcendental::Function::Sin,
                    1 => super::transcendental::Function::Cos,
                    2 => super::transcendental::Function::Tan,
                    _ => return Err(X87Error::InvalidOperand),
                };
                let result = super::transcendental::evaluate(
                    kind,
                    x,
                    Ext80::ZERO,
                    self.state.control,
                    workspace,
                )?;
                let value = self.rounded(result);
                self.write(0, value)?;
                if function == 2 {
                    self.push(Ext80::ONE);
                }
                if function == 3 {
                    let cosine = super::transcendental::evaluate(
                        super::transcendental::Function::Cos,
                        x,
                        Ext80::ZERO,
                        self.state.control,
                        workspace,
                    )?;
                    let value = self.rounded(cosine);
                    self.push(value);
                }
            }
            X87Operation::LoadConstant { index } => {
                let value = constant(index, X87Rounding::from_control(self.state.control))
                    .ok_or(X87Error::InvalidOperand)?;
                self.state.status &= !C1;
                self.push(value);
            }
            X87Operation::Free { index, pop } => {
                let physical = self.physical(index)?;
                self.state.tag |= 3 << (physical * 2);
                if pop {
                    self.pop();
                }
            }
            X87Operation::AdjustTop { increment } => {
                self.set_top(self.state.top().wrapping_add(if increment { 1 } else { 7 }));
                self.state.status &= !C1;
            }
            X87Operation::LoadControl => {
                self.state.control =
                    u16::from_le_bytes(input.try_into().map_err(|_| X87Error::InvalidInputLength)?);
                self.refresh_summary();
            }
            X87Operation::StoreControl | X87Operation::StoreStatus => {
                let word = if matches!(operation, X87Operation::StoreControl) {
                    self.state.control
                } else {
                    self.state.status
                };
                self.memory.as_deref_mut().expect("validated x87 output")[..2]
                    .copy_from_slice(&word.to_le_bytes());
                self.memory_len = 2;
            }
            X87Operation::LoadEnvironment { operand16 } => self.load_environment(input, operand16),
            X87Operation::StoreEnvironment { operand16 } => {
                self.save_environment(operand16);
                self.state.control |= 0x3f;
                self.refresh_summary();
            }
            X87Operation::Restore { operand16 } => {
                let environment_size = if operand16 { 14 } else { 28 };
                self.load_environment(&input[..environment_size], operand16);
                for index in 0..8 {
                    let physical = (self.state.top() + index) & 7;
                    self.state.registers[physical].copy_from_slice(
                        &input[environment_size + index * 10..environment_size + (index + 1) * 10],
                    );
                }
            }
            X87Operation::Save { operand16 } => {
                self.save_environment(operand16);
                let environment_size = self.memory_len;
                let memory = self.memory.as_deref_mut().expect("validated x87 output");
                for index in 0..8 {
                    let physical = (self.state.top() + index) & 7;
                    memory[environment_size + index * 10..environment_size + (index + 1) * 10]
                        .copy_from_slice(&self.state.registers[physical]);
                }
                self.memory_len += 80;
                let registers = self.state.registers;
                *self.state = X87State::default();
                self.state.registers = registers;
            }
        }
        Ok(())
    }

    fn refresh_summary(&mut self) {
        if self.state.status & 0x3f & !self.state.control != 0 {
            self.state.status |= 0x8080;
        } else {
            self.state.status &= !0x8080;
        }
    }

    fn compare(&mut self, a: Ext80, b: Ext80, quiet: bool, integer_flags: bool, eflags: u32) {
        let (order, exceptions) = a.compare(b, quiet);
        self.state.status |= exceptions;
        self.state.status &= !C1;
        if integer_flags {
            let bits = match order {
                Some(Ordering::Greater) => 0,
                Some(Ordering::Less) => 1,
                Some(Ordering::Equal) => 0x40,
                None => 0x45,
            };
            self.eflags = Some((eflags & !0x8d5) | bits);
        } else {
            let bits = match order {
                Some(Ordering::Greater) => 0,
                Some(Ordering::Less) => C0,
                Some(Ordering::Equal) => C3,
                None => C0 | C2 | C3,
            };
            self.state.status = (self.state.status & !CONDITIONS) | bits;
        }
    }

    fn remainder(&mut self, nearest: bool) -> Result<(), X87Error> {
        let a = self.read(0)?;
        let b = self.read(1)?;
        let (remainder, partial, q) = a.remainder(b, nearest);
        let value = self.rounded(remainder);
        self.write(0, value)?;
        if partial {
            self.state.status |= C2;
        } else {
            self.state.status = (self.state.status & !CONDITIONS)
                | if q & 4 != 0 { C0 } else { 0 }
                | if q & 1 != 0 { C1 } else { 0 }
                | if q & 2 != 0 { C3 } else { 0 };
        }
        Ok(())
    }

    fn save_environment(&mut self, operand16: bool) {
        let memory = self.memory.as_deref_mut().expect("validated x87 output");
        if operand16 {
            for (offset, word) in [
                (0, self.state.control),
                (2, self.state.status),
                (4, self.state.tag),
                (6, self.state.instruction_pointer as u16),
                (8, self.state.code_selector),
                (10, self.state.data_pointer as u16),
                (12, self.state.data_selector),
            ] {
                memory[offset..offset + 2].copy_from_slice(&word.to_le_bytes());
            }
            self.memory_len = 14;
        } else {
            memory[..28].fill(0xff);
            for (offset, word) in [
                (0, self.state.control),
                (4, self.state.status),
                (8, self.state.tag),
                (16, self.state.code_selector),
                (18, self.state.opcode),
                (24, self.state.data_selector),
            ] {
                memory[offset..offset + 2].copy_from_slice(&word.to_le_bytes());
            }
            memory[12..16].copy_from_slice(&self.state.instruction_pointer.to_le_bytes());
            memory[20..24].copy_from_slice(&self.state.data_pointer.to_le_bytes());
            memory[26..28].fill(0);
            self.memory_len = 28;
        }
    }

    fn load_environment(&mut self, bytes: &[u8], operand16: bool) {
        let word = |offset| u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
        if operand16 {
            self.state.control = word(0);
            self.state.status = word(2);
            self.state.tag = word(4);
            self.state.instruction_pointer = u32::from(word(6));
            self.state.code_selector = word(8);
            self.state.data_pointer = u32::from(word(10));
            self.state.data_selector = word(12);
        } else {
            self.state.control = word(0);
            self.state.status = word(4);
            self.state.tag = word(8);
            self.state.instruction_pointer =
                u32::from_le_bytes(bytes[12..16].try_into().expect("environment pointer"));
            self.state.code_selector = word(16);
            self.state.opcode = word(18) & 0x7ff;
            self.state.data_pointer =
                u32::from_le_bytes(bytes[20..24].try_into().expect("environment pointer"));
            self.state.data_selector = word(24);
        }
        self.refresh_summary();
    }
}

fn binary(kind: X87Binary, a: Ext80, b: Ext80, control: u16) -> Rounded {
    match kind {
        X87Binary::Add => a.add(b, control),
        X87Binary::Multiply => a.multiply(b, control),
        X87Binary::Subtract => a.add(b.negated(), control),
        X87Binary::ReverseSubtract => b.add(a.negated(), control),
        X87Binary::Divide => a.divide(b, control),
        X87Binary::ReverseDivide => b.divide(a, control),
    }
}

fn load(format: X87Format, bytes: &[u8]) -> (Ext80, u16) {
    if format == X87Format::Extended80 {
        return (Ext80(bytes.try_into().expect("extended80")), 0);
    }
    let value = match format {
        X87Format::Float32 => {
            Ext80::from_f32_bits(u32::from_le_bytes(bytes.try_into().expect("float32")))
        }
        X87Format::Float64 => {
            Ext80::from_f64_bits(u64::from_le_bytes(bytes.try_into().expect("float64")))
        }
        X87Format::Extended80 => Ext80(bytes.try_into().expect("extended80")),
        X87Format::Integer16 => Ext80::from_i64(i64::from(i16::from_le_bytes(
            bytes.try_into().expect("integer16"),
        ))),
        X87Format::Integer32 => Ext80::from_i64(i64::from(i32::from_le_bytes(
            bytes.try_into().expect("integer32"),
        ))),
        X87Format::Integer64 => {
            Ext80::from_i64(i64::from_le_bytes(bytes.try_into().expect("integer64")))
        }
        X87Format::PackedBcd => {
            let mut magnitude = 0_i64;
            for index in (0..9).rev() {
                let high = bytes[index] >> 4;
                let low = bytes[index] & 15;
                if high > 9 || low > 9 {
                    return (Ext80::INDEFINITE, INVALID);
                }
                magnitude = magnitude * 100 + i64::from(high) * 10 + i64::from(low);
            }
            let value = Ext80::from_i64(magnitude);
            if bytes[9] & 0x80 != 0 {
                value.negated()
            } else {
                value
            }
        }
    };
    let input_denormal = match format {
        X87Format::Float32 => {
            let bits = u32::from_le_bytes(bytes.try_into().expect("float32"));
            bits & 0x7f80_0000 == 0 && bits & 0x007f_ffff != 0
        }
        X87Format::Float64 => {
            let bits = u64::from_le_bytes(bytes.try_into().expect("float64"));
            bits & 0x7ff0_0000_0000_0000 == 0 && bits & 0x000f_ffff_ffff_ffff != 0
        }
        _ => false,
    };
    let exceptions = match value.kind() {
        Kind::Unsupported | Kind::NaN { signaling: true } => INVALID,
        _ => {
            if input_denormal {
                DENORMAL
            } else {
                value.operand_exceptions()
            }
        }
    };
    (
        if exceptions & INVALID != 0 {
            if matches!(value.kind(), Kind::NaN { .. }) {
                Ext80::from_parts(value.negative(), 0x7fff, value.significand() | (1 << 62))
            } else {
                Ext80::INDEFINITE
            }
        } else {
            value
        },
        exceptions,
    )
}

fn store(format: X87Format, value: Ext80, control: u16) -> ([u8; 10], u16, bool) {
    let mut bytes = [0; 10];
    let mode = X87Rounding::from_control(control);
    let (exceptions, incremented) = match format {
        X87Format::Extended80 => {
            bytes = value.0;
            (0, false)
        }
        X87Format::Float32 | X87Format::Float64 => {
            let (bits, flags, up) = value.to_ieee_with_control(
                if format == X87Format::Float32 { 8 } else { 11 },
                if format == X87Format::Float32 { 23 } else { 52 },
                control,
            );
            let size = format.size();
            bytes[..size].copy_from_slice(&bits.to_le_bytes()[..size]);
            (flags, up)
        }
        X87Format::Integer16 | X87Format::Integer32 | X87Format::Integer64 => {
            let (bits, flags, up) = value.to_integer((format.size() * 8) as u32, mode);
            let size = format.size();
            bytes[..size].copy_from_slice(&bits.to_le_bytes()[..size]);
            (flags, up)
        }
        X87Format::PackedBcd => {
            let (bits, mut flags, up) = value.to_integer(64, mode);
            let mut magnitude = (bits as i64).unsigned_abs();
            if flags & INVALID != 0 || magnitude > 999_999_999_999_999_999 {
                flags |= INVALID;
                bytes[7] = 0xc0;
                bytes[8] = 0xff;
                bytes[9] = 0xff;
            } else {
                for byte in bytes.iter_mut().take(9) {
                    let low = magnitude % 10;
                    magnitude /= 10;
                    let high = magnitude % 10;
                    magnitude /= 10;
                    *byte = (low | (high << 4)) as u8;
                }
                bytes[9] = if value.negative() { 0x80 } else { 0 };
            }
            (flags, up)
        }
    };
    (bytes, exceptions, incremented)
}

fn constant(index: u8, mode: X87Rounding) -> Option<Ext80> {
    let (exponent, mut significand, above) = match index {
        0 => (0x3fff, 0x8000_0000_0000_0000, false),
        1 => (0x4000, 0xd49a_784b_cd1b_8afe, true),
        2 => (0x3fff, 0xb8aa_3b29_5c17_f0bc, false),
        3 => (0x4000, 0xc90f_daa2_2168_c235, true),
        4 => (0x3ffd, 0x9a20_9a84_fbcf_f799, false),
        5 => (0x3ffe, 0xb172_17f7_d1cf_79ac, false),
        6 => return Some(Ext80::ZERO),
        _ => return None,
    };
    if index != 0 {
        if matches!(mode, X87Rounding::Down | X87Rounding::Zero) && above {
            significand -= 1;
        }
        if mode == X87Rounding::Up && !above {
            significand += 1;
        }
    }
    Some(Ext80::from_parts(false, exponent, significand))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn execute(state: &mut X87State, operation: X87Operation, input: &[u8]) -> X87Outcome {
        let outcome = state.evaluate(operation, input, 0x8d7).unwrap();
        *state = outcome.state;
        outcome
    }

    #[test]
    fn stack_push_exchange_pop_and_masked_overflow_preserve_real80() {
        let mut state = X87State::default();
        for value in 1_i64..=8 {
            execute(
                &mut state,
                X87Operation::Load(X87Format::Integer64),
                &value.to_le_bytes(),
            );
        }
        assert_eq!(state.top(), 0);
        assert_eq!(state.stack(0), Some(Ext80::from_i64(8)));
        assert_eq!(state.tag, 0);
        execute(&mut state, X87Operation::Exchange { index: 7 }, &[]);
        assert_eq!(state.stack(0), Some(Ext80::ONE));
        execute(&mut state, X87Operation::LoadConstant { index: 0 }, &[]);
        assert_eq!(state.top(), 7);
        assert_eq!(state.status & 0x241, 0x241);
        assert_eq!(state.stack(0), Some(Ext80::INDEFINITE));
        let result = execute(
            &mut state,
            X87Operation::Store {
                format: X87Format::Extended80,
                pop: true,
                truncate: false,
            },
            &[],
        );
        assert_eq!(&result.memory[..10], &Ext80::INDEFINITE.0);
        assert_eq!(state.top(), 0);
    }

    #[test]
    fn unmasked_invalid_commits_only_exception_status_and_has_pending_priority() {
        let mut state = X87State::default();
        state.control &= !1;
        let initial = state;
        let outcome = state
            .evaluate(X87Operation::SquareRoot, &[], 0x402)
            .unwrap();
        assert!(outcome.unmasked_exception);
        assert_eq!(outcome.state.tag, initial.tag);
        assert_eq!(outcome.state.registers, initial.registers);
        assert_eq!(outcome.state.status & 0x80c1, 0x80c1);
        state = outcome.state;
        let pending = state
            .evaluate(X87Operation::LoadConstant { index: 0 }, &[], 0x402)
            .unwrap();
        assert!(pending.unmasked_exception);
        assert_eq!(pending.state.registers, initial.registers);
        execute(&mut state, X87Operation::ClearExceptions, &[]);
        assert_eq!(state.status & 0x80ff, 0);
    }

    #[test]
    fn unmasked_precision_commits_store_and_pop_before_deferred_delivery() {
        let mut state = X87State::default();
        execute(
            &mut state,
            X87Operation::Load(X87Format::Float64),
            &0x3ff8_0000_0000_0000_u64.to_le_bytes(),
        );
        state.control &= !32;
        let outcome = state
            .evaluate(
                X87Operation::Store {
                    format: X87Format::Integer32,
                    pop: true,
                    truncate: false,
                },
                &[],
                0x402,
            )
            .unwrap();
        assert!(outcome.unmasked_exception);
        assert_eq!(outcome.memory_len, 4);
        assert_eq!(&outcome.memory[..4], &2_i32.to_le_bytes());
        assert_eq!(outcome.state.top(), 0);
        assert_eq!(outcome.state.tag, 0xffff);
        assert_eq!(outcome.state.status, 0x82a0);
    }

    #[test]
    fn unmasked_register_range_results_use_architectural_exponent_bias() {
        for (a, b, mask, expected, flags) in [
            (
                Ext80::from_parts(false, 0x7ffe, 1 << 63),
                Ext80::from_i64(2),
                8,
                Ext80::from_parts(false, 0x1fff, 1 << 63),
                0x8088,
            ),
            (
                Ext80::from_parts(false, 1, 1 << 63),
                Ext80::from_parts(false, 0x3ffe, 1 << 63),
                16,
                Ext80::from_parts(false, 0x6000, 1 << 63),
                0x8090,
            ),
        ] {
            let mut state = X87State::default();
            execute(&mut state, X87Operation::Load(X87Format::Extended80), &b.0);
            execute(&mut state, X87Operation::Load(X87Format::Extended80), &a.0);
            state.control &= !mask;
            let before_top = state.top();
            let outcome = state
                .evaluate(
                    X87Operation::Binary {
                        kind: X87Binary::Multiply,
                        destination: 0,
                        source: 1,
                        pop: false,
                    },
                    &[],
                    0x402,
                )
                .unwrap();
            assert!(outcome.unmasked_exception);
            assert_eq!(outcome.state.top(), before_top);
            assert_eq!(outcome.state.stack(0), Some(expected));
            assert_eq!(outcome.state.status & !0x3800, flags);
        }
    }

    #[test]
    fn unmasked_memory_range_blocks_write_and_pop_including_exact_tiny_values() {
        for (value, mask, flags) in [
            (Ext80::from_parts(false, 16383 + 128, 1 << 63), 8, 0x8088),
            (Ext80::from_parts(false, 16383 - 149, 1 << 63), 16, 0x8090),
        ] {
            let mut state = X87State::default();
            execute(
                &mut state,
                X87Operation::Load(X87Format::Extended80),
                &value.0,
            );
            state.control &= !mask;
            let before = state;
            let outcome = state
                .evaluate(
                    X87Operation::Store {
                        format: X87Format::Float32,
                        pop: true,
                        truncate: false,
                    },
                    &[],
                    0x402,
                )
                .unwrap();
            assert!(outcome.unmasked_exception);
            assert_eq!(outcome.memory_len, 0);
            assert_eq!(outcome.state.registers, before.registers);
            assert_eq!(outcome.state.tag, before.tag);
            assert_eq!(outcome.state.top(), before.top());
            assert_eq!(outcome.state.status & !0x3800, flags);
        }
    }

    #[test]
    fn unmasked_precomputation_priority_preserves_sticky_flags_without_new_postflags() {
        let mut state = X87State::default();
        execute(
            &mut state,
            X87Operation::Load(X87Format::Extended80),
            &Ext80::from_i64(2).0,
        );
        execute(
            &mut state,
            X87Operation::Load(X87Format::Extended80),
            &Ext80::from_parts(false, 0, 1).0,
        );
        state.control &= !DENORMAL;
        state.status |= 32;
        let before = state;
        let outcome = state
            .evaluate(
                X87Operation::Binary {
                    kind: X87Binary::Divide,
                    destination: 0,
                    source: 1,
                    pop: false,
                },
                &[],
                0x402,
            )
            .unwrap();
        assert!(outcome.unmasked_exception);
        assert_eq!(outcome.state.registers, before.registers);
        assert_eq!(outcome.state.status & !0x3800, 0x80a2);
    }

    #[test]
    fn conversion_compare_and_control_use_low_level_bits_and_pop_transactionally() {
        let mut state = X87State::default();
        execute(
            &mut state,
            X87Operation::Load(X87Format::Float64),
            &0x3ff8_0000_0000_0000_u64.to_le_bytes(),
        );
        let stored = execute(
            &mut state,
            X87Operation::Store {
                format: X87Format::Integer16,
                pop: false,
                truncate: false,
            },
            &[],
        );
        assert_eq!(&stored.memory[..2], &2_i16.to_le_bytes());
        assert_eq!(state.status & 0x220, 0x220);
        execute(&mut state, X87Operation::LoadConstant { index: 0 }, &[]);
        let compared = execute(
            &mut state,
            X87Operation::Compare {
                index: 1,
                quiet: true,
                pop: 1,
                integer_flags: true,
            },
            &[],
        );
        assert_eq!(compared.eflags, Some(3));
        assert_eq!(
            state.stack(0),
            Some(Ext80::from_parts(false, 16383, 0xc000_0000_0000_0000))
        );
        execute(
            &mut state,
            X87Operation::LoadControl,
            &0x0f7f_u16.to_le_bytes(),
        );
        let truncated = execute(
            &mut state,
            X87Operation::Store {
                format: X87Format::Integer32,
                pop: true,
                truncate: false,
            },
            &[],
        );
        assert_eq!(&truncated.memory[..4], &1_i32.to_le_bytes());
        assert!(state.stack(0).is_none());
    }

    #[test]
    fn environment_save_restore_round_trips_physical_stack_and_raw80() {
        let mut state = X87State {
            instruction_pointer: 0x12345678,
            data_pointer: 0x87654321,
            opcode: 0x345,
            ..X87State::default()
        };
        execute(
            &mut state,
            X87Operation::Load(X87Format::Integer64),
            &i64::MAX.to_le_bytes(),
        );
        execute(&mut state, X87Operation::LoadConstant { index: 3 }, &[]);
        let original = state;
        let saved = execute(&mut state, X87Operation::Save { operand16: false }, &[]);
        assert_eq!(saved.memory_len, 108);
        assert_eq!(state.tag, 0xffff);
        execute(
            &mut state,
            X87Operation::Restore { operand16: false },
            &saved.memory[..108],
        );
        assert_eq!(state, original);
        for operation in [
            X87Operation::Load(X87Format::Float32),
            X87Operation::Binary {
                kind: X87Binary::ReverseDivide,
                destination: 7,
                source: 0,
                pop: true,
            },
            X87Operation::Compare {
                index: 1,
                quiet: true,
                pop: 2,
                integer_flags: false,
            },
            X87Operation::Save { operand16: true },
        ] {
            assert_eq!(X87Operation::decode(operation.encode()), Some(operation));
        }
        assert_eq!(X87Operation::decode(0xffff_ffff), None);
    }
}
