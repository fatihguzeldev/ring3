use super::{cpu_store, mutate};

pub(crate) fn cpu_task_return(next_pc: u32) -> u32 {
    cpu_store(|instance| instance.cpu_task_return(next_pc))
}
pub(crate) fn cpu_task_return_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    next_pc: u32,
) -> u32 {
    cpu_store(|instance| {
        instance.cpu_task_return_resident(
            u64::from(key_low) | u64::from(key_high) << 32,
            u64::from(id_low) | u64::from(id_high) << 32,
            next_pc,
        )
    })
}

pub(crate) fn configure_task_controls() -> u32 {
    mutate(|instance| instance.configure_task_controls_request())
}
pub(crate) fn read_task_controls() -> u32 {
    mutate(|instance| instance.read_task_controls())
}
pub(crate) fn read_task_context() -> u32 {
    mutate(|instance| instance.read_task_context())
}

pub(crate) fn configure_task_return_sources() -> u32 {
    mutate(|instance| instance.configure_task_return_sources_request())
}
pub(crate) fn task_return() -> u32 {
    cpu_store(|instance| instance.task_return())
}
pub(crate) fn task_return_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32) -> u32 {
    cpu_store(|instance| {
        instance.task_return_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
        )
    })
}
