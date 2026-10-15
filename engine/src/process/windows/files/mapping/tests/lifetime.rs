use super::returned;
use crate::{
    files::{FileAccess32, FileError32},
    memory::GuestAddress,
    process::windows::files::{handles::KernelFileObject32, tests::support::*},
    windows::files::FileApi32 as Api,
};

#[test]
fn named_section_loses_name_after_last_handle_while_existing_view_survives() {
    let (mut files, mut memory, _) = runtime();
    path(&mut memory, 0x1000, "shared");
    let section = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::CreateFileMappingW,
        &[u32::MAX, 0, 4, 0, 4096, 0x1000],
    ));
    let KernelFileObject32::Mapping(id) = files.handles.object(section).unwrap() else {
        unreachable!()
    };
    let view = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::MapViewOfFile,
        &[section, 2, 0, 0, 0],
    ));
    memory.write(GuestAddress(view), &[42]).unwrap();
    let reopened =
        returned(files.execute_mapping(&mut memory, &[], Api::OpenFileMappingW, &[4, 0, 0x1000]));
    returned(files.close_mapping(&mut memory, id));
    files.handles.remove(section).unwrap();
    returned(files.close_mapping(&mut memory, id));
    files.handles.remove(reopened).unwrap();
    assert_eq!(read::<1>(&memory, view), [42]);
    assert!(matches!(
        files.execute_mapping(&mut memory, &[], Api::OpenFileMappingW, &[4, 0, 0x1000]),
        Err(FileError32::NotFound)
    ));
    let replacement = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::CreateFileMappingW,
        &[u32::MAX, 0, 4, 0, 4096, 0x1000],
    ));
    let reader = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::MapViewOfFile,
        &[replacement, 4, 0, 0, 0],
    ));
    assert_eq!(read::<1>(&memory, reader), [0]);
    returned(files.execute_mapping(&mut memory, &[], Api::UnmapViewOfFile, &[view]));
}

#[test]
fn final_release_flushes_dirty_shared_bytes_after_last_mapping_handle_was_closed() {
    let (mut files, mut memory, _) = runtime();
    let file = open(&mut files, "C:\\final.bin", FileAccess32::ALL);
    write_file(&mut files, file, &[1, 2]);
    let section = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::CreateFileMappingW,
        &[file, 0, 4, 0, 0, 0],
    ));
    let KernelFileObject32::Mapping(id) = files.handles.object(section).unwrap() else {
        unreachable!()
    };
    let view = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::MapViewOfFile,
        &[section, 2, 0, 0, 0],
    ));
    memory.write(GuestAddress(view + 1), &[7]).unwrap();
    returned(files.close_mapping(&mut memory, id));
    files.handles.remove(section).unwrap();
    assert_eq!(file_bytes(&mut files, file), [1, 2]);
    returned(files.execute_mapping(&mut memory, &[], Api::UnmapViewOfFile, &[view]));
    assert_eq!(file_bytes(&mut files, file), [1, 7]);
}
