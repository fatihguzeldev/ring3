use ring3_engine::{
    cpu::{
        dbt::{CompileLimits, compile_entry_region},
        x86::decode::decode_one,
    },
    loader::load_pe32,
    memory::GuestAddress,
};
use ring3_observability::{CategorySelection, Config, Level, Observer, categories};

#[test]
fn direct_loader_decoder_and_codegen_emit_without_process_or_playground() {
    let observer = Observer::new(Config {
        level: Level::Detailed,
        ..Config::default()
    })
    .unwrap();
    {
        let _capture = observer.enter();
        let mut bytes = super::image::image();
        bytes[super::image::TEXT_RAW + 5..super::image::TEXT_RAW + 7]
            .copy_from_slice(&[0xeb, 0xfe]);
        let image = load_pe32(&bytes, 4).unwrap();
        let (memory, _) = image.into_parts();
        assert_eq!(
            decode_one(&memory, GuestAddress(super::image::BASE + 0x1000))
                .unwrap()
                .length(),
            5
        );
        compile_entry_region(
            &memory,
            &[GuestAddress(super::image::BASE + 0x1000)],
            CompileLimits::default(),
        )
        .unwrap();
    }
    let trace = observer.finish();
    for name in [
        "engine.loader.pe32",
        "engine.cpu.decode",
        "engine.cpu.codegen",
        "capture.finished",
    ] {
        assert!(
            trace
                .windows(name.len())
                .any(|bytes| bytes == name.as_bytes()),
            "missing {name}"
        );
    }
    assert_eq!(observer.stats().dropped_events, 0);
}

#[test]
fn basic_context_filter_does_not_emit_detailed_cpu_events() {
    let observer = Observer::new(Config {
        level: Level::Basic,
        categories: CategorySelection::Only(vec![categories::ENGINE_LOADER]),
        ..Config::default()
    })
    .unwrap();
    {
        let _capture = observer.enter();
        load_pe32(&super::image::image(), 4).unwrap();
    }
    let trace = observer.finish();
    assert!(trace.windows("load".len()).any(|bytes| bytes == b"load"));
    assert!(!trace.windows("parse".len()).any(|bytes| bytes == b"parse"));
}
