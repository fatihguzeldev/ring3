use ring3_core::execution::{
    FileContentsMode, FileMetadata, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
};

fn push(code: &mut Vec<u8>, value: u32) {
    code.push(0x68);
    code.extend(value.to_le_bytes());
}

fn executable() -> Vec<u8> {
    let mut code = Vec::new();
    for (index, path) in [0x0040_2200, 0x0040_2240, 0x0040_2200]
        .into_iter()
        .enumerate()
    {
        for value in [0, 0, 3, 0, 1, 0x8000_0000, path] {
            push(&mut code, value);
        }
        code.extend([0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x89, 0xc6]);
        for value in [
            0,
            0x0040_2380,
            4,
            0x0040_2300 + u32::try_from(index).unwrap() * 4,
        ] {
            push(&mut code, value);
        }
        code.extend([0x56, 0xff, 0x15, 0x64, 0x20, 0x40, 0]);
        code.extend([0x56, 0xff, 0x15, 0x68, 0x20, 0x40, 0]);
    }
    code.push(0xcc);
    super::imported_executable::pe32(
        &code,
        "kErNeL32.dll",
        &["CreateFileA", "ReadFile", "CloseHandle"],
    )
}

pub fn imported_opens_resume_read_and_refill_evicted_snapshots() {
    let files = [
        FileMetadata {
            path: b"C:\\a",
            size: 4,
        },
        FileMetadata {
            path: b"C:\\b",
            size: 4,
        },
    ];
    let mut results = Vec::new();
    for budget in [1, 5, 100] {
        let mut p = Process32::load_with_options(
            &executable(),
            64,
            ProcessOptions {
                files: &files,
                file_contents_mode: FileContentsMode::OnDemand { cache_bytes: 4 },
                ..ProcessOptions::default()
            },
        )
        .unwrap();
        p.memory.write(0x0040_2200, b"C:\\a\0").unwrap();
        p.memory.write(0x0040_2240, b"C:\\b\0").unwrap();
        let pages = p.memory.mapped_pages();
        let mut counts = (0, 0);
        let mut ids = Vec::new();
        let mut complete = false;
        for _ in 0..100 {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            match run.reason {
                ProcessStop::FileContentsRequired => {
                    let request = p.pending_file_contents().unwrap();
                    assert_eq!(request.size, 4);
                    let id = request.id;
                    let bytes = match request.path {
                        b"C:\\a" => *b"data",
                        b"C:\\b" => *b"next",
                        _ => panic!("unexpected catalog path"),
                    };
                    let cpu = p.cpu;
                    let waiting = p.run(100);
                    assert_eq!((waiting.instructions, waiting.api_calls), (0, 0));
                    assert_eq!(waiting.reason, ProcessStop::FileContentsRequired);
                    assert_eq!(p.cpu, cpu);
                    p.supply_file_contents(id, Box::from(bytes)).unwrap();
                    assert_eq!(p.cpu, cpu);
                    ids.push(id);
                }
                ProcessStop::Stopped(StopReason::Breakpoint) => {
                    complete = true;
                    break;
                }
                reason => assert_eq!(reason, ProcessStop::Stopped(StopReason::InstructionLimit)),
            }
        }
        assert!(complete);
        assert_eq!(ids, [1, 2, 3]);
        assert_eq!(counts, (52, 9));
        let mut output = [0; 12];
        p.memory.read(0x0040_2300, &mut output).unwrap();
        assert_eq!(&output, b"datanextdata");
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(p.memory.mapped_pages(), pages);
        results.push((p.cpu, counts, output));
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}
