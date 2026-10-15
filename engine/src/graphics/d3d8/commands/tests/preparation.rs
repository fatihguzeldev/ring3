use super::*;

#[test]
fn prepared_group_is_neutral_until_commit_and_cannot_be_committed_after_other_commands() {
    let mut queue = queue(256);
    let prepared = queue
        .prepare(&[Command::CursorVisible(true), Command::CursorVisible(false)])
        .unwrap();
    assert_eq!(queue.seal(), Ok(None));
    queue.can_commit(&prepared).unwrap();
    queue.submit(&Command::CursorVisible(true)).unwrap();
    assert_eq!(
        queue.can_commit(&prepared),
        Err(CommandError::StalePreparation)
    );
    assert_eq!(queue.commit(prepared), Err(CommandError::StalePreparation));
    queue.seal().unwrap();
    let fragments: Vec<_> = packet(queue.peek().unwrap().1).unwrap().1.collect();
    assert_eq!(fragments.len(), 1);
    assert_eq!(
        (fragments[0].command, fragments[0].bytes),
        (1, &[1, 0, 0, 0][..])
    );
}

#[test]
fn prepared_group_preserves_logical_command_boundaries_across_batch_drains() {
    let mut queue = queue(128);
    let data = [8; 96];
    let prepared = queue
        .prepare(&[
            Command::CursorVisible(true),
            Command::Upload(Upload {
                resource: resource(2),
                subresource: 0,
                offset: 0,
                row_pitch: 96,
                slice_pitch: 96,
                row_bytes: 96,
                rows: 1,
                slices: 1,
                bytes: &data,
            }),
            Command::CursorVisible(false),
        ])
        .unwrap();
    assert_eq!(queue.commit(prepared), Ok(Progress::Draining));
    let mut commands = Vec::new();
    let mut completed = Vec::new();
    while let Some((id, bytes)) = queue.peek() {
        for fragment in packet(bytes).unwrap().1 {
            commands.push((fragment.command, fragment.opcode));
            if fragment.flags & 2 != 0 {
                completed.push(fragment.command);
            }
        }
        queue.acknowledge(id).unwrap();
    }
    assert_eq!(completed, [1, 2, 3]);
    assert_eq!(commands.first(), Some(&(1, 19)));
    assert_eq!(commands.last(), Some(&(3, 19)));
    assert!(!queue.is_draining());
}

#[test]
fn prepared_group_budget_charges_command_metadata_before_publication() {
    let mut queue = CommandQueue::new(
        7,
        CommandLimits {
            batch_bytes: 128,
            pending_bytes: 64,
        },
    )
    .unwrap();
    assert!(matches!(
        queue.prepare(&[Command::CursorVisible(true), Command::CursorVisible(false)]),
        Err(CommandError::Capacity)
    ));
    assert_eq!(queue.seal(), Ok(None));
    assert_eq!(
        queue.submit(&Command::CursorVisible(true)),
        Ok(Progress::Queued)
    );
}
