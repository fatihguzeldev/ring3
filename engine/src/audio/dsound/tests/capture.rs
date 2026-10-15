use super::*;
#[test]
fn capture_receipts_are_exact_and_real_pcm_advances_a_wrapping_ring() {
    let mut capture = CaptureRing::new(format(), 4, 0, 16).unwrap();
    assert!(capture.request(1, 9).is_none());
    capture.start(1).unwrap();
    let request = capture.request(1, 9).unwrap();
    let plan = capture.prepare_write(request, 1, 9, &[1, 2, 3]).unwrap();
    capture.commit_write(plan, &[1, 2, 3]);
    assert_eq!(capture.position(), 3);
    assert_eq!(capture.bytes(), &[1, 2, 3, 128]);
    assert!(capture.prepare_write(request, 1, 9, &[4]).is_err());
    let request = capture.request(1, 9).unwrap();
    let plan = capture.prepare_write(request, 1, 9, &[4, 5]).unwrap();
    let progress = capture.progress(plan);
    assert_eq!(
        (
            progress.first_byte,
            progress.last_byte,
            progress.wrapped,
            progress.stopped
        ),
        (3, 1, true, false)
    );
    capture.commit_write(plan, &[4, 5]);
    assert_eq!(capture.bytes(), &[5, 2, 3, 4]);
    capture.stop().unwrap();
    capture.start(0).unwrap();
    assert!(capture.prepare_write(request, 1, 9, &[6]).is_err());
    let request = capture.request(1, 9).unwrap();
    let plan = capture
        .prepare_write(request, 1, 9, &[8, 9, 10, 11])
        .unwrap();
    assert!(capture.progress(plan).stopped);
    capture.commit_write(plan, &[8, 9, 10, 11]);
    assert_eq!(capture.status(), 0);
}

#[test]
fn partial_capture_lock_writes_and_failed_invalidation_preserve_other_bytes_and_identity() {
    let mut capture = CaptureRing::new(format(), 8, 0, 16).unwrap();
    capture.write_locked(6, &[1, 2], &[3, 4]).unwrap();
    assert_eq!(capture.bytes(), &[3, 4, 128, 128, 128, 128, 1, 2]);
    for (offset, first, second) in [
        (7, &[9, 9][..], &[][..]),
        (0, &[9][..], &[8][..]),
        (8, &[][..], &[][..]),
    ] {
        assert!(capture.write_locked(offset, first, second).is_err());
        assert_eq!(capture.bytes(), &[3, 4, 128, 128, 128, 128, 1, 2]);
    }
    capture.start(1).unwrap();
    capture.revision_for_test(u64::MAX - 1);
    let request = capture.request(1, 9);
    assert!(capture.invalidate().is_err());
    assert_eq!(capture.request(1, 9), request);
    assert_eq!(capture.status(), 3);
}

#[test]
fn capture_creation_flags_and_resource_budget_use_literal_desktop_contracts() {
    for flags in [0, 0x8000_0000, 0x200, 0x8000_0200] {
        assert!(CaptureRing::new(format(), 8, flags, 8).is_ok());
    }
    assert!(matches!(
        CaptureRing::new(format(), 8, 1, 8),
        Err(HResult::INVALID_PARAM)
    ));
    assert!(matches!(
        CaptureRing::new(format(), 8, 0, 4),
        Err(HResult::OUT_OF_MEMORY)
    ));
}
