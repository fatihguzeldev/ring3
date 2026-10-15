use super::*;
use crate::graphics::BatchId;

#[test]
fn oversized_upload_drains_complete_fragments_in_order_and_blocks_until_final_ack() {
    let mut queue = queue(160);
    let data: Vec<u8> = (0..256).map(|index| index as u8).collect();
    let command = Command::Upload(Upload {
        resource: resource(2),
        subresource: 0,
        offset: 0,
        row_pitch: 256,
        slice_pitch: 256,
        row_bytes: 256,
        rows: 1,
        slices: 1,
        bytes: &data,
    });
    assert!(queue.preflight(&command).unwrap().requires_drain());
    assert_eq!(queue.submit(&command), Ok(Progress::Draining));
    let mut payload = Vec::new();
    let mut sequence = 1;
    loop {
        let (id, bytes) = queue.peek().unwrap();
        assert_eq!(id.sequence, sequence);
        assert_eq!(
            queue.preflight(&Command::CursorVisible(true)),
            Err(CommandError::Backpressure)
        );
        let (header, fragments) = packet(bytes).unwrap();
        assert_eq!((header.first_command, header.last_command), (1, 1));
        let fragments: Vec<_> = fragments.collect();
        assert_eq!(fragments.len(), 1);
        let fragment = fragments[0];
        assert_eq!(
            (fragment.opcode, fragment.command, fragment.offset as usize),
            (3, 1, payload.len())
        );
        payload.extend_from_slice(fragment.bytes);
        let last = fragment.flags & 2 != 0;
        assert!(queue.is_draining());
        let progress = queue.acknowledge(id).unwrap();
        if last {
            assert_eq!(progress, Progress::Queued);
            break;
        }
        assert_eq!(progress, Progress::Draining);
        sequence += 1;
    }
    assert_eq!(sequence, 5);
    assert_eq!(payload.len(), 56 + data.len());
    assert_eq!(&payload[56..], data);
    assert!(!queue.is_draining());
    assert!(queue.peek().is_none());
    assert_eq!(
        queue.submit(&Command::CursorVisible(true)),
        Ok(Progress::Queued)
    );
}

#[test]
fn stale_ack_and_invalid_reset_preserve_sealed_packet_and_pending_upload() {
    let mut queue = queue(128);
    let bytes = [9u8; 96];
    queue
        .submit(&Command::Upload(Upload {
            resource: resource(2),
            subresource: 0,
            offset: 0,
            row_pitch: 96,
            slice_pitch: 96,
            row_bytes: 96,
            rows: 1,
            slices: 1,
            bytes: &bytes,
        }))
        .unwrap();
    let (id, packet) = queue.peek().unwrap();
    let original = packet.to_vec();
    assert_eq!(
        queue.acknowledge(BatchId {
            sequence: id.sequence + 1,
            ..id
        }),
        Err(CommandError::StaleBatch)
    );
    assert_eq!(queue.reset(id.epoch), Err(CommandError::InvalidEpoch));
    assert_eq!(queue.peek().unwrap().1, original);
    assert_eq!(queue.draining_command(), Some(1));
    queue.reset(id.epoch + 1).unwrap();
    assert!(!queue.is_draining());
    assert_eq!(queue.acknowledge(id), Err(CommandError::StaleBatch));
    queue.submit(&Command::CursorVisible(false)).unwrap();
    let next = queue.seal().unwrap().unwrap();
    assert_eq!((next.epoch, next.sequence), (id.epoch + 1, 1));
}

#[test]
fn invalid_upload_preflight_is_neutral_and_cannot_discard_existing_commands() {
    let mut queue = queue(256);
    queue.submit(&Command::CursorVisible(true)).unwrap();
    let invalid = Command::Upload(Upload {
        resource: resource(2),
        subresource: 0,
        offset: 0,
        row_pitch: 4,
        slice_pitch: 8,
        row_bytes: 3,
        rows: 2,
        slices: 1,
        bytes: &[1, 2, 3],
    });
    assert_eq!(queue.preflight(&invalid), Err(CommandError::InvalidCommand));
    assert_eq!(queue.submit(&invalid), Err(CommandError::InvalidCommand));
    queue.seal().unwrap();
    let (header, records) = packet(queue.peek().unwrap().1).unwrap();
    assert_eq!(
        (header.fragments, header.first_command, header.last_command),
        (1, 1, 1)
    );
    assert_eq!(
        records.map(|record| record.opcode).collect::<Vec<_>>(),
        [19]
    );
}
