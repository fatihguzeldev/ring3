use super::imported_executable;

use ring3_core::execution::{
    FileMetadata, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
};

const INITIALIZE: u32 = 0x7000_0474;
const CREATE: u32 = 0x7000_047c;
const QUERY_INTERFACE: u32 = 0x7000_0480;
const ADD_REF: u32 = 0x7000_0484;
const RELEASE: u32 = 0x7000_0488;
const RENDER_FILE: u32 = 0x7000_048c;
const UNSUPPORTED: u32 = 0x7000_0ffc;
const STACK: u32 = 0x1000_ef00;
const CLSID: u32 = 0x1000_e000;
const IID: u32 = 0x1000_e020;
const OUTPUT: u32 = 0x1000_e040;
const FILTER_GRAPH: [u8; 16] = guid(
    0xe436_ebb3,
    0x524f,
    0x11ce,
    [0x9f, 0x53, 0, 0x20, 0xaf, 0x0b, 0xa7, 0x70],
);
const GRAPH_BUILDER: [u8; 16] = directshow_iid(0x56a8_68a9);
const MEDIA_CONTROL: [u8; 16] = directshow_iid(0x56a8_68b1);
const MEDIA_SEEKING: [u8; 16] = guid(
    0x36b7_3880,
    0xc2c8,
    0x11cf,
    [0x8b, 0x46, 0, 0x80, 0x5f, 0x6c, 0xef, 0x60],
);
const MEDIA_POSITION: [u8; 16] = directshow_iid(0x56a8_68b2);
const VIDEO_WINDOW: [u8; 16] = directshow_iid(0x56a8_68b4);
const BASIC_AUDIO: [u8; 16] = directshow_iid(0x56a8_68b5);
const UNKNOWN: [u8; 16] = guid(0, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);

const fn guid(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> [u8; 16] {
    let a = data1.to_le_bytes();
    let b = data2.to_le_bytes();
    let c = data3.to_le_bytes();
    [
        a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], data4[0], data4[1], data4[2], data4[3],
        data4[4], data4[5], data4[6], data4[7],
    ]
}

const fn directshow_iid(data1: u32) -> [u8; 16] {
    guid(
        data1,
        0x0ad4,
        0x11ce,
        [0xb0, 0x3a, 0, 0x20, 0xaf, 0x0b, 0xa7, 0x70],
    )
}

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "ole32.dll", &["CoInitialize", "CoCreateInstance"]),
        64,
    )
    .unwrap()
}

fn write_guid(process: &mut Process32, address: u32, value: &[u8; 16]) {
    process.memory.write(u64::from(address), value).unwrap();
}

fn word(process: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn call(process: &mut Process32, api: u32, args: &[u32]) -> (ProcessStop, u32) {
    process.cpu.eip = api;
    process.cpu.set_register(Register32::Esp, STACK);
    let frame: Vec<_> = std::iter::once(0x0040_1000_u32)
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect();
    process.memory.write(u64::from(STACK), &frame).unwrap();
    let result = process.run(1);
    if result.reason == ProcessStop::Stopped(StopReason::InstructionLimit) {
        assert_eq!((result.instructions, result.api_calls), (0, 1));
        assert_eq!(
            process.cpu.register(Register32::Esp),
            STACK + 4 + u32::try_from(args.len() * 4).unwrap()
        );
    }
    (result.reason, process.cpu.register(Register32::Eax))
}

fn ok(process: &mut Process32, api: u32, args: &[u32]) -> u32 {
    let (stop, eax) = call(process, api, args);
    assert_eq!(stop, ProcessStop::Stopped(StopReason::InstructionLimit));
    eax
}

#[test]
fn render_file_reports_missing_media_and_refuses_an_existing_unrendered_file() {
    let media = FileMetadata {
        path: b"C:\\present.mpg",
        size: 4,
    };
    let mut process = Process32::load_with_options(
        &imported_executable::pe32(&[0xcc], "ole32.dll", &["CoInitialize", "CoCreateInstance"]),
        64,
        ProcessOptions {
            files: &[media],
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    write_guid(&mut process, CLSID, &FILTER_GRAPH);
    write_guid(&mut process, IID, &GRAPH_BUILDER);
    assert_eq!(ok(&mut process, INITIALIZE, &[0]), 0);
    assert_eq!(ok(&mut process, CREATE, &[CLSID, 0, 1, IID, OUTPUT]), 0);
    let graph = word(&process, OUTPUT);
    let address = STACK + 0x100;
    for (path, expected) in [
        (r"C:\missing.mpg", Some(0x8004_0216)),
        (r"C:\present.mpg", None),
        (r"C:\bad?.mpg", None),
    ] {
        let bytes: Vec<_> = path
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        process.memory.write(u64::from(address), &bytes).unwrap();
        let (stop, value) = call(&mut process, RENDER_FILE, &[graph, address, 0]);
        if let Some(expected) = expected {
            assert_eq!(stop, ProcessStop::Stopped(StopReason::InstructionLimit));
            assert_eq!(value, expected);
        } else {
            assert_eq!(
                stop,
                ProcessStop::UnsupportedApi {
                    address: RENDER_FILE
                }
            );
        }
    }
}

#[test]
fn graph_activation_exposes_five_distinct_interfaces_and_shared_lifetime() {
    let mut process = process();
    let pages_before = process.memory.mapped_pages();
    write_guid(&mut process, CLSID, &FILTER_GRAPH);
    write_guid(&mut process, IID, &GRAPH_BUILDER);
    assert_eq!(ok(&mut process, INITIALIZE, &[0]), 0);
    assert_eq!(process.memory.mapped_pages(), pages_before);
    assert_eq!(ok(&mut process, CREATE, &[CLSID, 0, 1, IID, OUTPUT]), 0);
    assert_eq!(process.memory.mapped_pages(), pages_before + 1);
    let graph = word(&process, OUTPUT);
    assert_ne!(graph, 0);
    let table = word(&process, graph);
    assert_eq!(
        [
            word(&process, table),
            word(&process, table + 4),
            word(&process, table + 8)
        ],
        [QUERY_INTERFACE, ADD_REF, RELEASE]
    );
    assert_eq!(word(&process, table + 13 * 4), RENDER_FILE);
    assert_eq!(
        call(&mut process, RENDER_FILE, &[graph, IID, 0]).0,
        ProcessStop::UnsupportedApi {
            address: RENDER_FILE
        }
    );

    let mut pointers = vec![graph];
    for iid in [
        MEDIA_CONTROL,
        MEDIA_SEEKING,
        MEDIA_POSITION,
        VIDEO_WINDOW,
        BASIC_AUDIO,
    ] {
        write_guid(&mut process, IID, &iid);
        assert_eq!(ok(&mut process, QUERY_INTERFACE, &[graph, IID, OUTPUT]), 0);
        let interface = word(&process, OUTPUT);
        assert!(!pointers.contains(&interface));
        let table = word(&process, interface);
        assert_eq!(
            [
                word(&process, table),
                word(&process, table + 4),
                word(&process, table + 8)
            ],
            [QUERY_INTERFACE, ADD_REF, RELEASE]
        );
        assert_eq!(word(&process, table + 13 * 4), UNSUPPORTED);
        pointers.push(interface);
    }
    assert_eq!(ok(&mut process, ADD_REF, &[pointers[1]]), 7);
    assert_eq!(ok(&mut process, RELEASE, &[pointers[2]]), 6);
    write_guid(&mut process, IID, &UNKNOWN);
    assert_eq!(
        ok(&mut process, QUERY_INTERFACE, &[pointers[3], IID, OUTPUT]),
        0
    );
    assert_eq!(word(&process, OUTPUT), graph);
    for expected in (0..7).rev() {
        assert_eq!(ok(&mut process, RELEASE, &[graph]), expected);
    }
    let (stop, _) = call(&mut process, ADD_REF, &[graph]);
    assert_eq!(stop, ProcessStop::UnsupportedApi { address: ADD_REF });
}

#[test]
fn activation_failures_clear_output_without_creating_a_graph() {
    let mut process = process();
    write_guid(&mut process, CLSID, &FILTER_GRAPH);
    write_guid(&mut process, IID, &GRAPH_BUILDER);
    assert_eq!(
        ok(&mut process, CREATE, &[CLSID, 0, 1, IID, OUTPUT]),
        0x8004_01f0
    );
    assert_eq!(word(&process, OUTPUT), 0);
    assert_eq!(ok(&mut process, INITIALIZE, &[0]), 0);

    write_guid(&mut process, CLSID, &UNKNOWN);
    assert_eq!(
        ok(&mut process, CREATE, &[CLSID, 0, 1, IID, OUTPUT]),
        0x8004_0154
    );
    assert_eq!(word(&process, OUTPUT), 0);
    write_guid(&mut process, CLSID, &FILTER_GRAPH);
    assert_eq!(
        ok(&mut process, CREATE, &[CLSID, 1, 1, IID, OUTPUT]),
        0x8004_0110
    );
    assert_eq!(
        ok(&mut process, CREATE, &[CLSID, 0, 4, IID, OUTPUT]),
        0x8004_0154
    );
    write_guid(&mut process, IID, &FILTER_GRAPH);
    assert_eq!(
        ok(&mut process, CREATE, &[CLSID, 0, 1, IID, OUTPUT]),
        0x8000_4002
    );
    assert_eq!(word(&process, OUTPUT), 0);
    assert_eq!(
        ok(&mut process, CREATE, &[CLSID, 0, 1, IID, 0]),
        0x8000_4003
    );

    write_guid(&mut process, IID, &GRAPH_BUILDER);
    assert_eq!(ok(&mut process, CREATE, &[CLSID, 0, 1, IID, OUTPUT]), 0);
    let graph = word(&process, OUTPUT);
    write_guid(&mut process, IID, &FILTER_GRAPH);
    assert_eq!(
        ok(&mut process, QUERY_INTERFACE, &[graph, IID, OUTPUT]),
        0x8000_4002
    );
    assert_eq!(word(&process, OUTPUT), 0);
    assert_eq!(ok(&mut process, RELEASE, &[graph]), 0);
    let (stop, _) = call(&mut process, CREATE, &[0x1000_fffc, 0, 1, IID, OUTPUT]);
    assert!(matches!(
        stop,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(word(&process, OUTPUT), 0);
}
