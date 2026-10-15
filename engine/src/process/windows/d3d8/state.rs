use super::{
    D3D8Runtime32,
    memory::{read_words, write_words},
};
use crate::{
    graphics::d3d8::{
        api::HResult,
        state::{Light, Material, Matrix, Viewport},
    },
    memory::{AddressSpace, GuestAddress},
};

impl D3D8Runtime32 {
    pub(super) fn state_call(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        slot: u8,
        args: &[u32],
    ) -> Result<Option<u32>, HResult> {
        match slot {
            37 | 39 => {
                let matrix = Matrix(read_words(memory, args[2])?);
                let state = self
                    .device_mut(pointer)?
                    .state_mut()
                    .map_err(HResult::from)?;
                if slot == 37 {
                    state.set_transform(args[1], matrix)?;
                } else {
                    state.multiply_transform(args[1], matrix)?;
                }
            }
            38 => write_words(
                memory,
                args[2],
                &self.device(pointer)?.state().transform(args[1])?.0,
            )?,
            40 => {
                let [x, y, width, height, min_z, max_z] = read_words(memory, args[1])?;
                self.device_mut(pointer)?
                    .state_mut()
                    .map_err(HResult::from)?
                    .set_viewport(Viewport {
                        x,
                        y,
                        width,
                        height,
                        min_z,
                        max_z,
                    })?;
            }
            41 => {
                let viewport = self.device(pointer)?.state().snapshot().viewport;
                write_words(
                    memory,
                    args[1],
                    &[
                        viewport.x,
                        viewport.y,
                        viewport.width,
                        viewport.height,
                        viewport.min_z,
                        viewport.max_z,
                    ],
                )?;
            }
            42 => {
                let words: [u32; 17] = read_words(memory, args[1])?;
                let material = Material {
                    diffuse: words[0..4].try_into().unwrap(),
                    ambient: words[4..8].try_into().unwrap(),
                    specular: words[8..12].try_into().unwrap(),
                    emissive: words[12..16].try_into().unwrap(),
                    power: words[16],
                };
                self.device_mut(pointer)?
                    .state_mut()
                    .map_err(HResult::from)?
                    .set_material(material)?;
            }
            43 => {
                let material = self.device(pointer)?.state().snapshot().material;
                let mut words = [0; 17];
                words[..4].copy_from_slice(&material.diffuse);
                words[4..8].copy_from_slice(&material.ambient);
                words[8..12].copy_from_slice(&material.specular);
                words[12..16].copy_from_slice(&material.emissive);
                words[16] = material.power;
                write_words(memory, args[1], &words)?;
            }
            44 => {
                let words: [u32; 26] = read_words(memory, args[2])?;
                let light = Light {
                    kind: words[0],
                    diffuse: words[1..5].try_into().unwrap(),
                    specular: words[5..9].try_into().unwrap(),
                    ambient: words[9..13].try_into().unwrap(),
                    position: words[13..16].try_into().unwrap(),
                    direction: words[16..19].try_into().unwrap(),
                    range: words[19],
                    falloff: words[20],
                    attenuation: words[21..24].try_into().unwrap(),
                    theta: words[24],
                    phi: words[25],
                };
                self.device_mut(pointer)?
                    .state_mut()
                    .map_err(HResult::from)?
                    .set_light(args[1], light)?;
            }
            45 => {
                let light = self.device(pointer)?.state().light(args[1])?;
                let mut words = [0; 26];
                words[0] = light.kind;
                words[1..5].copy_from_slice(&light.diffuse);
                words[5..9].copy_from_slice(&light.specular);
                words[9..13].copy_from_slice(&light.ambient);
                words[13..16].copy_from_slice(&light.position);
                words[16..19].copy_from_slice(&light.direction);
                words[19] = light.range;
                words[20] = light.falloff;
                words[21..24].copy_from_slice(&light.attenuation);
                words[24] = light.theta;
                words[25] = light.phi;
                write_words(memory, args[2], &words)?;
            }
            46 => self
                .device_mut(pointer)?
                .state_mut()
                .map_err(HResult::from)?
                .enable_light(args[1], args[2] != 0)?,
            47 => {
                let enabled = self.device(pointer)?.state().light_enabled(args[1])?;
                write_words(memory, args[2], &[if enabled { 128 } else { 0 }])?;
            }
            48 => {
                let plane = read_words(memory, args[2])?;
                self.device_mut(pointer)?
                    .state_mut()
                    .map_err(HResult::from)?
                    .set_clip_plane(args[1], plane)?;
            }
            49 => {
                let plane = *self
                    .device(pointer)?
                    .state()
                    .snapshot()
                    .clip_planes
                    .get(args[1] as usize)
                    .ok_or(HResult::INVALID_CALL)?;
                write_words(memory, args[2], &plane)?;
            }
            50 => self
                .device_mut(pointer)?
                .state_mut()
                .map_err(HResult::from)?
                .set_render_state(args[1], args[2])?,
            51 => write_words(
                memory,
                args[2],
                &[self.device(pointer)?.state().render_state(args[1])?],
            )?,
            58 => {
                let status = read_words(memory, args[1])?;
                self.device_mut(pointer)?
                    .state_mut()
                    .map_err(HResult::from)?
                    .set_clip_status(status)?;
            }
            59 => write_words(
                memory,
                args[1],
                &self.device(pointer)?.state().snapshot().clip_status,
            )?,
            62 => write_words(
                memory,
                args[3],
                &[self
                    .device(pointer)?
                    .state()
                    .texture_stage_state(args[1], args[2])?],
            )?,
            63 => self
                .device_mut(pointer)?
                .state_mut()
                .map_err(HResult::from)?
                .set_texture_stage_state(args[1], args[2], args[3])?,
            68 => self
                .device_mut(pointer)?
                .state_mut()
                .map_err(HResult::from)?
                .set_palette(args[1])?,
            69 => write_words(
                memory,
                args[1],
                &[self.device(pointer)?.state().snapshot().palette],
            )?,
            77 => write_words(
                memory,
                args[1],
                &[self.device(pointer)?.state().snapshot().vertex_shader],
            )?,
            89 => write_words(
                memory,
                args[1],
                &[self.device(pointer)?.state().snapshot().pixel_shader],
            )?,
            79 | 91 => {
                let maximum = if slot == 79 { 96 } else { 8 };
                let constants = read_constants(memory, args[2], args[1], args[3], maximum)?;
                let state = self
                    .device_mut(pointer)?
                    .state_mut()
                    .map_err(HResult::from)?;
                let constants = &constants[..args[3] as usize];
                if slot == 79 {
                    state.set_vertex_constants(args[1], constants)?;
                } else {
                    state.set_pixel_constants(args[1], constants)?;
                }
            }
            80 | 92 => {
                let constants = if slot == 80 {
                    &self.device(pointer)?.state().snapshot().vertex_constants[..]
                } else {
                    &self.device(pointer)?.state().snapshot().pixel_constants[..]
                };
                let end = args[1].checked_add(args[3]).ok_or(HResult::INVALID_CALL)? as usize;
                let selected = constants
                    .get(args[1] as usize..end)
                    .ok_or(HResult::INVALID_CALL)?;
                let mut words = [0; 96 * 4];
                for (output, input) in words.iter_mut().zip(selected.iter().flatten()) {
                    *output = *input;
                }
                write_words(memory, args[2], &words[..selected.len() * 4])?;
            }
            _ => return Ok(None),
        }
        Ok(Some(0))
    }
}

fn read_constants(
    memory: &AddressSpace,
    pointer: u32,
    start: u32,
    count: u32,
    maximum: u32,
) -> Result<[[u32; 4]; 96], HResult> {
    if start.checked_add(count).is_none_or(|end| end > maximum) {
        return Err(HResult::INVALID_CALL);
    }
    let mut constants = [[0; 4]; 96];
    if u64::from(pointer) + u64::from(count) * 16 > 1 << 32 {
        return Err(HResult::INVALID_CALL);
    }
    for index in 0..count {
        constants[index as usize] = read_words(memory, pointer + index * 16)?;
    }
    Ok(constants)
}
