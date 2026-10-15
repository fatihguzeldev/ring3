use super::{
    D3D8Runtime32, DispatchError32,
    memory::{check_output, read_words, write_words},
};
use crate::{
    graphics::d3d8::{
        api::HResult,
        shader::{Declaration, MAX_DECLARATION_TOKENS, MAX_SHADER_TOKENS, Program, Stage},
    },
    memory::{AddressSpace, GuestAddress},
};

impl D3D8Runtime32 {
    pub(super) fn shader_call(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        slot: u8,
        args: &[u32],
    ) -> Result<Option<u32>, DispatchError32> {
        match slot {
            75 => {
                check_output(memory, args[3], 4)?;
                let declaration = read_tokens(memory, args[1], TokenKind::Declaration)?;
                let function = if args[2] == 0 {
                    None
                } else {
                    Some(read_tokens(memory, args[2], TokenKind::Vertex)?)
                };
                let prepared = self.device_mut(pointer)?.prepare_vertex_shader(
                    &declaration,
                    function.as_deref(),
                    args[4],
                )?;
                self.device(pointer)?.can_commit_shader(&prepared)?;
                write_words(memory, args[3], &[prepared.handle()])?;
                self.device_mut(pointer)?
                    .commit_shader(prepared)
                    .expect("exclusive device preparation remains current after guest output");
            }
            76 => self.device_mut(pointer)?.bind_vertex_shader(args[1])?,
            78 => self.device_mut(pointer)?.delete_vertex_shader(args[1])?,
            81 | 82 | 93 => {
                let shader = self.device(pointer)?.shaders();
                let words = match slot {
                    81 => shader.vertex(args[1])?.declaration.tokens(),
                    82 => shader
                        .vertex(args[1])?
                        .function
                        .as_ref()
                        .map_or(&[][..], Program::tokens),
                    _ => shader.pixel(args[1])?.function.tokens(),
                };
                get_tokens(memory, args[2], args[3], words)?;
            }
            87 => {
                check_output(memory, args[2], 4)?;
                let function = read_tokens(memory, args[1], TokenKind::Pixel)?;
                let prepared = self.device_mut(pointer)?.prepare_pixel_shader(&function)?;
                self.device(pointer)?.can_commit_shader(&prepared)?;
                write_words(memory, args[2], &[prepared.handle()])?;
                self.device_mut(pointer)?
                    .commit_shader(prepared)
                    .expect("exclusive device preparation remains current after guest output");
            }
            88 => self.device_mut(pointer)?.bind_pixel_shader(args[1])?,
            90 => self.device_mut(pointer)?.delete_pixel_shader(args[1])?,
            _ => return Ok(None),
        }
        Ok(Some(0))
    }
}

enum TokenKind {
    Declaration,
    Vertex,
    Pixel,
}

fn read_tokens(memory: &AddressSpace, pointer: u32, kind: TokenKind) -> Result<Vec<u32>, HResult> {
    let maximum = if matches!(kind, TokenKind::Declaration) {
        MAX_DECLARATION_TOKENS
    } else {
        MAX_SHADER_TOKENS
    };
    let mut words = Vec::new();
    words
        .try_reserve(maximum.min(256))
        .map_err(|_| HResult::OUT_OF_MEMORY)?;
    for index in 0..maximum {
        let address = pointer
            .checked_add(index as u32 * 4)
            .ok_or(HResult::INVALID_CALL)?;
        let [token] = read_words(memory, address)?;
        words.try_reserve(1).map_err(|_| HResult::OUT_OF_MEMORY)?;
        words.push(token);
        let is_end = if matches!(kind, TokenKind::Declaration) {
            token == u32::MAX
        } else {
            token == 0xffff
        };
        if is_end {
            let parsed = match kind {
                TokenKind::Declaration => Declaration::parse(&words).map(|_| ()),
                TokenKind::Vertex => Program::parse(&words, Stage::Vertex).map(|_| ()),
                TokenKind::Pixel => Program::parse(&words, Stage::Pixel).map(|_| ()),
            };
            match parsed {
                Ok(()) => return Ok(words),
                Err(crate::graphics::d3d8::shader::ShaderError::Truncated) => {}
                Err(_) => return Err(HResult::INVALID_CALL),
            }
        }
    }
    Err(HResult::INVALID_CALL)
}

fn get_tokens(
    memory: &mut AddressSpace,
    data: u32,
    size: u32,
    words: &[u32],
) -> Result<(), HResult> {
    let required = (words.len() * 4) as u32;
    check_output(memory, size, 4)?;
    if data == 0 {
        return write_words(memory, size, &[required]);
    }
    let [capacity] = read_words(memory, size)?;
    if capacity < required {
        return Err(HResult::INVALID_CALL);
    }
    check_output(memory, data, required as usize)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(required as usize)
        .map_err(|_| HResult::OUT_OF_MEMORY)?;
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    memory.write_spans(&[
        (GuestAddress(data), &bytes),
        (GuestAddress(size), &required.to_le_bytes()),
    ])?;
    Ok(())
}
