use super::*;
use crate::{
    memory::{PAGE_SIZE, PageRange},
    process::Ole32Config32,
    windows::memory::{MEM_COMMIT, MEM_FREE, MEM_PRIVATE, MEM_RESERVE, MemoryError32},
};

fn query(engine: &mut EngineInstance, address: u32, output: u32) -> [u32; 7] {
    assert_eq!(engine.memory_query(&[address, output, 28]).unwrap(), 28);
    let mut bytes = [0; 28];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(output), &mut bytes)
        .unwrap();
    core::array::from_fn(|index| {
        u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
    })
}

#[test]
fn sparse_loaded_image_gap_is_reserved_and_query_stops_at_image_boundaries() {
    let mut image = crate::process::windows::process::test_image_for_startup();
    image.resize(1536, 0);
    image[0x86..0x88].copy_from_slice(&2u16.to_le_bytes());
    image[0xd0..0xd4].copy_from_slice(&0x5000u32.to_le_bytes());
    for (offset, value) in [
        (8, 512u32),
        (12, 0x4000),
        (16, 512),
        (20, 1024),
        (36, 0xc0000040),
    ] {
        image[0x1a0 + offset..0x1a4 + offset].copy_from_slice(&value.to_le_bytes());
    }
    let mut engine = EngineInstance::new(128, KEY).unwrap();
    engine.register_guest_module("sparse.exe", &image).unwrap();
    engine
        .start_module_process("sparse.exe", None, Default::default())
        .unwrap();
    engine.map(0x30000000, 1, 3).unwrap();
    let before = engine.memory().unwrap().mapped_pages();
    let info = query(&mut engine, 0x402123, 0x30000000);
    assert_eq!(info[0], 0x402000);
    assert_eq!(info[1], 0x400000);
    assert_eq!(info[2], 0x80);
    assert_eq!(info[3], 2 * PAGE_SIZE);
    assert_eq!(info[4], MEM_RESERVE);
    assert_eq!(info[5], 0);
    assert_eq!(info[6], 0x1000000);
    assert_eq!(engine.memory().unwrap().mapped_pages(), before);
    assert_eq!(
        engine.memory_allocate(&[0x402000, PAGE_SIZE, MEM_RESERVE, 4]),
        Err(MemoryError32::InvalidAddress)
    );
    let committed = query(&mut engine, 0x404000, 0x30000000);
    assert_eq!(
        (committed[1], committed[3], committed[4], committed[6]),
        (0x400000, PAGE_SIZE, MEM_COMMIT, 0x1000000)
    );
    assert_eq!(committed[2], 0x80);
    let free = query(&mut engine, 0x3ff000, 0x30000000);
    assert_eq!(
        (free[0], free[3], free[4], free[6]),
        (0x3ff000, PAGE_SIZE, MEM_FREE, 0)
    );
}

#[test]
fn configured_unmapped_arena_is_reserved_and_does_not_merge_with_free_neighbors() {
    let mut engine = engine();
    engine
        .configure_ole32(Ole32Config32 {
            objects: PageRange::new(GuestAddress(0x200000), 16).unwrap(),
            max_objects: 8,
            max_threads: 8,
        })
        .unwrap();
    let info = query(&mut engine, 0x201321, 0x1800);
    assert_eq!(
        (info[0], info[1], info[3], info[4], info[5], info[6]),
        (
            0x201000,
            0x200000,
            15 * PAGE_SIZE,
            MEM_RESERVE,
            0,
            MEM_PRIVATE
        )
    );
    let free = query(&mut engine, 0x1ff123, 0x1800);
    assert_eq!((free[0], free[3], free[4]), (0x1ff000, PAGE_SIZE, MEM_FREE));
    assert_eq!(
        engine.memory_allocate(&[0x200000, PAGE_SIZE, MEM_RESERVE, 4]),
        Err(MemoryError32::InvalidAddress)
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(0x201000))
            .is_err()
    );
}

#[test]
fn canonical_thread_storage_keeps_its_reserve_commit_query_boundaries() {
    let mut engine = engine();
    let base = engine
        .windows_memory
        .reserve_thread_storage(engine.memory.as_mut().unwrap(), &[], 0x10000)
        .unwrap();
    engine
        .windows_memory
        .commit_thread_storage(
            engine.memory.as_mut().unwrap(),
            base + 0xf000,
            PAGE_SIZE,
            &[],
        )
        .unwrap();
    let reserve = query(&mut engine, base + 3, 0x1800);
    assert_eq!(
        (reserve[0], reserve[1], reserve[3], reserve[4], reserve[6]),
        (base, base, 0xf000, MEM_RESERVE, MEM_PRIVATE)
    );
    let commit = query(&mut engine, base + 0xf123, 0x1800);
    assert_eq!(
        (commit[0], commit[1], commit[3], commit[4], commit[5]),
        (base + 0xf000, base, PAGE_SIZE, MEM_COMMIT, 4)
    );
}

#[test]
fn automation_callback_arena_has_one_reserved_owner_before_guest_allocation() {
    let mut engine = engine();
    let range = engine.automation_scratch_bounds();
    let base = range.first as u32 * PAGE_SIZE;
    let info = query(&mut engine, base + 3, 0x1800);
    assert_eq!(
        (info[0], info[1], info[3], info[4], info[5], info[6]),
        (base, base, 32 * PAGE_SIZE, MEM_RESERVE, 0, MEM_PRIVATE)
    );
    assert_eq!(
        engine.memory_allocate(&[base, PAGE_SIZE, MEM_RESERVE, 4]),
        Err(MemoryError32::InvalidAddress)
    );
    assert!(engine.check_guest_reservation_admission(&[range]).is_err());
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(base))
            .is_err()
    );
}
