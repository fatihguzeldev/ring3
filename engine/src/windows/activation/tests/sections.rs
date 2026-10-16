use super::*;

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
#[test]
fn section_headers_dll_record_and_com_progid_aliases_have_literal_windows_offsets() {
    let graph = graph("Ring3.Sections");
    let sections = encode_activation_sections(&graph, 0x58000004, 65536).unwrap();
    let dll = sections.iter().find(|section| section.id == 2).unwrap();
    assert_eq!(&dll.bytes[..8], &[0x53, 0x73, 0x48, 0x64, 44, 0, 0, 0]);
    assert_eq!(word(&dll.bytes, 20), 1);
    assert_eq!(word(&dll.bytes, 24), 44);
    let entry = dll
        .find(&ActivationSectionKey32::String(
            "PRIVATE.DLL".encode_utf16().collect(),
        ))
        .unwrap();
    assert_eq!(entry.length, 20);
    assert_eq!(
        &dll.bytes[entry.offset as usize..entry.offset as usize + 20],
        &[20, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    let com = sections.iter().find(|section| section.id == 4).unwrap();
    assert_eq!(&com.bytes[..8], &[0x47, 0x73, 0x48, 0x64, 40, 0, 0, 0]);
    assert_eq!(word(&com.bytes, 20), 2);
    let class = com.find(&ActivationSectionKey32::Guid(CLSID)).unwrap();
    let offset = class.offset as usize;
    assert_eq!(word(&com.bytes, offset), 120);
    assert_eq!(word(&com.bytes, offset + 8), 4);
    assert_eq!(&com.bytes[offset + 12..offset + 28], &CLSID);
    assert_eq!(&com.bytes[offset + 44..offset + 60], &CLSID);
    let alias: [u8; 16] = com.bytes[offset + 28..offset + 44].try_into().unwrap();
    assert_ne!(alias, CLSID);
    assert_eq!(
        com.find(&ActivationSectionKey32::Guid(alias))
            .unwrap()
            .offset,
        class.offset
    );
    let progid = sections.iter().find(|section| section.id == 7).unwrap();
    let entry = progid
        .find(&ActivationSectionKey32::String(
            "ring3.component.1".encode_utf16().collect(),
        ))
        .unwrap();
    let guid_offset = word(&progid.bytes, entry.offset as usize + 8) as usize;
    assert_eq!(&progid.bytes[guid_offset..guid_offset + 16], &alias);
    assert_eq!(progid.global_length, 16);
    assert_eq!(entry.roster, 1);
}

#[test]
fn section_budget_failure_does_not_publish_or_consume_context_identity() {
    let mut contexts = ActivationContexts32::new(ActivationConfig32::default()).unwrap();
    let first = contexts
        .prepare_create(graph("Ring3.Budget"), false)
        .unwrap();
    let handle = first.handle();
    let sections = encode_activation_sections(first.graph(), handle, 65536).unwrap();
    let length = sections
        .iter()
        .map(|section| section.bytes.len())
        .sum::<usize>();
    assert!(matches!(
        encode_activation_sections(first.graph(), handle, length - 1),
        Err(ActivationError32::NotEnoughMemory)
    ));
    assert!(!contexts.contains(handle));
    let retry = contexts
        .prepare_create(graph("Ring3.Budget"), false)
        .unwrap();
    assert_eq!(retry.handle(), handle);
    contexts.commit_create(retry);
    let cookie = activate(&mut contexts, 1, handle);
    let com = sections.iter().find(|section| section.id == 4).unwrap();
    let entry = com.find(&ActivationSectionKey32::Guid(CLSID)).unwrap();
    let alias = com.bytes[entry.offset as usize + 28..entry.offset as usize + 44]
        .try_into()
        .unwrap();
    assert_eq!(
        contexts
            .active_com_redirection(1, alias)
            .unwrap()
            .unwrap()
            .clsid,
        CLSID
    );
    let pop = contexts.prepare_deactivate(1, 0, cookie).unwrap();
    contexts.commit_deactivate(pop);
}
