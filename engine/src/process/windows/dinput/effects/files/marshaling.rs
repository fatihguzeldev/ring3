use super::super::super::{
    DirectInputRuntime32,
    memory::{bytes, put},
};
use super::{MAX_ENTRIES, MAX_FILE_BYTES};
use crate::{
    input::dinput::{
        DirectInputResult32 as H, InterfaceKind32,
        haptics::{
            EffectKind32,
            files::{FileEffect32, decode},
        },
    },
    memory::{AddressSpace, GuestAddress},
    process::WindowsProviderOutcome32,
};

impl DirectInputRuntime32 {
    pub(in crate::process::windows::dinput) fn enumerate_effect_file(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        interface: InterfaceKind32,
        args: &[u32],
        input: &[u8],
    ) -> Result<WindowsProviderOutcome32, H> {
        let mut effects = decode(input, MAX_ENTRIES, MAX_FILE_BYTES)?;
        let axes = self.device(pointer)?.native.description.axes as usize;
        let mut rows = Vec::new();
        let mut fixups = Vec::new();
        rows.try_reserve_exact(effects.len())
            .map_err(|_| H::OUT_OF_MEMORY)?;
        fixups
            .try_reserve_exact(effects.len())
            .map_err(|_| H::OUT_OF_MEMORY)?;
        for effect in &mut effects {
            if args[4] & 1 == 0 && !effect.standard() {
                continue;
            }
            if args[4] & 0x10 != 0 && effect.axes.len() > axes && axes != 0 {
                fit_axes(effect, axes)?;
            }
            let (row, relocations) = guest_row(effect)?;
            rows.push(row);
            fixups.push(relocations);
        }
        self.start_relocated_enumeration(memory, args[2], args[3], interface, rows, Some(fixups))
    }
}
fn guest_row(effect: &FileEffect32) -> Result<(Vec<u8>, Vec<usize>), H> {
    let axes = 340;
    let direction = axes + effect.axes.len() * 4;
    let envelope = direction + effect.direction.len() * 4;
    let payload = envelope + usize::from(effect.envelope.is_some()) * 20;
    let custom = payload + effect.type_specific.len();
    let length = custom;
    let mut row = bytes(length)?;
    let mut fixups = Vec::new();
    fixups.try_reserve_exact(6).map_err(|_| H::OUT_OF_MEMORY)?;
    put(&mut row, 0, 284);
    row[4..20].copy_from_slice(&effect.guid);
    put(&mut row, 20, 284);
    fixups.push(20);
    row[24..24 + effect.name.len()].copy_from_slice(&effect.name);
    for (index, value) in [
        56,
        effect.flags,
        effect.duration,
        effect.sample_period,
        effect.gain,
        effect.trigger,
        effect.trigger_repeat,
        effect.axes.len() as u32,
        axes as u32,
        direction as u32,
        if effect.envelope.is_some() {
            envelope as u32
        } else {
            0
        },
        effect.type_specific.len() as u32,
        if effect.type_specific.is_empty() {
            0
        } else {
            payload as u32
        },
        effect.start_delay,
    ]
    .into_iter()
    .enumerate()
    {
        put(&mut row, 284 + index * 4, value)
    }
    fixups.extend([284 + 32, 284 + 36]);
    for (index, value) in effect.axes.iter().enumerate() {
        put(&mut row, axes + index * 4, *value)
    }
    for (index, value) in effect.direction.iter().enumerate() {
        put(&mut row, direction + index * 4, *value as u32)
    }
    if let Some(value) = effect.envelope {
        fixups.push(284 + 40);
        for (index, value) in [
            20,
            value.attack_level,
            value.attack_time,
            value.fade_level,
            value.fade_time,
        ]
        .into_iter()
        .enumerate()
        {
            put(&mut row, envelope + index * 4, value)
        }
    }
    if !effect.type_specific.is_empty() {
        row[payload..custom].copy_from_slice(&effect.type_specific);
        fixups.push(284 + 48)
    }
    Ok((row, fixups))
}

fn fit_axes(effect: &mut FileEffect32, axes: usize) -> Result<(), H> {
    let original = effect.axes.len();
    if effect.flags & 0x70 != 0x10 {
        let mut parameters =
            crate::input::dinput::haptics::EffectParameters32::empty(EffectKind32::Constant);
        if original > 8 {
            return Err(H::INVALID_PARAM);
        }
        parameters.axis_count = original as u8;
        parameters.coordinates = if effect.flags & 0x70 == 0x20 {
            crate::input::dinput::haptics::Coordinates32::Polar
        } else {
            crate::input::dinput::haptics::Coordinates32::Spherical
        };
        parameters.direction[..original].copy_from_slice(&effect.direction);
        let direction = parameters
            .converted_direction(crate::input::dinput::haptics::Coordinates32::Cartesian)?;
        effect.direction.copy_from_slice(&direction[..original]);
        effect.flags = effect.flags & !0x70 | 0x10;
    }
    effect.axes.truncate(axes);
    effect.direction.truncate(axes);
    if EffectKind32::from_guid(effect.guid).is_some_and(|kind| kind.family() == 4)
        && effect.type_specific.len() == original * 24
    {
        effect.type_specific.truncate(axes * 24);
    }
    effect.validate()
}
