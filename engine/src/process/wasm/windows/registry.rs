use crate::{
    abi::{
        arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
        header,
        registry::{CHANGE_SIZE, encode_change},
    },
    process::{HostError, wasm::mutate},
    windows::registry::{RegistryChangeId32, RegistryConfig32, RegistryLimits32},
};
fn id(el: u32, eh: u32, sl: u32, sh: u32, rl: u32, rh: u32) -> RegistryChangeId32 {
    RegistryChangeId32 {
        epoch: super::wide(el, eh),
        sequence: super::wide(sl, sh),
        revision: super::wide(rl, rh),
    }
}
pub(crate) fn configure_registry(
    keys: u32,
    values: u32,
    handles: u32,
    bytes: u32,
    changes: u32,
    separate_views: u32,
) -> u32 {
    mutate(|instance| {
        if separate_views > 1 {
            return Err(HostError::InvalidRequest);
        }
        instance.configure_registry(RegistryConfig32 {
            limits: RegistryLimits32 {
                keys,
                values,
                handles,
                bytes,
                changes,
            },
            separate_views: separate_views != 0,
        })
    })
}
pub(crate) fn register_advapi32_module(base: u32) -> u32 {
    mutate(|instance| instance.register_advapi32_module(base))
}
pub(crate) fn registry_change(index: u32) -> u32 {
    mutate(|instance| {
        let mut bytes = [0; CHANGE_SIZE];
        encode_change(instance.registry_change(index)?, &mut bytes);
        instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + CHANGE_SIZE]
            .copy_from_slice(&bytes);
        Ok(())
    })
}
pub(crate) fn registry_read_change_text(
    el: u32,
    eh: u32,
    sl: u32,
    sh: u32,
    rl: u32,
    rh: u32,
    part: u32,
    offset: u32,
) -> u32 {
    mutate(|instance| {
        let mut units = [0; (TRANSFER_SIZE - 16) / 2];
        let count = instance.registry_read_change_text(
            id(el, eh, sl, sh, rl, rh),
            part,
            offset,
            &mut units,
        )?;
        let output = &mut instance.arena.as_mut().get_mut()
            [TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE];
        output.fill(0);
        header::write(&mut output[..16 + count * 2], *b"R3RT");
        header::write_u32(output, 12, count as u32);
        for (index, unit) in units[..count].iter().enumerate() {
            output[16 + index * 2..18 + index * 2].copy_from_slice(&unit.to_le_bytes());
        }
        Ok(())
    })
}
pub(crate) fn registry_read_change_data(
    el: u32,
    eh: u32,
    sl: u32,
    sh: u32,
    rl: u32,
    rh: u32,
    offset: u32,
) -> u32 {
    mutate(|instance| {
        let mut bytes = [0; TRANSFER_SIZE];
        let count =
            instance.registry_read_change(id(el, eh, sl, sh, rl, rh), offset, &mut bytes[16..])?;
        header::write(&mut bytes[..16], *b"R3RT");
        header::write_u32(&mut bytes, 8, (16 + count) as u32);
        header::write_u32(&mut bytes, 12, count as u32);
        instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE]
            .copy_from_slice(&bytes);
        Ok(())
    })
}
pub(crate) fn acknowledge_registry_change(
    el: u32,
    eh: u32,
    sl: u32,
    sh: u32,
    rl: u32,
    rh: u32,
) -> u32 {
    mutate(|instance| instance.registry_acknowledge_change(id(el, eh, sl, sh, rl, rh)))
}
pub(crate) fn pending_registry_call() -> u32 {
    mutate(|instance| {
        let pending = instance.pending_registry_call();
        let output = &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 40];
        header::write(output, *b"R3RW");
        if let Some(pending) = pending {
            header::write_u32(output, 12, 1);
            output[16..24].copy_from_slice(&pending.epoch.to_le_bytes());
            header::write_u32(output, 24, pending.token);
            header::write_u32(output, 28, u32::from(pending.ready));
        }
        Ok(())
    })
}
pub(crate) fn resume_registry_call(low: u32, high: u32, token: u32) -> u32 {
    mutate(|instance| instance.resume_registry_call(super::wide(low, high), token))
}

pub(crate) fn begin_registry_seed(total: u32) -> u32 {
    mutate(|instance| {
        let value = instance.begin_registry_seed(total)?;
        seed_status(instance, value);
        Ok(())
    })
}
pub(crate) fn append_registry_seed(
    el: u32,
    eh: u32,
    tl: u32,
    th: u32,
    offset: u32,
    length: u32,
) -> u32 {
    mutate(|instance| {
        let bytes = super::input(instance, length)?.to_vec();
        let value = instance.append_registry_seed(
            super::wide(el, eh),
            super::wide(tl, th),
            offset,
            &bytes,
        )?;
        seed_status(instance, value);
        Ok(())
    })
}
pub(crate) fn finish_registry_seed(el: u32, eh: u32, tl: u32, th: u32) -> u32 {
    mutate(|instance| instance.finish_registry_seed(super::wide(el, eh), super::wide(tl, th)))
}
pub(crate) fn abort_registry_seed(el: u32, eh: u32, tl: u32, th: u32) -> u32 {
    mutate(|instance| instance.abort_registry_seed(super::wide(el, eh), super::wide(tl, th)))
}
fn seed_status(
    instance: &mut crate::process::EngineInstance,
    value: crate::process::windows::registry::RegistryInput32,
) {
    let out = &mut instance.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 40];
    header::write(out, *b"R3RI");
    out[16..24].copy_from_slice(&value.epoch.to_le_bytes());
    out[24..32].copy_from_slice(&value.token.to_le_bytes());
    header::write_u32(out, 32, value.received);
    header::write_u32(out, 36, value.total);
}
