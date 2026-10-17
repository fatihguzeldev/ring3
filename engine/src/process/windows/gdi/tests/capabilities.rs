use super::{call, fixture};
use crate::windows::gdi::{GdiApi32 as Api, GdiError32};

#[test]
fn unavailable_recording_paths_curves_and_printing_do_not_publish_resources() {
    let (mut runtime, mut ram) = fixture();
    let dc = call(&mut runtime, &mut ram, Api::CreateCompatibleDC, &[0]).unwrap();
    let before = runtime.model.state(dc).unwrap().clone();
    let handles = (runtime.model.dc_count(), runtime.model.object_count());
    for (api, args) in [
        (Api::CreateMetaFileA, vec![0]),
        (Api::ArcTo, vec![dc, 0, 0, 10, 10, 0, 0, 10, 10]),
        (Api::PolyBezierTo, vec![dc, 0, 3]),
        (Api::PolyDraw, vec![dc, 0, 0, 0]),
        (Api::SetColorAdjustment, vec![dc, 0]),
        (Api::StartDocA, vec![dc, 0]),
        (Api::SetAbortProc, vec![dc, 0]),
        (Api::StartPage, vec![dc]),
        (Api::EndPage, vec![dc]),
        (Api::EndDoc, vec![dc]),
        (Api::AbortDoc, vec![dc]),
        (Api::Escape, vec![dc, 8, 0, 0, 0]),
    ] {
        assert_eq!(
            call(&mut runtime, &mut ram, api, &args),
            Err(GdiError32::Unsupported),
            "{api:?}"
        );
    }
    assert_eq!(
        call(&mut runtime, &mut ram, Api::SelectClipPath, &[dc, 5]),
        Err(GdiError32::NoPath)
    );
    assert_eq!(GdiError32::NoPath.win32(), 1003);
    assert_eq!(GdiError32::Unsupported.win32(), 50);
    for (api, args) in [
        (Api::CloseMetaFile, vec![dc]),
        (Api::DeleteMetaFile, vec![dc]),
        (Api::CopyMetaFileA, vec![dc, 0]),
        (Api::EnumMetaFile, vec![dc, dc, 0, 0]),
        (Api::PlayMetaFile, vec![dc, dc]),
    ] {
        assert_eq!(
            call(&mut runtime, &mut ram, api, &args),
            Err(GdiError32::InvalidHandle)
        );
    }
    assert_eq!(
        (runtime.model.dc_count(), runtime.model.object_count()),
        handles
    );
    assert_eq!(runtime.model.state(dc).unwrap(), &before);
}

#[test]
fn captured_unavailable_calls_deliver_documented_failure_and_thread_last_error() {
    use super::paint::{capture, eax, fixture_with_startup};
    let (mut engine, _, _) = fixture_with_startup(false);
    capture(&mut engine, Api::CreateCompatibleDC.id(), &[0]);
    let dc = eax(&engine);
    assert_ne!(dc, 0);
    for (api, args, value, error) in [
        (Api::CreateMetaFileA, vec![0], 0, 50),
        (Api::StartDocA, vec![dc, 0], 0, 50),
        (Api::SelectClipPath, vec![dc, 5], 0, 1003),
        (Api::DeleteMetaFile, vec![dc], 0, 6),
        (Api::GetNearestColor, vec![0, 0], u32::MAX, 6),
        (Api::SetMapperFlags, vec![dc, 2], u32::MAX, 87),
    ] {
        capture(&mut engine, api.id(), &args);
        assert_eq!(eax(&engine), value, "{api:?}");
        assert_eq!(engine.windows_thread.last_error(), error, "{api:?}");
    }
}
