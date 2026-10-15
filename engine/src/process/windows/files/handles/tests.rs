use super::*;

#[test]
fn handle_namespace_and_retired_identity_are_independent_of_event_handles() {
    let mut handles = KernelFileHandles32::new(1).unwrap();
    let prepared = handles.prepare(3, true).unwrap();
    let first = handles.commit(prepared, KernelFileObject32::File(OpenFileId(7)));
    assert_eq!(first, 0x8000_0000);
    assert!(handles.prepare(0, false).is_err());
    assert_eq!(handles.file(first), Ok(OpenFileId(7)));
    assert_eq!(handles.access(first), Ok(3));
    assert_eq!(handles.inherit(first), Ok(true));
    handles.remove(first).unwrap();
    let prepared = handles.prepare(1, false).unwrap();
    let second = handles.commit(prepared, KernelFileObject32::Mapping(9));
    assert_ne!(first, second);
    assert_eq!(handles.object(first), Err(FileError32::InvalidHandle));
    assert_eq!(handles.file(second), Err(FileError32::InvalidHandle));
    assert_eq!(handles.object(0x1000_0000), Err(FileError32::InvalidHandle));
}
