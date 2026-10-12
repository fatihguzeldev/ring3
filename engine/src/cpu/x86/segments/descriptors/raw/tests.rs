use super::*;

fn record(low: u32, high: u32) -> SegmentDescriptorRecord {
    SegmentDescriptorRecord::from_words(low, high)
}

fn legacy(kind: SegmentDescriptorKind) -> SegmentDescriptor {
    SegmentDescriptor {
        selector: 0x43,
        base: 0x1234_5678,
        dpl: 3,
        present: true,
        kind,
    }
}

#[test]
fn legacy_configuration_preserves_its_exact_view_and_rejection_atomicity() {
    let mut table = SegmentDescriptorTable::empty();
    for kind in [
        SegmentDescriptorKind::Data { writable: false },
        SegmentDescriptorKind::Data { writable: true },
        SegmentDescriptorKind::Code {
            readable: true,
            conforming: false,
        },
        SegmentDescriptorKind::Code {
            readable: false,
            conforming: true,
        },
        SegmentDescriptorKind::System,
    ] {
        let input = legacy(kind);
        table.configure(input).unwrap();
        let mut normalized = input;
        normalized.selector = 0x40;
        assert_eq!(table.descriptor(0x43), Some(&normalized));
        assert_eq!(
            table.raw_descriptor(0x42),
            SegmentDescriptorRecord::from_legacy(input).as_ref()
        );
    }
    for (selector, dpl, error_code) in [(3, 3, 0), (0x43, 4, 0x40)] {
        let before = table.clone();
        let mut input = legacy(SegmentDescriptorKind::System);
        input.selector = selector;
        input.dpl = dpl;
        assert_eq!(
            table.configure(input),
            Err(SegmentLoadError {
                failure: SegmentLoadFailure::Type,
                error_code
            })
        );
        assert_eq!(table, before);
    }
}

#[test]
fn raw_system_payloads_have_no_invented_code_data_view_or_load() {
    let mut table = SegmentDescriptorTable::empty();
    for kind in 0..16 {
        let raw = record(0x7654_3210, 0xfedc_e0ba | (kind << 8));
        table.configure_raw(0x43, raw).unwrap();
        assert_eq!(table.raw_descriptor(0x40), Some(&raw));
        assert_eq!(table.descriptor(0x43), None);
        assert!(!table.verify(0x43, 3, false));
        assert!(!table.verify(0x43, 3, true));
        assert_eq!(
            table.resolve_load(SegmentRegister::Fs, 0x43, 3, &SegmentState::default()),
            Err(SegmentLoadError {
                failure: SegmentLoadFailure::Type,
                error_code: 0x40
            })
        );
        table.commit_accessed(0x43);
        assert_eq!(table.raw_descriptor(0x43), Some(&raw));
        let before = table.clone();
        assert_eq!(
            table.update_base(0x43, 0),
            Err(SegmentLoadError {
                failure: SegmentLoadFailure::Type,
                error_code: 0x40
            })
        );
        assert_eq!(table, before);
    }
    let before = table.clone();
    for selector in 0..4 {
        assert_eq!(
            table.configure_raw(selector, record(u32::MAX, u32::MAX)),
            Err(SegmentLoadError {
                failure: SegmentLoadFailure::Type,
                error_code: 0
            })
        );
        assert_eq!(table, before);
    }
    // index zero in the ldt is distinct from a null gdt selector.
    let raw = record(0x0123_4567, 0x89ab_cdef);
    table.configure_raw(7, raw).unwrap();
    assert_eq!(table.raw_descriptor(4), Some(&raw));
    assert_eq!(table.raw_descriptor(3), None);
}

#[test]
fn raw_load_stages_exact_attributes_and_accessed_commit_without_flattening() {
    let mut table = SegmentDescriptorTable::empty();
    let raw = record(0x5678_1234, 0x9a50_f6bc);
    table.configure_raw(0x43, raw).unwrap();
    let state = SegmentState::default();
    let before = table.clone();
    let load = table
        .resolve_load(SegmentRegister::Fs, 0x43, 3, &state)
        .unwrap();
    assert_eq!(load.selector, 0x43);
    assert_eq!(load.base, 0x9abc_5678);
    assert_eq!(load.cache.bits(), 0xfa70_1234);
    assert_eq!(table, before);
    let mut context = SegmentContext::default();
    load.apply_context(&mut context, SegmentRegister::Fs);
    assert!(context.is_valid());
    assert_eq!(context.state.bases[4], 0x9abc_5678);
    assert_eq!(context.caches[4].bits(), 0xfa70_1234);
    table.commit_accessed(0x43);
    assert_eq!(
        table.raw_descriptor(0x43).unwrap().to_words(),
        (0x5678_1234, 0x9a50_f7bc)
    );
    assert_eq!(table.descriptor(0x43).unwrap().base, 0x9abc_5678);
    table
        .configure_raw(0x43, record(0x1111_0042, 0x2240_f022))
        .unwrap();
    assert_eq!(context.state.bases[4], 0x9abc_5678);
    assert_eq!(context.caches[4].bits(), 0xfa70_1234);
    assert_eq!(
        table
            .resolve_load(SegmentRegister::Fs, 0x43, 3, &state)
            .unwrap()
            .cache
            .limit(),
        0x42
    );
}

#[test]
fn verification_and_failed_resolution_preserve_every_raw_bit() {
    let mut table = SegmentDescriptorTable::empty();
    for (high, read, write, failure) in [
        (0x0040_7200, true, true, SegmentLoadFailure::NotPresent),
        (0x0040_f000, true, false, SegmentLoadFailure::Type),
        (0x0040_d200, false, false, SegmentLoadFailure::Privilege),
        (0x0040_f800, false, false, SegmentLoadFailure::Type),
    ] {
        let raw = record(0x0012_3456, high);
        table.configure_raw(0x43, raw).unwrap();
        let before = table.clone();
        assert_eq!(table.verify(0x43, 3, false), read);
        assert_eq!(table.verify(0x43, 3, true), write);
        let target = if high == 0x0040_f000 {
            SegmentRegister::Ss
        } else {
            SegmentRegister::Fs
        };
        assert_eq!(
            table
                .resolve_load(target, 0x43, 3, &SegmentState::default())
                .unwrap_err()
                .failure,
            failure
        );
        assert_eq!(table, before);
    }
    // readable conforming code is visible independently of dpl; presence is not queried.
    table.configure_raw(0x43, record(0, 0x0040_1e00)).unwrap();
    assert!(table.verify(0x43, 3, false));
    assert!(!table.verify(0x43, 3, true));
    assert_eq!(table.raw_descriptor(0x43).unwrap().descriptor_type(), 0xe);
}

#[test]
fn unsupported_profiles_remain_distinct_from_architectural_load_failures() {
    let mut table = SegmentDescriptorTable::empty();
    let state = SegmentState::default();
    for (target, low, high) in [
        (SegmentRegister::Ss, 0x0000_ffff, 0x0000_f200),
        (SegmentRegister::Fs, 0x0000_ffff, 0x0060_f200),
        (SegmentRegister::Ds, 0x0010_ffff, 0x0040_f200),
    ] {
        table.configure_raw(0x43, record(low, high)).unwrap();
        let before = table.clone();
        assert_eq!(
            table
                .resolve_load(target, 0x43, 3, &state)
                .unwrap_err()
                .failure,
            SegmentLoadFailure::UnsupportedProfile
        );
        assert_eq!(table, before);
    }
    let mut old = legacy(SegmentDescriptorKind::Data { writable: true });
    old.base = 0x10;
    table.configure(old).unwrap();
    assert_eq!(
        table
            .resolve_load(SegmentRegister::Ds, 0x43, 3, &state)
            .unwrap_err()
            .failure,
        SegmentLoadFailure::NonFlatLegacy
    );
    // architectural type/privilege/presence failures retain precedence over profile refusal.
    for (high, expected) in [
        (0x0060_f000, SegmentLoadFailure::Type),
        (0x0060_d200, SegmentLoadFailure::Privilege),
        (0x0060_7200, SegmentLoadFailure::NotPresent),
    ] {
        table.configure_raw(0x43, record(0xffff, high)).unwrap();
        assert_eq!(
            table
                .resolve_load(SegmentRegister::Ss, 0x43, 3, &state)
                .unwrap_err()
                .failure,
            expected
        );
    }
}

#[test]
fn base_updates_preserve_raw_authority_and_legacy_system_compatibility() {
    let mut table = SegmentDescriptorTable::empty();
    table
        .configure_raw(0x43, record(0x5678_1234, 0x9ad5_f6bc))
        .unwrap();
    table.update_base(0x43, 0x1122_3344).unwrap();
    assert_eq!(
        table.raw_descriptor(0x40).unwrap().to_words(),
        (0x3344_1234, 0x11d5_f622)
    );
    assert_eq!(table.descriptor(0x43).unwrap().base, 0x1122_3344);
    let old = legacy(SegmentDescriptorKind::System);
    table.configure(old).unwrap();
    table.update_base(0x43, 0x89ab_cdef).unwrap();
    let mut updated = old;
    updated.selector = 0x40;
    updated.base = 0x89ab_cdef;
    assert_eq!(table.descriptor(0x43), Some(&updated));
    assert_eq!(
        table.raw_descriptor(0x43).unwrap().to_words(),
        (0xcdef_0000, 0x8900_e0ab)
    );
}
