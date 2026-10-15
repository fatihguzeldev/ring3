mod declaration;
mod fvf;
mod instruction;
mod parser;
mod registers;
mod validation;

pub use declaration::{Declaration, Element, ElementType, Tessellation};
pub use fvf::Fvf;
pub use instruction::{
    Destination, Instruction, Operation, Register, RegisterFile, Source, SourceModifier,
};
pub use parser::Program;

pub const MAX_SHADER_TOKENS: usize = 16_384;
pub const MAX_DECLARATION_TOKENS: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Vertex,
    Pixel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version {
    pub stage: Stage,
    pub minor: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderError {
    InvalidVersion,
    InvalidToken,
    InvalidInstruction,
    InvalidRegister,
    InvalidModifier,
    InvalidDeclaration,
    InvalidFvf,
    UninitializedRegister,
    InstructionLimit,
    TokenLimit,
    Truncated,
    Allocation,
}

#[cfg(test)]
mod tests;
