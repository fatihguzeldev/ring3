#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{FileContents, FileMetadata, LoadError, Process32, ProcessOptions};

fn load(files: &[FileMetadata<'_>], contents: &[FileContents<'_>]) -> Result<Process32, LoadError> {
    Process32::load_with_options(
        &imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["fopen"]),
        64,
        ProcessOptions {
            files,
            file_contents: contents,
            ..ProcessOptions::default()
        },
    )
}

#[test]
fn contents_require_unique_declared_names_and_exact_metadata_sizes() {
    let files = [
        FileMetadata {
            path: b"C:\\a",
            size: 1,
        },
        FileMetadata {
            path: b"C:\\empty",
            size: 0,
        },
    ];
    assert!(load(&files, &[]).is_ok());
    assert!(
        load(
            &files,
            &[
                FileContents {
                    path: b"c:\\A",
                    bytes: b"x"
                },
                FileContents {
                    path: b"C:\\empty",
                    bytes: b""
                }
            ]
        )
        .is_ok()
    );
    let oversized_path = vec![b'a'; 32768];
    for contents in [
        vec![FileContents {
            path: &oversized_path,
            bytes: b"x",
        }],
        vec![FileContents {
            path: b"C:\\absent",
            bytes: b"x",
        }],
        vec![FileContents {
            path: b"C:\\a",
            bytes: b"",
        }],
        vec![FileContents {
            path: b"C:\\empty",
            bytes: b"x",
        }],
        vec![
            FileContents {
                path: b"C:\\a",
                bytes: b"x",
            },
            FileContents {
                path: b"c:\\A",
                bytes: b"x",
            },
        ],
        vec![
            FileContents {
                path: b"C:\\a",
                bytes: b"x"
            };
            3
        ],
    ] {
        assert!(matches!(
            load(&files, &contents),
            Err(LoadError::InvalidProcessParameters)
        ));
    }
}

#[test]
fn aggregate_limit_is_checked_before_copying_any_file_bytes() {
    let bytes = vec![0; 4 * 1024 * 1024];
    let paths: Vec<_> = (0..257).map(|i| format!("C:\\file{i}")).collect();
    let files: Vec<_> = paths
        .iter()
        .map(|path| FileMetadata {
            path: path.as_bytes(),
            size: bytes.len() as u64,
        })
        .collect();
    let contents: Vec<_> = paths
        .iter()
        .map(|path| FileContents {
            path: path.as_bytes(),
            bytes: &bytes,
        })
        .collect();
    assert!(matches!(
        load(&files, &contents),
        Err(LoadError::FileContentsLimitExceeded)
    ));
}
