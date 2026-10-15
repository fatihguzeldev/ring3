use super::{ShaderError, Stage, Version};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterFile {
    Temporary,
    Input,
    Constant,
    Address,
    Texture,
    RasterOutput,
    ColorOutput,
    TextureOutput,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Register {
    pub file: RegisterFile,
    pub index: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Destination {
    pub register: Register,
    pub write_mask: u8,
    pub saturate: bool,
    pub shift: i8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceModifier {
    None,
    Negate,
    Bias,
    BiasNegate,
    Sign,
    SignNegate,
    Complement,
    Double,
    DoubleNegate,
    DivideZ,
    DivideW,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Source {
    pub register: Register,
    pub swizzle: [u8; 4],
    pub modifier: SourceModifier,
    pub relative: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Operation {
    Nop = 0,
    Move = 1,
    Add = 2,
    Subtract = 3,
    MultiplyAdd = 4,
    Multiply = 5,
    Reciprocal = 6,
    ReciprocalSqrt = 7,
    Dot3 = 8,
    Dot4 = 9,
    Minimum = 10,
    Maximum = 11,
    LessThan = 12,
    GreaterEqual = 13,
    Exp = 14,
    Log = 15,
    Lit = 16,
    Distance = 17,
    Lerp = 18,
    Fraction = 19,
    Matrix4x4 = 20,
    Matrix4x3 = 21,
    Matrix3x4 = 22,
    Matrix3x3 = 23,
    Matrix3x2 = 24,
    TextureCoordinate = 64,
    Kill = 65,
    Sample = 66,
    Bump = 67,
    BumpLuminance = 68,
    TextureAr = 69,
    TextureGb = 70,
    Matrix3x2Pad = 71,
    Matrix3x2Sample = 72,
    Matrix3x3Pad = 73,
    Matrix3x3Sample = 74,
    Matrix3x3Specular = 76,
    Matrix3x3VSpecular = 77,
    ExpPartial = 78,
    LogPartial = 79,
    Conditional = 80,
    Define = 81,
    TextureRgb = 82,
    TextureDot3Sample = 83,
    Matrix3x2Depth = 84,
    TextureDot3 = 85,
    TextureMatrix3x3 = 86,
    TextureDepth = 87,
    Compare = 88,
    Bump14 = 89,
    Phase = 0xfffd,
}

impl Operation {
    pub(super) fn decode(opcode: u16, version: Version) -> Result<Self, ShaderError> {
        use Operation::*;
        let operation = match opcode {
            0 => Nop,
            1 => Move,
            2 => Add,
            3 => Subtract,
            4 => MultiplyAdd,
            5 => Multiply,
            6 => Reciprocal,
            7 => ReciprocalSqrt,
            8 => Dot3,
            9 => Dot4,
            10 => Minimum,
            11 => Maximum,
            12 => LessThan,
            13 => GreaterEqual,
            14 => Exp,
            15 => Log,
            16 => Lit,
            17 => Distance,
            18 => Lerp,
            19 => Fraction,
            20 => Matrix4x4,
            21 => Matrix4x3,
            22 => Matrix3x4,
            23 => Matrix3x3,
            24 => Matrix3x2,
            64 => TextureCoordinate,
            65 => Kill,
            66 => Sample,
            67 => Bump,
            68 => BumpLuminance,
            69 => TextureAr,
            70 => TextureGb,
            71 => Matrix3x2Pad,
            72 => Matrix3x2Sample,
            73 => Matrix3x3Pad,
            74 => Matrix3x3Sample,
            76 => Matrix3x3Specular,
            77 => Matrix3x3VSpecular,
            78 => ExpPartial,
            79 => LogPartial,
            80 => Conditional,
            81 => Define,
            82 => TextureRgb,
            83 => TextureDot3Sample,
            84 => Matrix3x2Depth,
            85 => TextureDot3,
            86 => TextureMatrix3x3,
            87 => TextureDepth,
            88 => Compare,
            89 => Bump14,
            0xfffd => Phase,
            _ => return Err(ShaderError::InvalidInstruction),
        };
        let valid = match version.stage {
            Stage::Vertex => matches!(
                operation,
                Nop | Move
                    | Add
                    | Subtract
                    | MultiplyAdd
                    | Multiply
                    | Reciprocal
                    | ReciprocalSqrt
                    | Dot3
                    | Dot4
                    | Minimum
                    | Maximum
                    | LessThan
                    | GreaterEqual
                    | Exp
                    | Log
                    | Lit
                    | Distance
                    | Fraction
                    | Matrix4x4
                    | Matrix4x3
                    | Matrix3x4
                    | Matrix3x3
                    | Matrix3x2
                    | ExpPartial
                    | LogPartial
                    | Define
            ),
            Stage::Pixel => match operation {
                Nop | Move | Add | Subtract | MultiplyAdd | Multiply | Dot3 | Lerp
                | Conditional | Define | Kill => true,
                Dot4 | Compare => version.minor >= 2,
                TextureCoordinate | Sample => true,
                Bump | BumpLuminance | TextureAr | TextureGb | Matrix3x2Pad | Matrix3x2Sample
                | Matrix3x3Pad | Matrix3x3Sample | Matrix3x3Specular | Matrix3x3VSpecular => {
                    version.minor <= 3
                }
                TextureRgb | TextureDot3Sample | TextureDot3 | TextureMatrix3x3 => {
                    (2..=3).contains(&version.minor)
                }
                Matrix3x2Depth => version.minor == 3,
                TextureDepth | Bump14 | Phase => version.minor == 4,
                _ => false,
            },
        };
        if valid {
            Ok(operation)
        } else {
            Err(ShaderError::InvalidInstruction)
        }
    }

    pub fn source_count(self, version: Version) -> usize {
        use Operation::*;
        match self {
            Nop | Define | Phase | Kill | TextureDepth => 0,
            TextureCoordinate | Sample => usize::from(version.minor == 4),
            Move | Reciprocal | ReciprocalSqrt | Exp | Log | Lit | Fraction | ExpPartial
            | LogPartial => 1,
            MultiplyAdd | Lerp | Conditional | Compare => 3,
            Matrix3x3Specular => 2,
            Add | Subtract | Multiply | Dot3 | Dot4 | Minimum | Maximum | LessThan
            | GreaterEqual | Distance | Matrix4x4 | Matrix4x3 | Matrix3x4 | Matrix3x3
            | Matrix3x2 | Bump14 => 2,
            _ => 1,
        }
    }

    pub fn is_texture(self) -> bool {
        (64..=77).contains(&(self as u16)) || (82..=87).contains(&(self as u16))
    }
    pub fn has_destination(self) -> bool {
        !matches!(self, Self::Nop | Self::Phase)
    }

    pub(super) fn slots(self, version: Version) -> (u16, u16) {
        if version.stage == Stage::Vertex {
            let cost = match self {
                Self::Define => 0,
                Self::Exp | Self::Log => 10,
                Self::Fraction => 3,
                Self::Matrix4x4 | Self::Matrix3x4 => 4,
                Self::Matrix4x3 | Self::Matrix3x3 => 3,
                Self::Matrix3x2 => 2,
                _ => 1,
            };
            (cost, 0)
        } else {
            match self {
                Self::Define | Self::Phase | Self::Nop => (0, 0),
                Self::BumpLuminance => (1, 1),
                Self::Bump14 => (2, 0),
                Self::Dot4 | Self::Compare if version.minor < 4 => (2, 0),
                operation if operation.is_texture() => (0, 1),
                _ => (1, 0),
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Instruction {
    pub operation: Operation,
    pub destination: Option<Destination>,
    pub sources: [Option<Source>; 3],
    pub immediate: Option<[u32; 4]>,
    pub coissue: bool,
}
