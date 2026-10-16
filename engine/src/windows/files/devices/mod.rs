mod context;
mod names;

pub(crate) use context::NullDevices32;
pub(crate) use names::device_name;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DeviceDescriptionId32(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeviceName32 {
    Null,
    Console,
    ConsoleInput,
    ConsoleOutput,
}
pub(crate) struct PreparedNullOpen32 {
    slot: usize,
    id: DeviceDescriptionId32,
    overlapped: bool,
}
struct NullDescription32 {
    id: DeviceDescriptionId32,
    references: u32,
    overlapped: bool,
}

#[cfg(test)]
mod tests;
