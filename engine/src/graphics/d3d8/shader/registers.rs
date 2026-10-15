use super::{
    Destination, Operation, Register, RegisterFile, ShaderError, Source, SourceModifier, Stage,
    Version,
};

fn register(token: u32, version: Version) -> Result<Register, ShaderError> {
    use RegisterFile::*;
    let index = (token & 0x7ff) as u16;
    let file = match ((token >> 28) & 7, version.stage) {
        (0, _) => Temporary,
        (1, _) => Input,
        (2, _) => Constant,
        (3, Stage::Vertex) => Address,
        (3, Stage::Pixel) => Texture,
        (4, Stage::Vertex) => RasterOutput,
        (5, Stage::Vertex) => ColorOutput,
        (6, Stage::Vertex) => TextureOutput,
        _ => return Err(ShaderError::InvalidRegister),
    };
    let limit = match (version.stage, file) {
        (Stage::Vertex, Temporary) => 12,
        (Stage::Vertex, Input) => 16,
        (Stage::Vertex, Constant) => 96,
        (Stage::Vertex, Address) => 1,
        (Stage::Vertex, RasterOutput) => 3,
        (Stage::Vertex, ColorOutput) => 2,
        (Stage::Vertex, TextureOutput) => 8,
        (Stage::Pixel, Temporary) => {
            if version.minor == 4 {
                6
            } else {
                2
            }
        }
        (Stage::Pixel, Texture) => {
            if version.minor == 4 {
                6
            } else {
                4
            }
        }
        (Stage::Pixel, Input) => 2,
        (Stage::Pixel, Constant) => 8,
        _ => return Err(ShaderError::InvalidRegister),
    };
    if index >= limit {
        return Err(ShaderError::InvalidRegister);
    }
    Ok(Register { file, index })
}

pub(super) fn destination(
    token: u32,
    version: Version,
    operation: Operation,
) -> Result<Destination, ShaderError> {
    use RegisterFile::*;
    if token & 0x8000_0000 == 0 || token & 0x0000_f800 != 0 {
        return Err(ShaderError::InvalidToken);
    }
    let register = register(token, version)?;
    let write_mask = ((token >> 16) & 15) as u8;
    let modifier = (token >> 20) & 15;
    let shift = ((token >> 24) & 15) as i8;
    let shift = if shift >= 8 { shift - 16 } else { shift };
    if write_mask == 0 || modifier > 1 {
        return Err(ShaderError::InvalidModifier);
    }
    if version.stage == Stage::Vertex && (modifier != 0 || shift != 0) {
        return Err(ShaderError::InvalidModifier);
    }
    if version.stage == Stage::Vertex
        && operation == Operation::Fraction
        && !matches!(write_mask, 2 | 3)
    {
        return Err(ShaderError::InvalidModifier);
    }
    if version.stage == Stage::Pixel {
        if !(-3..=3).contains(&shift) || (version.minor < 4 && !matches!(shift, -1 | 0 | 1 | 2)) {
            return Err(ShaderError::InvalidModifier);
        }
        if operation.is_texture() && (modifier != 0 || shift != 0) {
            return Err(ShaderError::InvalidModifier);
        }
        if version.minor < 4 && !matches!(write_mask, 7 | 8 | 15) {
            return Err(ShaderError::InvalidModifier);
        }
    }
    let valid = if operation == Operation::Define {
        register.file == Constant && write_mask == 15 && modifier == 0 && shift == 0
    } else if version.stage == Stage::Vertex {
        matches!(
            register.file,
            Temporary | RasterOutput | ColorOutput | TextureOutput
        ) || (register.file == Address && operation == Operation::Move && write_mask == 1)
    } else if operation.is_texture() {
        if version.minor == 4 {
            register.file == Temporary || (operation == Operation::Kill && register.file == Texture)
        } else {
            register.file == Texture
        }
    } else {
        register.file == Temporary || (version.minor < 4 && register.file == Texture)
    };
    if !valid {
        return Err(ShaderError::InvalidRegister);
    }
    if operation == Operation::TextureDepth && register.index != 5 {
        return Err(ShaderError::InvalidRegister);
    }
    Ok(Destination {
        register,
        write_mask,
        saturate: modifier == 1,
        shift,
    })
}

pub(super) fn source(
    token: u32,
    version: Version,
    operation: Operation,
    destination: Destination,
) -> Result<Source, ShaderError> {
    use RegisterFile::*;
    use SourceModifier::*;
    if token & 0x8000_0000 == 0 || token & 0x0000_d800 != 0 {
        return Err(ShaderError::InvalidToken);
    }
    let register = register(token, version)?;
    let swizzle = std::array::from_fn(|index| ((token >> (16 + index * 2)) & 3) as u8);
    let modifier = match (token >> 24) & 15 {
        0 => None,
        1 => Negate,
        2 => Bias,
        3 => BiasNegate,
        4 => Sign,
        5 => SignNegate,
        6 => Complement,
        7 => Double,
        8 => DoubleNegate,
        9 => DivideZ,
        10 => DivideW,
        _ => return Err(ShaderError::InvalidModifier),
    };
    let relative = token & 0x2000 != 0;
    if relative && (version.stage != Stage::Vertex || register.file != Constant) {
        return Err(ShaderError::InvalidRegister);
    }
    if version.stage == Stage::Vertex {
        if !matches!(register.file, Temporary | Input | Constant) {
            return Err(ShaderError::InvalidRegister);
        }
        if !matches!(modifier, None | Negate) {
            return Err(ShaderError::InvalidModifier);
        }
    } else {
        if !matches!(register.file, Temporary | Input | Constant | Texture) {
            return Err(ShaderError::InvalidRegister);
        }
        if version.minor == 4 && register.file == Constant && modifier != None {
            return Err(ShaderError::InvalidModifier);
        }
        if !operation.is_texture() && version.minor == 4 && register.file == Texture {
            return Err(ShaderError::InvalidRegister);
        }
        let coordinate_swizzle = version.minor == 4
            && operation.is_texture()
            && matches!(swizzle, [0, 1, 2, 2] | [0, 1, 3, 3]);
        if swizzle != [0, 1, 2, 3]
            && !coordinate_swizzle
            && !swizzle.iter().all(|component| *component == swizzle[0])
        {
            return Err(ShaderError::InvalidModifier);
        }
        if version.minor < 4
            && swizzle != [0, 1, 2, 3]
            && swizzle != [3; 4]
            && !(swizzle == [2; 4] && destination.write_mask == 8)
        {
            return Err(ShaderError::InvalidModifier);
        }
        if version.minor < 4 && matches!(modifier, Double | DoubleNegate | DivideZ | DivideW) {
            return Err(ShaderError::InvalidModifier);
        }
        if operation.is_texture() {
            if version.minor == 4 {
                let xyz = swizzle == [0, 1, 2, 3] || swizzle == [0, 1, 2, 2];
                let xyw = swizzle == [0, 1, 3, 3];
                let valid = match (operation, register.file, modifier) {
                    (Operation::Sample, Texture, None) => xyz || xyw,
                    (Operation::Sample, Texture, DivideW) => xyw,
                    (Operation::Sample, Temporary, None | DivideZ) => xyz,
                    (Operation::TextureCoordinate, Texture, None) => {
                        (xyz || xyw) && destination.write_mask == 7
                    }
                    (Operation::TextureCoordinate, Texture, DivideW) => {
                        xyw && destination.write_mask == 3
                    }
                    _ => false,
                };
                if !valid {
                    return Err(ShaderError::InvalidModifier);
                }
                if operation == Operation::Sample && destination.write_mask != 15 {
                    return Err(ShaderError::InvalidModifier);
                }
            }
            if matches!(operation, Operation::TextureAr | Operation::TextureGb) && modifier != None
            {
                return Err(ShaderError::InvalidModifier);
            }
            let allowed = if version.minor == 4 {
                matches!(modifier, None | DivideZ | DivideW)
                    && matches!(operation, Operation::Sample | Operation::TextureCoordinate)
            } else {
                modifier == None
                    || (matches!(modifier, Sign | SignNegate)
                        && (version.minor >= 2
                            || matches!(
                                operation,
                                Operation::Matrix3x2Pad
                                    | Operation::Matrix3x2Sample
                                    | Operation::Matrix3x3Pad
                                    | Operation::Matrix3x3Sample
                                    | Operation::Matrix3x3Specular
                                    | Operation::Matrix3x3VSpecular
                            )))
            };
            if !allowed {
                return Err(ShaderError::InvalidModifier);
            }
            if version.minor < 4
                && register.file != Texture
                && !(operation == Operation::Matrix3x3Specular && register.file == Constant)
            {
                return Err(ShaderError::InvalidRegister);
            }
            if version.minor == 4 && !matches!(register.file, Temporary | Texture) {
                return Err(ShaderError::InvalidRegister);
            }
        } else if matches!(modifier, DivideZ | DivideW) {
            return Err(ShaderError::InvalidModifier);
        }
    }
    Ok(Source {
        register,
        swizzle,
        modifier,
        relative,
    })
}
