use crate::{
    files::{FileAccess32, FileDisposition32, FileLimits32, FileOpenOptions32, Vfs32},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::windows::files::{
        FileRuntime32,
        asynchronous::AsyncRuntime32,
        handles::{KernelFileHandles32, KernelFileObject32},
        mapping::{MappingLimits32, MappingRuntime32},
        ordinary::OrdinaryRuntime32,
    },
    windows::synchronization::{SynchronizationContext32, SynchronizationLimits32},
};

pub(in crate::process::windows::files) fn runtime()
-> (FileRuntime32, AddressSpace, SynchronizationContext32) {
    let mut memory = AddressSpace::new(128).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 4).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    (
        FileRuntime32 {
            vfs: Vfs32::new(7, FileLimits32::default()).unwrap(),
            handles: KernelFileHandles32::new(64).unwrap(),
            ordinary: OrdinaryRuntime32::new(16, 64).unwrap(),
            mappings: MappingRuntime32::new(
                MappingLimits32 {
                    objects: 16,
                    views: 32,
                },
                PageRange::new(GuestAddress(0x1000_0000), 0x1000).unwrap(),
            )
            .unwrap(),
            asynchronous: AsyncRuntime32::new(16, 1024 * 1024).unwrap(),
            reply: None,
            events: Vec::new(),
            host_input: Default::default(),
        },
        memory,
        SynchronizationContext32::new(7, SynchronizationLimits32::default(), 0).unwrap(),
    )
}
pub(in crate::process::windows::files) fn path(
    memory: &mut AddressSpace,
    address: u32,
    value: &str,
) {
    let mut bytes = Vec::new();
    for unit in value.encode_utf16().chain([0]) {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    memory.write(GuestAddress(address), &bytes).unwrap();
}
pub(in crate::process::windows::files) fn open(
    runtime: &mut FileRuntime32,
    path: &str,
    access: FileAccess32,
) -> u32 {
    open_options(
        runtime,
        path,
        FileOpenOptions32 {
            access,
            disposition: FileDisposition32::OpenAlways,
            ..FileOpenOptions32::default()
        },
    )
}
pub(in crate::process::windows::files) fn open_options(
    runtime: &mut FileRuntime32,
    path: &str,
    options: FileOpenOptions32,
) -> u32 {
    let path: Vec<u16> = path.encode_utf16().collect();
    let prepared = runtime.vfs.prepare_open(&path, options).unwrap();
    let open = runtime.vfs.commit_open(prepared).open;
    let handle = runtime.handles.prepare(options.access.0, false).unwrap();
    runtime
        .handles
        .commit(handle, KernelFileObject32::File(open))
}
pub(in crate::process::windows::files) fn dword(memory: &AddressSpace, address: u32) -> u32 {
    u32::from_le_bytes(read::<4>(memory, address))
}
pub(in crate::process::windows::files) fn read<const N: usize>(
    memory: &AddressSpace,
    address: u32,
) -> [u8; N] {
    let mut bytes = [0; N];
    memory.read(GuestAddress(address), &mut bytes).unwrap();
    bytes
}
pub(in crate::process::windows::files) fn file_bytes(
    runtime: &mut FileRuntime32,
    handle: u32,
) -> Vec<u8> {
    let file = runtime
        .vfs
        .file_for_open(runtime.handles.file(handle).unwrap())
        .unwrap();
    let length = runtime.vfs.info(file).unwrap().size as usize;
    match runtime.vfs.prepare_read_at(file, 0, length).unwrap() {
        crate::files::FileIo32::Ready(read) => read.bytes().to_vec(),
        crate::files::FileIo32::NeedBytes(_) => panic!("fixture owns file bytes"),
    }
}
pub(in crate::process::windows::files) fn write_file(
    runtime: &mut FileRuntime32,
    handle: u32,
    bytes: &[u8],
) {
    let file = runtime
        .vfs
        .file_for_open(runtime.handles.file(handle).unwrap())
        .unwrap();
    match runtime.vfs.prepare_write_at(file, 0, bytes).unwrap() {
        crate::files::FileIo32::Ready(write) => {
            runtime.vfs.commit_write(write);
        }
        crate::files::FileIo32::NeedBytes(_) => panic!("fixture owns file bytes"),
    }
}
