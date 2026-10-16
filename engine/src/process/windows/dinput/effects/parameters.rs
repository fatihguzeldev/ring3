use super::super::{
    DirectInputRuntime32,
    memory::{bytes, output, put, words},
};
use crate::{
    input::dinput::{
        DirectInputResult32 as H,
        format::DeviceObject32,
        haptics::{Coordinates32, EffectPayload32},
    },
    memory::{AddressSpace, GuestAddress},
};

impl DirectInputRuntime32 {
    pub(in crate::process::windows::dinput) fn get_effect_parameters(
        &self,
        memory: &mut AddressSpace,
        device: GuestAddress,
        id: u64,
        address: u32,
        mask: u32,
    ) -> Result<u32, H> {
        if mask & !0x3ff != 0 {
            return Err(H::INVALID_PARAM);
        }
        let [size] = words(memory, address)?;
        if !matches!(size, 52 | 56) || (size == 52 && mask & 512 != 0) {
            return Err(H::INVALID_PARAM);
        }
        let mut header = bytes(size as usize)?;
        memory.read(GuestAddress(address), &mut header)?;
        let mut values = [0u32; 14];
        for (value, chunk) in values.iter_mut().zip(header.chunks_exact(4)) {
            *value = u32::from_le_bytes(chunk.try_into().unwrap());
        }
        let force = self.force_device(device)?;
        let parameters = force.parameters(id)?;
        if mask & crate::input::dinput::haptics::REQUIRED_PARAMETERS & force.initialized(id)?
            != mask & crate::input::dinput::haptics::REQUIRED_PARAMETERS
        {
            return Err(H::INCOMPLETE_EFFECT);
        }
        let how = match values[1] & 3 {
            1 => 2,
            2 => 1,
            _ => 0,
        };
        if mask & (8 | 32) != 0 && how == 0 {
            return Err(H::INVALID_PARAM);
        }
        let coordinate = if values[1] & parameters.coordinates.flag() != 0 {
            parameters.coordinates
        } else if values[1] & 0x10 != 0 {
            Coordinates32::Cartesian
        } else if values[1] & 0x20 != 0 {
            Coordinates32::Polar
        } else if values[1] & 0x40 != 0 {
            Coordinates32::Spherical
        } else {
            parameters.coordinates
        };
        if mask & 64 != 0 && values[1] & 0x70 == 0 {
            return Err(H::INVALID_PARAM);
        }
        let payload_length = match &parameters.payload {
            EffectPayload32::Constant(_) => 4,
            EffectPayload32::Ramp(..) => 8,
            EffectPayload32::Periodic { .. } | EffectPayload32::Custom { .. } => 16,
            EffectPayload32::Conditions { count, .. } => u32::from(*count) * 24,
        };
        let more_axes = mask & (32 | 64) != 0 && values[7] < u32::from(parameters.axis_count);
        let more_payload = mask & 256 != 0 && values[11] < payload_length;
        if more_axes || more_payload {
            if more_axes {
                put(&mut header, 28, u32::from(parameters.axis_count));
            }
            if more_payload {
                put(&mut header, 44, payload_length);
            }
            output(memory, address, header.len())?;
            memory.write(GuestAddress(address), &header)?;
            return Err(H::MORE_DATA);
        }
        let mut outputs: Vec<(GuestAddress, Vec<u8>)> = Vec::new();
        outputs.try_reserve_exact(6).map_err(|_| H::OUT_OF_MEMORY)?;
        if mask & 1 != 0 {
            put(&mut header, 8, parameters.duration);
        }
        if mask & 2 != 0 {
            put(&mut header, 12, parameters.sample_period);
        }
        if mask & 4 != 0 {
            put(&mut header, 16, parameters.gain);
        }
        if mask & 8 != 0 {
            put(
                &mut header,
                20,
                parameters
                    .trigger
                    .map(|button| {
                        self.effect_object_identifier(
                            device,
                            DeviceObject32::Button(u16::from(button)),
                            how,
                        )
                    })
                    .transpose()?
                    .unwrap_or(u32::MAX),
            );
        }
        if mask & 16 != 0 {
            put(&mut header, 24, parameters.trigger_repeat);
        }
        if mask & (32 | 64) != 0 {
            put(&mut header, 28, u32::from(parameters.axis_count));
        }
        if mask & 32 != 0 {
            let mut axes = bytes(parameters.axis_count as usize * 4)?;
            for (index, axis) in parameters.axes[..parameters.axis_count as usize]
                .iter()
                .enumerate()
            {
                put(
                    &mut axes,
                    index * 4,
                    self.effect_object_identifier(
                        device,
                        DeviceObject32::Axis {
                            index: *axis,
                            aspect: 0,
                        },
                        how,
                    )?,
                );
            }
            outputs.push((GuestAddress(values[8]), axes));
        }
        if mask & 64 != 0 {
            put(&mut header, 4, values[1] & !0x70 | coordinate.flag());
            let direction = parameters.converted_direction(coordinate)?;
            let mut encoded = bytes(parameters.axis_count as usize * 4)?;
            for (index, value) in direction[..parameters.axis_count as usize]
                .iter()
                .enumerate()
            {
                put(&mut encoded, index * 4, *value as u32);
            }
            outputs.push((GuestAddress(values[9]), encoded));
        }
        if mask & 128 != 0 {
            if let Some(envelope) = parameters.envelope {
                let mut encoded = bytes(20)?;
                for (index, value) in [
                    20,
                    envelope.attack_level,
                    envelope.attack_time,
                    envelope.fade_level,
                    envelope.fade_time,
                ]
                .into_iter()
                .enumerate()
                {
                    put(&mut encoded, index * 4, value);
                }
                outputs.push((GuestAddress(values[10]), encoded));
            } else {
                put(&mut header, 40, 0);
            }
        }
        if mask & 256 != 0 {
            let mut encoded = bytes(payload_length as usize)?;
            match &parameters.payload {
                EffectPayload32::Constant(value) => put(&mut encoded, 0, *value as u32),
                EffectPayload32::Ramp(first, last) => {
                    put(&mut encoded, 0, *first as u32);
                    put(&mut encoded, 4, *last as u32);
                }
                EffectPayload32::Periodic {
                    magnitude,
                    offset,
                    phase,
                    period,
                } => {
                    for (index, value) in [*magnitude, *offset as u32, *phase, *period]
                        .into_iter()
                        .enumerate()
                    {
                        put(&mut encoded, index * 4, value);
                    }
                }
                EffectPayload32::Conditions {
                    count,
                    values: conditions,
                } => {
                    for (index, condition) in conditions[..*count as usize].iter().enumerate() {
                        for (member, value) in [
                            condition.offset as u32,
                            condition.positive_coefficient as u32,
                            condition.negative_coefficient as u32,
                            condition.positive_saturation,
                            condition.negative_saturation,
                            condition.deadband as u32,
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            put(&mut encoded, index * 24 + member * 4, value);
                        }
                    }
                }
                EffectPayload32::Custom {
                    channels,
                    period,
                    samples,
                } => {
                    let supplied = words::<4>(memory, values[12])?;
                    if supplied[2] < samples.len() as u32 {
                        let mut needed = bytes(16)?;
                        for (index, value) in [
                            u32::from(*channels),
                            *period,
                            samples.len() as u32,
                            supplied[3],
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            put(&mut needed, index * 4, value);
                        }
                        output(memory, values[12], 16)?;
                        memory.write(GuestAddress(values[12]), &needed)?;
                        return Err(H::MORE_DATA);
                    }
                    for (index, value) in [
                        u32::from(*channels),
                        *period,
                        samples.len() as u32,
                        supplied[3],
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        put(&mut encoded, index * 4, value);
                    }
                    let mut sample_bytes = bytes(samples.len() * 4)?;
                    for (index, value) in samples.iter().enumerate() {
                        put(&mut sample_bytes, index * 4, *value as u32);
                    }
                    outputs.push((GuestAddress(supplied[3]), sample_bytes));
                }
            }
            put(&mut header, 44, payload_length);
            outputs.push((GuestAddress(values[12]), encoded));
        }
        if mask & 512 != 0 {
            put(&mut header, 52, parameters.start_delay);
        }
        outputs.push((GuestAddress(address), header));
        let mut spans = [(GuestAddress(0), &[][..]); 6];
        for (span, (address, bytes)) in spans.iter_mut().zip(&outputs) {
            output(memory, address.0, bytes.len())?;
            *span = (*address, bytes);
        }
        memory.write_scatter(&spans[..outputs.len()])?;
        Ok(0)
    }

    fn effect_object_identifier(
        &self,
        device: GuestAddress,
        object: DeviceObject32,
        how: u32,
    ) -> Result<u32, H> {
        if how == 2 {
            return Ok(object.object_type(false));
        }
        self.device(device)?
            .native
            .format()
            .and_then(|format| {
                format
                    .bindings()
                    .iter()
                    .find(|binding| binding.object == Some(object))
            })
            .map(|binding| binding.offset)
            .ok_or(H::OBJECT_NOT_FOUND)
    }
}
