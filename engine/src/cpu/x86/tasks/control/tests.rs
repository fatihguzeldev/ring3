use super::*;

#[test]
fn live_controls_require_supplied_protected_mode_and_debug_authority() {
    for cr0 in [
        0x31,
        0x33,
        0x35,
        0x37,
        0x39,
        0x3b,
        0x3d,
        0x3f,
        0x8000_0031,
        0x0001_0031,
    ] {
        let state = TaskControlState32::new(cr0, 0xdead_b123, 0, 0x400, Some(true)).unwrap();
        assert_eq!(state.cr0(), cr0);
        assert_eq!(state.cr3(), 0xdead_b123);
        assert_eq!(state.paging_enabled(), cr0 >> 31 != 0);
        assert_eq!(state.nmi_blocked(), Some(true));
        assert_eq!(state.current_privilege(), 3);
    }
    for cr0 in [0, 0x30, 0x40031] {
        assert_eq!(
            TaskControlState32::new(cr0, 0, 0, 0, None),
            Err(TaskControlError32::UnsupportedControls)
        );
    }
    assert_eq!(
        TaskControlState32::new(0x31, 0, 4, 0, None),
        Err(TaskControlError32::InvalidLdtr)
    );
    for dr7 in [1, 2, 0x20, 0x80, 0x2000] {
        assert_eq!(
            TaskControlState32::new(0x31, 0, 0, dr7, None),
            Err(TaskControlError32::RequiresDebugAuthority)
        );
    }
}

#[test]
fn task_return_sets_ts_retains_pg0_cr3_clears_only_local_debug_and_unblocks_known_nmi() {
    let state = TaskControlState32 {
        cr0: 0x33,
        cr3: 0xdead_b123,
        ldtr: 0,
        dr7: 0x00ff_04ff,
        nmi_blocked: Some(true),
        cpl: 3,
    };
    let next = state.after_return(0x50, 3);
    assert_eq!(
        (next.cr0(), next.cr3(), next.ldtr(), next.dr7()),
        (0x3b, 0xdead_b123, 0x50, 0x00ff_04aa)
    );
    assert!(next.task_switched());
    assert!(next.monitor_coprocessor());
    assert_eq!(next.nmi_blocked(), Some(false));
    let unknown = TaskControlState32::new(0x31, 123, 0, 0, None)
        .unwrap()
        .after_return(0, 3);
    assert_eq!(unknown.nmi_blocked(), None);
}
