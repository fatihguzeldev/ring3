use crate::{
    cpu::dbt::GateSpec,
    loader::{
        ModuleError32, SystemExportTarget32, SystemModule32, pe32::module::PreparedImagePage32,
    },
    memory::{GuestAddress, Permissions},
};

fn gate_ids(module: &SystemModule32) -> Result<Vec<u32>, ModuleError32> {
    let mut ids = Vec::new();
    ids.try_reserve_exact(module.exports.len())
        .map_err(|_| ModuleError32::Allocation)?;
    for export in &module.exports {
        if let SystemExportTarget32::Gate(id) = export.target
            && !ids.contains(&id)
        {
            ids.push(id);
        }
    }
    Ok(ids)
}

pub(super) fn image_size(module: &SystemModule32) -> Result<u32, ModuleError32> {
    let bytes = (gate_ids(module)?.len() as u64 * 16).max(1).div_ceil(4096) * 4096;
    u32::try_from(bytes).map_err(|_| ModuleError32::Capacity)
}

pub(super) fn prepare(
    module: &SystemModule32,
    base: u32,
    size: u32,
) -> Result<(Vec<PreparedImagePage32>, Vec<GateSpec>), ModuleError32> {
    let ids = gate_ids(module)?;
    let mut pages = Vec::new();
    pages
        .try_reserve_exact((size / 4096) as usize)
        .map_err(|_| ModuleError32::Allocation)?;
    for offset in (0..size).step_by(4096) {
        pages.push(PreparedImagePage32 {
            address: GuestAddress(base + offset),
            permissions: Permissions::READ_EXECUTE,
            bytes: Box::new([0; 4096]),
        });
    }
    let mut gates = Vec::new();
    gates
        .try_reserve_exact(ids.len())
        .map_err(|_| ModuleError32::Allocation)?;
    for (index, id) in ids.into_iter().enumerate() {
        let offset = index * 16;
        pages[offset / 4096].bytes[offset % 4096..offset % 4096 + 2].copy_from_slice(&[0x0f, 0x0b]);
        gates.push(GateSpec {
            entry: GuestAddress(base + offset as u32),
            id,
        });
    }
    Ok((pages, gates))
}
