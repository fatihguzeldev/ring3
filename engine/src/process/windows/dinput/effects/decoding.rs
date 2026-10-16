use super::super::{
    DirectInputRuntime32,
    memory::{read, words},
};
use crate::{
    input::dinput::{
        DirectInputResult32 as H,
        format::DeviceObject32,
        haptics::{
            Condition32, Coordinates32, EffectEnvelope32, EffectKind32, EffectParameters32,
            EffectPayload32,
        },
    },
    memory::{AddressSpace, GuestAddress},
};
use std::sync::Arc;

impl DirectInputRuntime32 {
    pub(in crate::process::windows::dinput) fn read_effect_parameters(
        &self,
        memory: &AddressSpace,
        device: GuestAddress,
        kind: EffectKind32,
        address: u32,
        flags: u32,
        base: Option<&EffectParameters32>,
    ) -> Result<EffectParameters32, H> {
        let [size] = words(memory, address)?;
        if !matches!(size, 52 | 56) || flags & !(0xe00003ff) != 0 {
            return Err(H::INVALID_PARAM);
        }
        let mut header = [0; 14];
        for (index, value) in header[..size as usize / 4].iter_mut().enumerate() {
            *value = u32::from_le_bytes(read(
                memory,
                address
                    .checked_add(index as u32 * 4)
                    .ok_or(H::INVALID_PARAM)?,
            )?);
        }
        if size == 52 && flags & 512 != 0 {
            return Err(H::INVALID_PARAM);
        }
        let mut next = base
            .cloned()
            .unwrap_or_else(|| EffectParameters32::empty(kind));
        if flags & 1 != 0 {
            next.duration = header[2];
        }
        if flags & 2 != 0 {
            next.sample_period = header[3];
        }
        if flags & 4 != 0 {
            next.gain = header[4];
        }
        if flags & 16 != 0 {
            next.trigger_repeat = header[6];
        }
        if flags & 512 != 0 {
            next.start_delay = header[13];
        }
        let how = match header[1] & 3 {
            1 => 2,
            2 => 1,
            _ => 0,
        };
        if flags & (8 | 32) != 0 && how == 0 {
            return Err(H::INVALID_PARAM);
        }
        if flags & 8 != 0 {
            next.trigger = if header[5] == u32::MAX {
                None
            } else {
                match self.resolve_object(device, header[5], how)? {
                    DeviceObject32::Button(index) if index < 128 => Some(index as u8),
                    _ => return Err(H::OBJECT_NOT_FOUND),
                }
            };
        }
        if flags & (32 | 64) != 0 {
            if !(1..=8).contains(&header[7]) {
                return Err(H::INVALID_PARAM);
            }
            if flags & 32 != 0 {
                next.axis_count = header[7] as u8;
                next.axes = [0; 8];
                for index in 0..header[7] as usize {
                    let value = u32::from_le_bytes(read(
                        memory,
                        header[8]
                            .checked_add(index as u32 * 4)
                            .ok_or(H::INVALID_PARAM)?,
                    )?);
                    next.axes[index] = match self.resolve_object(device, value, how)? {
                        DeviceObject32::Axis { index, aspect: 0 } => index,
                        _ => return Err(H::OBJECT_NOT_FOUND),
                    };
                }
            }
            if flags & 64 != 0 {
                if next.axis_count != header[7] as u8 {
                    return Err(H::INVALID_PARAM);
                }
                next.coordinates = match header[1] & 0x70 {
                    0x10 => Coordinates32::Cartesian,
                    0x20 => Coordinates32::Polar,
                    0x40 => Coordinates32::Spherical,
                    _ => return Err(H::INVALID_PARAM),
                };
                next.direction = [0; 8];
                for (index, value) in next.direction[..next.axis_count as usize]
                    .iter_mut()
                    .enumerate()
                {
                    *value = u32::from_le_bytes(read(
                        memory,
                        header[9]
                            .checked_add(index as u32 * 4)
                            .ok_or(H::INVALID_PARAM)?,
                    )?) as i32;
                }
            }
        }
        if flags & 128 != 0 {
            next.envelope = if header[10] == 0 {
                None
            } else {
                let values = words::<5>(memory, header[10])?;
                if values[0] != 20 {
                    return Err(H::INVALID_PARAM);
                }
                Some(EffectEnvelope32 {
                    attack_level: values[1],
                    attack_time: values[2],
                    fade_level: values[3],
                    fade_time: values[4],
                })
            };
        }
        if flags & 256 != 0 {
            next.payload = payload(memory, kind, header[12], header[11], next.axis_count)?;
        }
        Ok(next)
    }
}

fn payload(
    memory: &AddressSpace,
    kind: EffectKind32,
    address: u32,
    length: u32,
    axes: u8,
) -> Result<EffectPayload32, H> {
    let expected = match kind.family() {
        1 => 4,
        2 => 8,
        3 | 5 => 16,
        4 => length,
        _ => return Err(H::INVALID_PARAM),
    };
    if length != expected
        || (kind.family() == 4
            && (length == 0
                || !length.is_multiple_of(24)
                || length / 24 > 8
                || (length / 24 != 1 && length / 24 != u32::from(axes))))
    {
        return Err(H::INVALID_PARAM);
    }
    match kind.family() {
        1 => Ok(EffectPayload32::Constant(
            words::<1>(memory, address)?[0] as i32,
        )),
        2 => {
            let values = words::<2>(memory, address)?;
            Ok(EffectPayload32::Ramp(values[0] as i32, values[1] as i32))
        }
        3 => {
            let values = words::<4>(memory, address)?;
            Ok(EffectPayload32::Periodic {
                magnitude: values[0],
                offset: values[1] as i32,
                phase: values[2],
                period: values[3],
            })
        }
        4 => {
            let count = (length / 24) as u8;
            let mut values = [Condition32::default(); 8];
            for (index, value) in values[..count as usize].iter_mut().enumerate() {
                let words = words::<6>(
                    memory,
                    address
                        .checked_add(index as u32 * 24)
                        .ok_or(H::INVALID_PARAM)?,
                )?;
                *value = Condition32 {
                    offset: words[0] as i32,
                    positive_coefficient: words[1] as i32,
                    negative_coefficient: words[2] as i32,
                    positive_saturation: words[3],
                    negative_saturation: words[4],
                    deadband: words[5] as i32,
                };
            }
            Ok(EffectPayload32::Conditions { count, values })
        }
        5 => {
            let values = words::<4>(memory, address)?;
            if values[0] == 0
                || values[0] > u32::from(axes)
                || values[2] == 0
                || values[2] > 32768
                || !values[2].is_multiple_of(values[0])
            {
                return Err(H::INVALID_PARAM);
            }
            memory.check_span(
                GuestAddress(values[3]),
                values[2] as usize * 4,
                crate::memory::Access::Read,
            )?;
            let mut samples = Vec::new();
            samples
                .try_reserve_exact(values[2] as usize)
                .map_err(|_| H::OUT_OF_MEMORY)?;
            for index in 0..values[2] {
                samples.push(u32::from_le_bytes(read(
                    memory,
                    values[3].checked_add(index * 4).ok_or(H::INVALID_PARAM)?,
                )?) as i32);
            }
            Ok(EffectPayload32::Custom {
                channels: values[0] as u8,
                period: values[1],
                samples: Arc::from(samples),
            })
        }
        _ => unreachable!(),
    }
}
