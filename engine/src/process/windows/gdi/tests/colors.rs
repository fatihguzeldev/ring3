use super::{call, fixture, memory};
use crate::{
    memory::{GuestAddress, PageRange, Permissions},
    windows::gdi::{Brush32, GdiApi32, GdiError32, GdiObject32},
};

#[test]
fn system_brush_is_cached_immortal_and_consumes_no_guest_object_quota() {
    let (mut runtime, mut ram) = fixture();
    let before = runtime.model.object_count();
    let brush = call(&mut runtime, &mut ram, GdiApi32::GetSysColorBrush, &[15]).unwrap();
    assert!(brush > 31);
    assert_eq!(
        runtime.model.object(brush),
        Ok(GdiObject32::Brush(Brush32::System(15)))
    );
    for _ in 0..100 {
        assert_eq!(
            call(&mut runtime, &mut ram, GdiApi32::GetSysColorBrush, &[15]),
            Ok(brush)
        );
    }
    let dc = call(&mut runtime, &mut ram, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
    call(&mut runtime, &mut ram, GdiApi32::SelectObject, &[dc, brush]).unwrap();
    assert_eq!(
        call(&mut runtime, &mut ram, GdiApi32::DeleteObject, &[brush]),
        Ok(1)
    );
    assert_eq!(
        call(&mut runtime, &mut ram, GdiApi32::GetObjectType, &[brush]),
        Ok(2)
    );
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::GetObjectW,
            &[brush, 12, 0x10100]
        ),
        Ok(12)
    );
    assert_eq!(memory::words::<3>(&ram, 0x10100).unwrap(), [0, 0xc0c0c0, 0]);
    assert_eq!(runtime.model.object_count(), before);
    for index in [25, 31, u32::MAX] {
        assert_eq!(
            call(&mut runtime, &mut ram, GdiApi32::GetSysColor, &[index]),
            Ok(0)
        );
        assert_eq!(
            call(&mut runtime, &mut ram, GdiApi32::GetSysColorBrush, &[index]),
            Ok(0)
        );
    }
    ram.protect(
        PageRange::new(GuestAddress(0x10000), 1).unwrap(),
        Permissions::READ,
    )
    .unwrap();
    let version = ram.transaction_version();
    assert!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::GetObjectW,
            &[brush, 12, 0x10100]
        )
        .is_err()
    );
    assert_eq!(ram.transaction_version(), version);
    assert_eq!(runtime.model.object_count(), before);
    assert_eq!(
        call(&mut runtime, &mut ram, GdiApi32::GetObjectType, &[brush]),
        Ok(2)
    );
    assert!(runtime.model.dc_kind(dc).is_ok());
    assert_eq!(runtime.model.dc_kind(brush), Err(GdiError32::InvalidHandle));
    assert_eq!(runtime.model.object(31), Err(GdiError32::InvalidHandle));
}

#[test]
fn system_color_queries_are_user32_exports_and_cached_brush_draws_profile_pixels() {
    let user32 = crate::windows::user32::user32_module(0x71000000);
    let gdi32 = crate::windows::gdi::gdi32_module(0x72000000);
    for name in ["GetSysColor", "GetSysColorBrush"] {
        assert!(
            user32
                .exports
                .iter()
                .any(|entry| entry.name.as_deref() == Some(name))
        );
        assert!(
            !gdi32
                .exports
                .iter()
                .any(|entry| entry.name.as_deref() == Some(name))
        );
    }
    let (mut engine, _, window) = super::paint::fixture_with_startup(true);
    super::paint::capture(&mut engine, GdiApi32::GetSysColor.id(), &[15]);
    let color = super::paint::eax(&engine);
    assert_eq!(color, 0xc0c0c0);
    super::paint::capture(&mut engine, GdiApi32::GetSysColorBrush.id(), &[15]);
    let brush = super::paint::eax(&engine);
    super::paint::capture(&mut engine, GdiApi32::GetDC.id(), &[window]);
    let dc = super::paint::eax(&engine);
    memory::write_rect(
        engine.memory.as_mut().unwrap(),
        0x12200,
        crate::windows::user32::Rect32 {
            left: 0,
            top: 0,
            right: 8,
            bottom: 6,
        },
    )
    .unwrap();
    super::paint::capture(&mut engine, GdiApi32::FillRect.id(), &[dc, 0x12200, brush]);
    assert_eq!(super::paint::eax(&engine), 1);
    super::paint::capture(&mut engine, GdiApi32::GetPixel.id(), &[dc, 2, 3]);
    assert_eq!(super::paint::eax(&engine), color);
    super::paint::capture(&mut engine, GdiApi32::DeleteObject.id(), &[brush]);
    assert_eq!(super::paint::eax(&engine), 1);
    super::paint::capture(&mut engine, GdiApi32::GetSysColorBrush.id(), &[15]);
    assert_eq!(super::paint::eax(&engine), brush);
    super::paint::capture(
        &mut engine,
        crate::windows::WindowsApi32::SetLastError.id(),
        &[0xabcdef01],
    );
    super::paint::capture(&mut engine, GdiApi32::GetSysColorBrush.id(), &[u32::MAX]);
    assert_eq!(super::paint::eax(&engine), 0);
    super::paint::capture(
        &mut engine,
        crate::windows::WindowsApi32::GetLastError.id(),
        &[],
    );
    assert_eq!(super::paint::eax(&engine), 0xabcdef01);
}
