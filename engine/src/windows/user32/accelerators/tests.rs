use super::*;
use crate::windows::user32::User32Error32 as E;

#[test]
fn resource_and_api_layouts_have_independent_width_padding_and_end_markers() {
    let resource = [
        9, 0, 0x41, 0, 0x34, 0x12, 0xab, 0xcd, 0x80, 0, 0xe9, 0, 0x78, 0x56, 0xef, 0x90,
    ];
    let parsed = Accelerator32::resource(&resource, 2).unwrap();
    assert_eq!(
        parsed,
        [
            Accelerator32 {
                flags: 9,
                key: 0x41,
                command: 0x1234
            },
            Accelerator32 {
                flags: 0,
                key: 0xe9,
                command: 0x5678
            },
        ]
    );
    let api = [9, 0xaa, 0x41, 0, 0x34, 0x12, 0, 0xbb, 0xe9, 0, 0x78, 0x56];
    assert_eq!(Accelerator32::api(&api, 2).unwrap(), parsed);
    assert_eq!(parsed[0].encode(), [9, 0, 0x41, 0, 0x34, 0x12]);
    assert_eq!(parsed[1].encode(), [0, 0, 0xe9, 0, 0x78, 0x56]);
    assert_eq!(Accelerator32::resource(&api, 2), Err(E::InvalidRequest));
    assert_eq!(Accelerator32::api(&resource, 2), Err(E::Win32(87)));
}

#[test]
fn malformed_tables_and_limits_do_not_truncate_a_valid_prefix_into_success() {
    let resource = [0x81, 0, 65, 0, 1, 0, 0, 0];
    for count in 0..resource.len() {
        assert_eq!(
            Accelerator32::resource(&resource[..count], 1),
            Err(E::InvalidRequest)
        );
    }
    assert_eq!(Accelerator32::resource(&resource, 0), Err(E::Capacity));
    let mut missing_end = resource;
    missing_end[0] = 1;
    assert_eq!(
        Accelerator32::resource(&missing_end, 1),
        Err(E::InvalidRequest)
    );
    let mut duplicate_end = resource.to_vec();
    duplicate_end.extend(resource);
    assert_eq!(
        Accelerator32::resource(&duplicate_end, 2),
        Err(E::InvalidRequest)
    );
    let mut invalid_flags = resource;
    invalid_flags[1] = 1;
    assert_eq!(
        Accelerator32::resource(&invalid_flags, 1),
        Err(E::InvalidRequest)
    );
}

fn entries() -> Vec<Accelerator32> {
    vec![Accelerator32 {
        flags: 1,
        key: 65,
        command: 9,
    }]
}

#[test]
fn repeated_resource_loads_share_identity_until_each_reference_is_destroyed() {
    let mut tables = AcceleratorStore32::new(1, 1).unwrap();
    let origin = ResourceKey32 {
        module: 1,
        entry: 0x1234,
    };
    let first = tables.create(entries(), Some(origin)).unwrap();
    assert_eq!(tables.retain_resource(origin), Ok(Some(first)));
    assert_eq!(tables.create(entries(), Some(origin)), Ok(first));
    assert_eq!(tables.destroy(first), Ok(false));
    assert_eq!(tables.destroy(first), Ok(false));
    assert_eq!(tables.entries(first).unwrap(), entries());
    assert_eq!(tables.destroy(first), Ok(true));
    assert_eq!(tables.entries(first), Err(E::Win32(1403)));
    assert_eq!(tables.retain_resource(origin), Ok(None));
    let fresh = tables
        .create(
            entries(),
            Some(ResourceKey32 {
                module: 2,
                ..origin
            }),
        )
        .unwrap();
    assert_ne!(first, fresh);
    assert_eq!(AcceleratorHandle32::try_from(fresh.get()), Ok(fresh));
}

#[test]
fn independently_created_tables_obey_aggregate_budget_and_never_alias_old_handles() {
    let mut tables = AcceleratorStore32::new(2, 1).unwrap();
    let first = tables.create(entries(), None).unwrap();
    assert_eq!(tables.create(entries(), None), Err(E::Capacity));
    assert_eq!(tables.entries(first).unwrap(), entries());
    assert_eq!(tables.destroy(first), Ok(true));
    let second = tables.create(entries(), None).unwrap();
    assert_ne!(first, second);
    assert_eq!(tables.destroy(first), Err(E::Win32(1403)));
    assert_eq!(tables.entries(second).unwrap(), entries());
}

#[test]
fn native_create_and_copy_preserve_the_uninterpreted_flag_bit() {
    let entry = Accelerator32 {
        flags: 0x41,
        key: 65,
        command: 7,
    };
    let decoded = Accelerator32::api(&[0x41, 0, 65, 0, 7, 0], 1).unwrap();
    assert_eq!(decoded, [entry]);
    let mut tables = AcceleratorStore32::new(1, 1).unwrap();
    let handle = tables.create(decoded, None).unwrap();
    assert_eq!(tables.entries(handle).unwrap(), [entry]);
    assert_eq!(
        tables.entries(handle).unwrap()[0].encode(),
        [0x41, 0, 65, 0, 7, 0]
    );
    assert_eq!(tables.destroy(handle), Ok(true));
}
