#[path = "support/imported_executable.rs"]
mod imported_executable;

use std::collections::BTreeSet;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const MALLOC: u32 = 0x7000_011c;
const FREE: u32 = 0x7000_0120;
const STACK: u32 = 0x1000_ff00;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["malloc", "free"]),
        64,
    )
    .unwrap()
}

fn call(process: &mut Process32, api: u32, argument: u32) -> u32 {
    process.cpu.eip = api;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 99);
    for (index, word) in [0x0040_1000, argument].into_iter().enumerate() {
        process
            .memory
            .write(u64::from(STACK) + index as u64 * 4, &word.to_le_bytes())
            .unwrap();
    }
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    process.cpu.register(Register32::Eax)
}

#[test]
fn hundreds_of_live_crt_objects_share_pages_and_reuse_zeroed_space() {
    let mut process = process();
    let baseline = process.memory.mapped_pages();
    let mut pointers = Vec::new();
    for index in 0..256_u32 {
        let pointer = call(&mut process, MALLOC, 76);
        assert_ne!(pointer, 0, "allocation {index}");
        assert_eq!(pointer % 16, 0);
        let mut initial = [1; 76];
        process
            .memory
            .read(u64::from(pointer), &mut initial)
            .unwrap();
        assert_eq!(initial, [0; 76]);
        process
            .memory
            .write(
                u64::from(pointer),
                &[u8::try_from(index % 255 + 1).unwrap()],
            )
            .unwrap();
        process
            .memory
            .write(u64::from(pointer + 75), &[42])
            .unwrap();
        pointers.push(pointer);
    }
    let unique: BTreeSet<_> = pointers.iter().copied().collect();
    assert_eq!(unique.len(), pointers.len());
    assert!(process.memory.mapped_pages() <= baseline + 8);
    for (index, &pointer) in pointers.iter().enumerate() {
        let mut first = [0];
        let mut last = [0];
        process.memory.read(u64::from(pointer), &mut first).unwrap();
        process
            .memory
            .read(u64::from(pointer + 75), &mut last)
            .unwrap();
        assert_eq!(first, [u8::try_from(index % 255 + 1).unwrap()]);
        assert_eq!(last, [42]);
    }

    let mut freed = BTreeSet::new();
    for &pointer in pointers.iter().step_by(2) {
        assert_eq!(call(&mut process, FREE, pointer), 99);
        freed.insert(pointer);
    }
    let mut recycled = Vec::new();
    for _ in 0..freed.len() {
        let pointer = call(&mut process, MALLOC, 76);
        assert!(freed.remove(&pointer));
        let mut bytes = [1; 76];
        process.memory.read(u64::from(pointer), &mut bytes).unwrap();
        assert_eq!(bytes, [0; 76]);
        recycled.push(pointer);
    }
    assert!(freed.is_empty());
    for &pointer in pointers.iter().skip(1).step_by(2).chain(&recycled) {
        assert_eq!(call(&mut process, FREE, pointer), 99);
    }
    assert_eq!(process.memory.mapped_pages(), baseline);
}

#[test]
fn mixed_small_sizes_remain_disjoint_and_release_empty_pages() {
    let mut process = process();
    let baseline = process.memory.mapped_pages();
    let sizes = [0_u32, 1, 16, 17, 33, 36, 65, 76, 255, 512, 1024, 2048];
    let mut allocated = Vec::new();
    for _ in 0..8 {
        for size in sizes {
            let pointer = call(&mut process, MALLOC, size);
            assert_ne!(pointer, 0);
            assert_eq!(pointer % 16, 0);
            let byte = [u8::try_from(size % 251 + 1).unwrap()];
            process.memory.write(u64::from(pointer), &byte).unwrap();
            allocated.push((pointer, size, byte));
        }
    }
    assert!(process.memory.mapped_pages() <= baseline + 20);
    for &(pointer, _, byte) in &allocated {
        let mut actual = [0];
        process
            .memory
            .read(u64::from(pointer), &mut actual)
            .unwrap();
        assert_eq!(actual, byte);
    }
    for &(pointer, _, _) in &allocated {
        assert_eq!(call(&mut process, FREE, pointer), 99);
    }
    assert_eq!(process.memory.mapped_pages(), baseline);
}
