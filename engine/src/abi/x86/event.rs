use crate::{
    abi::{AbiError, header},
    process::CpuEvent32,
};

pub const CPU_EVENT_SITE_SIZE: usize = 48;

/// the existing state/exit/x87/simd/segment records carry the corresponding stopped context.
pub fn encode_cpu_event_site32(event: &CpuEvent32, output: &mut [u8]) -> Result<(), AbiError> {
    if output.len() != CPU_EVENT_SITE_SIZE {
        return Err(AbiError::Length);
    }
    let site = event.instruction;
    let fields = [
        event
            .origin
            .and_then(|origin| origin.vector())
            .map_or(u32::MAX, u32::from),
        site.map_or(event.state.eip, |site| site.eip),
        event.state.eip,
        site.map_or(event.exit.retired, |site| site.retired),
        site.map_or(event.state.eflags, |site| site.flags),
        site.map_or(0, |site| u32::from(site.ss_debug_shadow)),
        site.and_then(|site| site.work_consumed).unwrap_or(u32::MAX),
        event.exit.retired,
    ];
    header::write(output, *b"R3CE");
    for (index, value) in fields.into_iter().enumerate() {
        header::write_u32(output, 16 + index * 4, value);
    }
    Ok(())
}
