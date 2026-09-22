#[path = "support/file_stream_cases.rs"]
mod file_stream_cases;
#[path = "support/imported_executable.rs"]
mod imported_executable;

#[test]
fn imported_streams_read_supplied_bytes_and_release_the_owned_record() {
    file_stream_cases::verify();
}
