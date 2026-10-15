use super::{CodecError, encoder::Encoder};
use crate::graphics::d3d8::{
    commands::ShaderDefinition,
    shader::{Declaration, Program, Register, RegisterFile, SourceModifier, Stage, Tessellation},
};

pub(super) fn encode(
    output: &mut Encoder<'_>,
    definition: &ShaderDefinition<'_>,
) -> Result<(), CodecError> {
    match definition {
        ShaderDefinition::Vertex {
            declaration,
            program: vertex,
        } => {
            if vertex.is_some_and(|program| program.version().stage != Stage::Vertex) {
                return Err(CodecError::InvalidCommand);
            }
            output.u32(1)?;
            declaration_ir(output, declaration)?;
            output.u32(u32::from(vertex.is_some()))?;
            if let Some(vertex) = vertex {
                program(output, vertex)?;
            }
        }
        ShaderDefinition::Pixel { program: pixel } => {
            if pixel.version().stage != Stage::Pixel {
                return Err(CodecError::InvalidCommand);
            }
            output.u32(2)?;
            program(output, pixel)?;
        }
    }
    Ok(())
}

fn declaration_ir(output: &mut Encoder<'_>, declaration: &Declaration) -> Result<(), CodecError> {
    output.count(declaration.elements().len())?;
    for element in declaration.elements() {
        output.words(&[
            u32::from(element.stream),
            element.offset,
            u32::from(element.register),
            element.kind as u32,
        ])?;
    }
    output.words(declaration.strides())?;
    output.count(declaration.constants().len())?;
    for (index, constant) in declaration.constants() {
        output.u32(u32::from(*index))?;
        output.words(constant)?;
    }
    output.count(declaration.tessellation().len())?;
    for tessellation in declaration.tessellation() {
        match tessellation {
            Tessellation::Normal {
                input,
                output: register,
            } => output.words(&[1, u32::from(*input), u32::from(*register)])?,
            Tessellation::Coordinates { output: register } => {
                output.words(&[2, 0, u32::from(*register)])?
            }
        }
    }
    Ok(())
}

fn program(output: &mut Encoder<'_>, program: &Program) -> Result<(), CodecError> {
    let version = program.version();
    output.words(&[
        match version.stage {
            Stage::Vertex => 1,
            Stage::Pixel => 2,
        },
        u32::from(version.minor),
    ])?;
    output.count(program.bind_constants().count())?;
    for (index, constant) in program.bind_constants() {
        output.u32(u32::from(index))?;
        output.words(&constant)?;
    }
    output.count(program.executable_instructions().count())?;
    for instruction in program.executable_instructions() {
        output.words(&[
            instruction.operation as u32,
            u32::from(instruction.coissue),
            u32::from(instruction.destination.is_some()),
        ])?;
        if let Some(destination) = instruction.destination {
            register(output, destination.register)?;
            output.words(&[
                u32::from(destination.write_mask),
                u32::from(destination.saturate),
                i32::from(destination.shift) as u32,
            ])?;
        }
        for source in instruction.sources {
            output.u32(u32::from(source.is_some()))?;
            if let Some(source) = source {
                register(output, source.register)?;
                let swizzle = source
                    .swizzle
                    .iter()
                    .enumerate()
                    .fold(0u32, |bits, (index, component)| {
                        bits | (u32::from(*component) << (index * 2))
                    });
                output.words(&[
                    swizzle,
                    modifier(source.modifier),
                    u32::from(source.relative),
                ])?;
            }
        }
        output.u32(u32::from(instruction.immediate.is_some()))?;
        if let Some(immediate) = instruction.immediate {
            output.words(&immediate)?;
        }
    }
    Ok(())
}

fn register(output: &mut Encoder<'_>, register: Register) -> Result<(), CodecError> {
    let file = match register.file {
        RegisterFile::Temporary => 0,
        RegisterFile::Input => 1,
        RegisterFile::Constant => 2,
        RegisterFile::Address => 3,
        RegisterFile::Texture => 4,
        RegisterFile::RasterOutput => 5,
        RegisterFile::ColorOutput => 6,
        RegisterFile::TextureOutput => 7,
    };
    output.words(&[file, u32::from(register.index)])
}

fn modifier(modifier: SourceModifier) -> u32 {
    match modifier {
        SourceModifier::None => 0,
        SourceModifier::Negate => 1,
        SourceModifier::Bias => 2,
        SourceModifier::BiasNegate => 3,
        SourceModifier::Sign => 4,
        SourceModifier::SignNegate => 5,
        SourceModifier::Complement => 6,
        SourceModifier::Double => 7,
        SourceModifier::DoubleNegate => 8,
        SourceModifier::DivideZ => 9,
        SourceModifier::DivideW => 10,
    }
}
