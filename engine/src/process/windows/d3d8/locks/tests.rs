use super::*;
use crate::{
    graphics::{
        GraphicsLimits,
        d3d8::resources::{
            BufferDesc, BufferKind, LockFlags, Pool, ResourceDesc, ResourceStore, Usage,
        },
    },
    memory::{GuestAddress, PAGE_SIZE, Permissions},
};

#[test]
fn disposal_keeps_equal_lock_tokens_from_other_device_resource_epochs() {
    let mut memory = AddressSpace::new(4).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let mut locks =
        LockRegistry32::new(PageRange::new(GuestAddress(0x2000), 2).unwrap(), 2).unwrap();
    let mut stores = [
        ResourceStore::new(17, GraphicsLimits::default()).unwrap(),
        ResourceStore::new(18, GraphicsLimits::default()).unwrap(),
    ];
    let mut ids = Vec::new();
    for (index, store) in stores.iter_mut().enumerate() {
        let id = store
            .create(ResourceDesc::Buffer(BufferDesc {
                length: 16,
                kind: BufferKind::Vertex { fvf: 0 },
                usage: Usage(0),
                pool: Pool::SystemMemory,
            }))
            .unwrap();
        let info = store.lock_buffer(id, 0, 16, LockFlags(0)).unwrap();
        assert_eq!(info.token, 1);
        locks
            .publish(
                &mut memory,
                id,
                info,
                &[index as u8 + 1; 16],
                0x1000 + index as u32 * 4,
                None,
            )
            .unwrap();
        ids.push(id);
    }
    let retained = locks.entries[1].range;
    let prepared = locks.prepare_disposal(&memory, &[ids[0]]).unwrap();
    assert_eq!(prepared.ranges.len(), 1);
    assert_ne!(prepared.ranges[0], retained);
    memory.unmap(prepared.ranges[0]).unwrap();
    locks.commit_disposal(&prepared);
    assert_eq!(locks.entries.len(), 1);
    assert_eq!(locks.entries[0].resource, ids[1]);
    assert_eq!(locks.staging_bytes(&memory, 0).unwrap(), [2; 16]);
    assert!(stores[1].is_locked(ids[1]).unwrap());
    assert_eq!(retained.count, 1);
    assert_eq!(memory.mapped_pages(), 2);
}

#[test]
fn reclaimed_lock_ranges_are_reused_but_foreign_mappings_are_skipped() {
    let mut memory = AddressSpace::new(4).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let bounds = PageRange::new(GuestAddress(0x2000), 2).unwrap();
    let mut locks = LockRegistry32::new(bounds, 2).unwrap();
    let mut store = ResourceStore::new(17, GraphicsLimits::default()).unwrap();
    let id = store
        .create(ResourceDesc::Buffer(BufferDesc {
            length: 16,
            kind: BufferKind::Vertex { fvf: 0 },
            usage: Usage(0),
            pool: Pool::SystemMemory,
        }))
        .unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x2000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x2000), &0xfeedbeefu32.to_le_bytes())
        .unwrap();
    for _ in 0..16 {
        let info = store.lock_buffer(id, 0, 16, LockFlags(0)).unwrap();
        locks
            .publish(&mut memory, id, info, &[7; 16], 0x1000, None)
            .unwrap();
        assert_eq!(locks.entries[0].range.first, 0x3000 / PAGE_SIZE as usize);
        memory.unmap(locks.entries[0].range).unwrap();
        locks.entries.clear();
        store.cancel_lock(id, info.token).unwrap();
    }
    let mut foreign = [0; 4];
    memory.read(GuestAddress(0x2000), &mut foreign).unwrap();
    assert_eq!(u32::from_le_bytes(foreign), 0xfeedbeef);
}

#[test]
fn idle_staging_foreign_protected_and_oversize_ranges_use_the_original_fallback() {
    for protect in [false, true] {
        let mut memory = AddressSpace::new(80).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::READ_WRITE,
            )
            .unwrap();
        let mut locks =
            LockRegistry32::new(PageRange::new(GuestAddress(0x2000), 128).unwrap(), 8).unwrap();
        let mut store = ResourceStore::new(17, GraphicsLimits::default()).unwrap();
        let id = store
            .create(ResourceDesc::Buffer(BufferDesc {
                length: 16,
                kind: BufferKind::Vertex { fvf: 0 },
                usage: Usage(0),
                pool: Pool::SystemMemory,
            }))
            .unwrap();
        locks.configure_staging_reuse(&mut memory, true).unwrap();
        let info = store.lock_buffer(id, 0, 16, LockFlags(0)).unwrap();
        locks
            .publish(&mut memory, id, info, &[7; 16], 0x1000, None)
            .unwrap();
        let original = locks.entries[0].range;
        locks.entries.clear();
        store.cancel_lock(id, info.token).unwrap();
        if protect {
            memory.protect(original, Permissions::READ).unwrap();
        } else {
            memory.unmap(original).unwrap();
            memory
                .map_zeroed(original, Permissions::READ_WRITE)
                .unwrap();
        }
        let foreign_version = memory
            .mapping_version_at(GuestAddress((original.first * PAGE_SIZE as usize) as u32))
            .unwrap();
        let info = store.lock_buffer(id, 0, 16, LockFlags(0)).unwrap();
        locks
            .publish(&mut memory, id, info, &[9; 16], 0x1000, None)
            .unwrap();
        let address = GuestAddress((original.first * PAGE_SIZE as usize) as u32);
        assert_ne!(locks.entries[0].range, original);
        assert_eq!(memory.mapping_version_at(address).unwrap(), foreign_version);
        assert_eq!(
            memory.permissions_at(address).unwrap(),
            if protect {
                Permissions::READ
            } else {
                Permissions::READ_WRITE
            }
        );
        assert_eq!(locks.staging_status()[2], 0);
        let cached = locks.entries[0].range;
        locks.entries.clear();
        store.cancel_lock(id, info.token).unwrap();
        let mut exit = Vec::new();
        locks
            .append_staging_exit_ranges(&memory, &mut exit)
            .unwrap();
        assert_eq!(exit, vec![cached]);
        locks.configure_staging_reuse(&mut memory, false).unwrap();
        assert!(
            memory
                .permissions_at(GuestAddress((cached.first * PAGE_SIZE as usize) as u32))
                .is_err()
        );
        locks.configure_staging_reuse(&mut memory, true).unwrap();
        let id = store
            .create(ResourceDesc::Buffer(BufferDesc {
                length: 256 * 1024 + 1,
                kind: BufferKind::Vertex { fvf: 0 },
                usage: Usage(0),
                pool: Pool::SystemMemory,
            }))
            .unwrap();
        let info = store.lock_buffer(id, 0, 0, LockFlags(0)).unwrap();
        locks
            .publish(
                &mut memory,
                id,
                info,
                &vec![5; 256 * 1024 + 1],
                0x1000,
                None,
            )
            .unwrap();
        assert_eq!(locks.staging_status()[1], 0);
        assert_eq!(locks.entries[0].range.count, 65);
    }
}
