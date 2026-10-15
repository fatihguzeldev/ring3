use super::*;
use crate::memory::GuestAddress;

fn description(thread_id: u32) -> WindowDescription32 {
    WindowDescription32 {
        thread_id,
        procedure: GuestAddress(0x4000 + thread_id * 16),
        extent: WindowExtent32 {
            width: 800,
            height: 600,
        },
        visible: true,
        enabled: true,
    }
}

fn messages(context: &mut WindowContext32) -> Vec<(u32, u32, u32, u32)> {
    let mut result = Vec::new();
    while let Some(message) = context.pop_message() {
        result.push((
            message.window.get(),
            message.message,
            message.wparam,
            message.lparam,
        ));
    }
    result
}

#[test]
fn focus_and_capture_changes_preserve_windows_message_identity_and_order() {
    let mut windows = WindowContext32::new(3, 16).unwrap();
    let first = windows.create(description(1)).unwrap();
    let second = windows.create(description(1)).unwrap();
    assert_eq!(windows.set_focus(1, Some(first)), Ok(None));
    assert_eq!(windows.set_capture(1, Some(first)), Ok(None));
    assert_eq!(windows.set_focus(1, Some(second)), Ok(Some(first)));
    assert_eq!(windows.set_capture(1, Some(second)), Ok(Some(first)));
    assert_eq!(windows.focus(), Some(second));
    assert_eq!(windows.capture(), Some(second));
    assert_eq!(
        messages(&mut windows),
        vec![
            (first.get(), 7, 0, 0),
            (first.get(), 8, second.get(), 0),
            (second.get(), 7, first.get(), 0),
            (first.get(), 0x215, 0, second.get()),
        ]
    );
    assert_eq!(windows.set_focus(1, Some(second)), Ok(Some(second)));
    assert_eq!(windows.set_capture(1, Some(second)), Ok(Some(second)));
    assert_eq!(windows.queued_messages(), 0);
}

#[test]
fn queue_exhaustion_preserves_extent_focus_capture_and_window_lifetime() {
    let mut windows = WindowContext32::new(2, 3).unwrap();
    let first = windows.create(description(1)).unwrap();
    let second = windows.create(description(1)).unwrap();
    windows.set_focus(1, Some(first)).unwrap();
    windows.set_capture(1, Some(first)).unwrap();
    windows
        .resize(
            first,
            WindowExtent32 {
                width: 1024,
                height: 768,
            },
        )
        .unwrap();
    let before = windows.get(first).unwrap();
    assert_eq!(
        windows.check_set_focus(1, Some(second)),
        Err(WindowError32::QueueFull)
    );
    assert_eq!(
        windows.check_destroy(1, first),
        Err(WindowError32::QueueFull)
    );
    assert_eq!(
        windows.set_focus(1, Some(second)),
        Err(WindowError32::QueueFull)
    );
    assert_eq!(windows.destroy(1, first), Err(WindowError32::QueueFull));
    assert_eq!(windows.focus(), Some(first));
    assert_eq!(windows.capture(), Some(first));
    assert_eq!(windows.get(first), Ok(before));
    assert_eq!(windows.queued_messages(), 2);
    windows
        .resize(
            first,
            WindowExtent32 {
                width: 1280,
                height: 720,
            },
        )
        .unwrap();
    assert_eq!(
        windows.resize(
            first,
            WindowExtent32 {
                width: 1920,
                height: 1080
            }
        ),
        Err(WindowError32::QueueFull)
    );
    assert_eq!(
        windows.get(first).unwrap().description.extent,
        WindowExtent32 {
            width: 1280,
            height: 720
        }
    );
}

#[test]
fn destruction_releases_focus_capture_and_never_reuses_stale_handles() {
    let mut windows = WindowContext32::new(1, 8).unwrap();
    let first = windows.create(description(1)).unwrap();
    windows.set_focus(1, Some(first)).unwrap();
    windows.set_capture(1, Some(first)).unwrap();
    messages(&mut windows);
    let before = windows.get(first).unwrap();
    assert_eq!(windows.destroy(1, first), Ok(before));
    assert_eq!(windows.focus(), None);
    assert_eq!(windows.capture(), None);
    let first_message = windows.peek_message().unwrap();
    assert_eq!(first_message.procedure, before.description.procedure);
    assert_eq!(
        messages(&mut windows),
        vec![
            (first.get(), 0x215, 0, 0),
            (first.get(), 8, 0, 0),
            (first.get(), 2, 0, 0),
            (first.get(), 0x82, 0, 0),
        ]
    );
    assert_eq!(windows.get(first), Err(WindowError32::InvalidHandle));
    let second = windows.create(description(1)).unwrap();
    assert_ne!(first, second);
    assert_eq!(
        windows.set_focus(1, Some(first)),
        Err(WindowError32::InvalidHandle)
    );
    assert_eq!(windows.get(second).unwrap().description.extent.width, 800);
}

#[test]
fn malformed_requests_and_foreign_threads_are_neutral() {
    let mut windows = WindowContext32::new(3, 8).unwrap();
    assert_eq!(
        windows.create(description(0)),
        Err(WindowError32::InvalidThread)
    );
    assert_eq!(
        windows.create(WindowDescription32 {
            extent: WindowExtent32 {
                width: u32::MAX,
                height: 1
            },
            ..description(1)
        }),
        Err(WindowError32::InvalidExtent)
    );
    let first = windows.create(description(1)).unwrap();
    let foreign = windows.create(description(2)).unwrap();
    let disabled = windows
        .create(WindowDescription32 {
            enabled: false,
            ..description(1)
        })
        .unwrap();
    assert_eq!(
        windows.set_focus(1, Some(foreign)),
        Err(WindowError32::InvalidThread)
    );
    assert_eq!(
        windows.set_capture(1, Some(foreign)),
        Err(WindowError32::InvalidThread)
    );
    assert_eq!(
        windows.destroy(1, foreign),
        Err(WindowError32::InvalidThread)
    );
    assert_eq!(
        windows.set_focus(1, Some(disabled)),
        Err(WindowError32::Disabled)
    );
    assert_eq!(
        windows.resize(
            first,
            WindowExtent32 {
                width: 1,
                height: u32::MAX
            }
        ),
        Err(WindowError32::InvalidExtent)
    );
    assert_eq!(windows.focus(), None);
    assert_eq!(windows.capture(), None);
    assert_eq!(windows.queued_messages(), 0);
    assert_eq!(windows.live_windows(), 3);
}
