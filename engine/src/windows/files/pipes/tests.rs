use super::*;
use crate::windows::security::{SecurityDescriptor32, Token32};
fn context() -> PipeContext32 {
    PipeContext32::new(PipeLimits32 {
        instances: 4,
        endpoints: 8,
        bytes: 128,
        messages: 8,
    })
    .unwrap()
}
fn security() -> SecurityDescriptor32 {
    SecurityDescriptor32::for_token(&Token32::virtual_process()).unwrap()
}
fn named(context: &mut PipeContext32, message: bool) -> PipeEndpointId32 {
    let prepared = context
        .prepare_server(
            &"\\\\.\\pipe\\ring3".encode_utf16().collect::<Vec<_>>(),
            PipeOptions32 {
                message,
                read_message: message,
                in_buffer: 8,
                out_buffer: 8,
                ..Default::default()
            },
            security(),
            &Token32::virtual_process(),
        )
        .unwrap();
    context.commit_create(prepared).0
}
fn connect_client(context: &mut PipeContext32) -> PipeEndpointId32 {
    let prepared = context
        .prepare_client(
            &"\\\\.\\PIPE\\RING3".encode_utf16().collect::<Vec<_>>(),
            0xc0000000,
            true,
            &Token32::virtual_process(),
        )
        .unwrap();
    assert_ne!(prepared.granted_access() & 3, 0);
    context.commit_client(prepared)
}
fn write_data(context: &mut PipeContext32, endpoint: PipeEndpointId32, bytes: &[u8]) {
    let PipeIo32::Ready(prepared) = context.prepare_write(endpoint, bytes).unwrap() else {
        panic!("bounded fixture write must be ready")
    };
    assert_eq!(prepared.length(), bytes.len());
    context.commit_write(prepared);
}
fn read(context: &mut PipeContext32, endpoint: PipeEndpointId32, length: usize) -> (Vec<u8>, bool) {
    let PipeIo32::Ready(prepared) = context.prepare_read(endpoint, length).unwrap() else {
        panic!("bounded fixture read must be ready")
    };
    let result = (prepared.bytes().to_vec(), prepared.more());
    context.commit_read(prepared);
    result
}
#[test]
fn message_reads_peek_and_byte_mode_preserve_packet_boundaries_and_literal_data() {
    let mut context = context();
    let server = named(&mut context, true);
    let client = connect_client(&mut context);
    assert_eq!(
        context.connection(server).unwrap(),
        PipeConnection32::Connected
    );
    write_data(&mut context, client, b"hello");
    assert_eq!(read(&mut context, server, 2), (b"he".to_vec(), true));
    let revision = context.revision;
    assert_eq!(context.peek(server, 1).unwrap(), (b"l".to_vec(), 3, 2));
    assert_eq!(context.revision, revision);
    assert_eq!(read(&mut context, server, 8), (b"llo".to_vec(), false));
    write_data(&mut context, client, b"ab");
    write_data(&mut context, client, b"cd");
    context.set_mode(server, 0).unwrap();
    assert_eq!(read(&mut context, server, 3), (b"abc".to_vec(), false));
    context.set_mode(server, 2).unwrap();
    assert_eq!(read(&mut context, server, 8), (b"d".to_vec(), false));
    assert!(!context.options(client).unwrap().read_message);
    assert!(context.options(server).unwrap().message);
    write_data(&mut context, server, b"response");
    assert_eq!(read(&mut context, client, 8), (b"response".to_vec(), false));
}
#[test]
fn disconnected_client_identity_cannot_write_into_reconnected_instance_and_close_drains_old_bytes()
{
    let mut context = context();
    let server = named(&mut context, false);
    let old = connect_client(&mut context);
    write_data(&mut context, old, b"discard");
    context.disconnect(server).unwrap();
    assert!(matches!(
        context.prepare_write(old, b"wrong"),
        Err(FileError32::PipeNotConnected)
    ));
    context.listen(server).unwrap();
    let current = connect_client(&mut context);
    assert_ne!(old, current);
    context.release(old).unwrap();
    write_data(&mut context, current, b"retained");
    context.release(current).unwrap();
    assert_eq!(read(&mut context, server, 8), (b"retained".to_vec(), false));
    assert!(matches!(
        context.prepare_read(server, 1),
        Err(FileError32::BrokenPipe)
    ));
    context.release(server).unwrap();
    assert_eq!(context.reserved, 0);
    assert!(context.instances.iter().all(Option::is_none));
}
#[test]
fn anonymous_ring_wrap_duplicate_lifetime_and_bound_failures_do_not_lose_bytes() {
    let mut context = context();
    let prepared = context.prepare_anonymous(4, security()).unwrap();
    let (reader, writer) = context.commit_create(prepared);
    let writer = writer.unwrap();
    context.retain(writer).unwrap();
    write_data(&mut context, writer, b"abcd");
    assert_eq!(read(&mut context, reader, 2), (b"ab".to_vec(), false));
    write_data(&mut context, writer, b"ef");
    context.release(writer).unwrap();
    assert_eq!(read(&mut context, reader, 4), (b"cdef".to_vec(), false));
    assert!(matches!(
        context.prepare_read(reader, 1).unwrap(),
        PipeIo32::Pending
    ));
    context.release(writer).unwrap();
    assert!(matches!(
        context.prepare_read(reader, 1),
        Err(FileError32::BrokenPipe)
    ));
    let revision = context.revision;
    assert!(matches!(
        context.prepare_anonymous(129, security()),
        Err(FileError32::Capacity)
    ));
    assert_eq!(context.revision, revision);
    context.release(reader).unwrap();
    assert_eq!(context.reserved, 0);
}

#[test]
fn nonblocking_writes_are_short_byte_writes_or_successful_zero_message_writes() {
    let mut context = context();
    let server = named(&mut context, false);
    let client = connect_client(&mut context);
    context.set_mode(client, 1).unwrap();
    write_data(&mut context, client, b"123456");
    let PipeIo32::Ready(write) = context.prepare_write(client, b"abcd").unwrap() else {
        panic!()
    };
    assert_eq!(write.length(), 2);
    context.commit_write(write);
    assert_eq!(read(&mut context, server, 8), (b"123456ab".to_vec(), false));
    context.release(client).unwrap();
    context.release(server).unwrap();
    let server = named(&mut context, true);
    let client = connect_client(&mut context);
    context.set_mode(client, 1).unwrap();
    write_data(&mut context, client, b"123456");
    let PipeIo32::Ready(write) = context.prepare_write(client, b"abcd").unwrap() else {
        panic!()
    };
    assert_eq!(write.length(), 0);
    context.commit_write(write);
    assert_eq!(read(&mut context, server, 8), (b"123456".to_vec(), false));
    assert!(matches!(
        context.prepare_read(server, 1).unwrap(),
        PipeIo32::Pending
    ));
}

#[test]
fn shared_named_pipe_security_updates_all_instances_but_keeps_existing_grants() {
    let mut context = context();
    let token = Token32::virtual_process();
    let name: Vec<u16> = r"\\.\pipe\acl".encode_utf16().collect();
    let options = PipeOptions32 {
        maximum: 2,
        in_buffer: 8,
        out_buffer: 8,
        ..Default::default()
    };
    let first = context
        .prepare_server(&name, options, security(), &token)
        .unwrap();
    let first = context.commit_create(first).0;
    let second = context
        .prepare_server(&name, options, security(), &token)
        .unwrap();
    let second = context.commit_create(second).0;
    let client = context
        .prepare_client(&name, 0xc0000000, false, &token)
        .unwrap();
    let client = context.commit_client(client);
    let mut denied = security();
    denied.dacl = crate::windows::security::Dacl32::Acl(crate::windows::security::Acl32 {
        revision: 2,
        aces: Vec::new(),
    });
    let update = context
        .prepare_security(first, 4, denied, &token, Some(0x40000))
        .unwrap();
    context.commit_security(update);
    assert_eq!(
        context.security(first).unwrap(),
        context.security(second).unwrap()
    );
    assert!(matches!(
        context.prepare_client(&name, 1, false, &token),
        Err(crate::files::FileError32::AccessDenied)
    ));
    write_data(&mut context, client, b"ok");
    assert_eq!(read(&mut context, first, 2).0, b"ok");
    assert!(matches!(
        context.prepare_security(first, 4, security(), &token, Some(0x20000)),
        Err(crate::files::FileError32::AccessDenied)
    ));
    let queried = context
        .query_security(first, 4, &token, Some(0x20000))
        .unwrap();
    assert_eq!(queried.owner, None);
}

#[test]
fn zero_byte_reads_wait_for_data_preserve_buffered_bytes_then_report_drained_peer_eof() {
    let mut context = context();
    let server = named(&mut context, false);
    assert!(matches!(context.peek(server, 0), Err(FileError32::BadPipe)));
    assert!(matches!(
        context.prepare_read(server, 0),
        Err(FileError32::PipeListening)
    ));
    let client = connect_client(&mut context);
    assert!(!context.read_ready(server));
    assert!(matches!(
        context.prepare_read(server, 0),
        Ok(PipeIo32::Pending)
    ));
    write_data(&mut context, client, b"x");
    assert!(context.read_ready(server));
    assert_eq!(read(&mut context, server, 0).0, b"");
    assert_eq!(read(&mut context, server, 1).0, b"x");
    write_data(&mut context, client, b"y");
    context.release(client).unwrap();
    assert_eq!(read(&mut context, server, 0).0, b"");
    assert_eq!(read(&mut context, server, 1).0, b"y");
    assert!(matches!(
        context.prepare_read(server, 0),
        Err(FileError32::BrokenPipe)
    ));
    assert!(matches!(
        context.prepare_write(server, b"z"),
        Err(FileError32::NoData)
    ));
}
#[test]
fn disconnected_server_peek_and_old_client_peek_keep_their_distinct_win32_errors() {
    let mut context = context();
    let server = named(&mut context, false);
    let client = connect_client(&mut context);
    context.disconnect(server).unwrap();
    assert_eq!(context.peek(server, 0).err(), Some(FileError32::BadPipe));
    assert_eq!(
        context.peek(client, 0).err(),
        Some(FileError32::PipeNotConnected)
    );
    assert_eq!(FileError32::BadPipe.win32(), 230);
    assert_eq!(FileError32::PipeNotConnected.win32(), 233);
}
