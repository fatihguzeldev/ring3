use ring3_observability::{Config, Field, Level, Observer};

#[test]
fn off_and_filtered_scopes_produce_no_events() {
    let off = Observer::new(Config::default()).unwrap();
    off.instant(Level::Basic, "engine.loader", "load", &[]);
    assert!(off.drain().is_empty());
    let observer = Observer::new(Config {
        level: Level::Basic,
        contexts: vec!["engine.cpu".into()],
        ..Config::default()
    })
    .unwrap();
    let baseline = observer.stats().emitted_events;
    observer.instant(Level::Detailed, "engine.cpu", "detail", &[]);
    observer.instant(Level::Basic, "engine.cpus", "not-a-child", &[]);
    observer.instant(Level::Basic, "engine.loader", "load", &[]);
    assert_eq!(observer.stats().emitted_events, baseline);
    observer.instant(Level::Basic, "engine.cpu.cache", "hit", &[]);
    assert_eq!(observer.stats().emitted_events, baseline + 1);
}

#[test]
fn bounded_capture_reports_loss_and_preserves_scope_ends() {
    let observer = Observer::new(Config {
        level: Level::Detailed,
        max_buffer_bytes: 4096,
        ..Config::default()
    })
    .unwrap();
    let scope = observer.scope(Level::Basic, "engine.loader", "load", &[]);
    for _ in 0..200 {
        observer.instant(
            Level::Basic,
            "engine.loader",
            "page",
            &[Field::string("value", &"x".repeat(200))],
        );
    }
    drop(scope);
    let before = observer.stats();
    assert!(before.dropped_events > 0);
    assert!(before.buffered_bytes <= 4096);
    assert_eq!(before.open_scopes, 0);
    assert!(!observer.drain().is_empty());
    assert_eq!(observer.stats().buffered_bytes, 0);
    let bytes = observer.finish();
    assert!(!bytes.is_empty());
    assert!(observer.finish().is_empty());
    assert!(observer.stats().finished);
    assert!(observer.stats().dropped_events >= before.dropped_events);
}

#[test]
fn component_context_is_scoped_and_nested_captures_restore_the_parent() {
    let first = Observer::new(Config {
        level: Level::Basic,
        ..Config::default()
    })
    .unwrap();
    let second = Observer::new(Config {
        level: Level::Basic,
        producer_sequence_id: 2,
        track_uuid: 0x2_0000_0001,
        ..Config::default()
    })
    .unwrap();
    let a = first.stats().emitted_events;
    let b = second.stats().emitted_events;
    {
        let _context = first.enter();
        ring3_observability::current().instant(Level::Basic, "engine.loader", "one", &[]);
        {
            let _nested = second.enter();
            ring3_observability::current().instant(Level::Basic, "engine.loader", "two", &[]);
        }
        ring3_observability::current().instant(Level::Basic, "engine.loader", "three", &[]);
    }
    ring3_observability::current().instant(Level::Basic, "engine.loader", "off", &[]);
    assert_eq!(first.stats().emitted_events, a + 2);
    assert_eq!(second.stats().emitted_events, b + 1);
}

#[test]
fn disabled_context_shadows_active_capture_without_contaminating_its_events() {
    let observer = Observer::new(Config {
        level: Level::Basic,
        ..Config::default()
    })
    .unwrap();
    let before = observer.stats().emitted_events;
    let _outer = observer.enter();
    {
        let _off = Observer::disabled().enter();
        ring3_observability::current().instant(
            Level::Basic,
            "engine.loader",
            "should-not-record",
            &[],
        );
    }
    assert_eq!(observer.stats().emitted_events, before);
    ring3_observability::current().instant(Level::Basic, "engine.loader", "resumed", &[]);
    assert_eq!(observer.stats().emitted_events, before + 1);
}

#[test]
fn byte_sized_drains_preserve_all_buffered_data_and_stop_at_empty() {
    let observer = Observer::new(Config {
        level: Level::Basic,
        ..Config::default()
    })
    .unwrap();
    observer.instant(Level::Basic, "engine.loader", "load", &[]);
    observer.finalize();
    let expected = observer.stats().buffered_bytes;
    let mut output = Vec::new();
    loop {
        let chunk = observer.drain_chunk(1);
        if chunk.is_empty() {
            break;
        }
        assert_eq!(chunk.len(), 1);
        output.extend_from_slice(&chunk);
    }
    assert_eq!(output.len(), expected);
    assert_eq!(output[0], 0x0a);
    assert!(
        output
            .windows("capture.finished".len())
            .any(|bytes| bytes == b"capture.finished")
    );
    assert_eq!(observer.stats().buffered_bytes, 0);
    assert!(observer.drain().is_empty());
}

#[test]
fn finalization_reports_unfinished_scopes_instead_of_inventing_scope_completion() {
    let observer = Observer::new(Config {
        level: Level::Basic,
        ..Config::default()
    })
    .unwrap();
    let scope = observer.scope(Level::Basic, "engine.loader", "unfinished", &[]);
    let bytes = observer.finish();
    assert_eq!(observer.stats().open_scopes, 1);
    assert!(
        bytes
            .windows("open_scopes".len())
            .any(|bytes| bytes == b"open_scopes")
    );
    drop(scope);
    assert!(observer.drain().is_empty());
}

#[test]
fn invalid_namespace_or_resource_settings_are_rejected() {
    for config in [
        Config {
            producer_sequence_id: 0,
            ..Config::default()
        },
        Config {
            track_uuid: u64::MAX,
            ..Config::default()
        },
        Config {
            max_buffer_bytes: 4095,
            ..Config::default()
        },
        Config {
            max_buffer_bytes: 64 * 1024 * 1024 + 1,
            ..Config::default()
        },
        Config {
            contexts: vec!["".into()],
            ..Config::default()
        },
    ] {
        assert!(Observer::new(config).is_err());
    }
}

#[test]
fn finalized_multichunk_trace_is_finished_only_after_every_byte_is_drained() {
    let observer = Observer::new(Config {
        level: Level::Basic,
        max_buffer_bytes: 64 * 1024,
        ..Config::default()
    })
    .unwrap();
    for _ in 0..100 {
        observer.instant(
            Level::Basic,
            "engine.loader",
            "page",
            &[Field::string(
                "path",
                "a long enough authored file path for a multichunk trace",
            )],
        );
    }
    observer.finalize();
    assert!(observer.stats().buffered_bytes > 4080 * 2);
    assert!(!observer.stats().drained());
    let mut chunks = 0;
    while !observer.stats().drained() {
        assert!(!observer.drain_chunk(4080).is_empty());
        chunks += 1;
    }
    assert!(chunks > 2);
    assert_eq!(observer.stats().buffered_bytes, 0);
}
