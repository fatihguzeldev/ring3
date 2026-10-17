use super::*;
use crate::{memory::GuestAddress, windows::window::WindowHandle32};
impl User32Context32 {
    pub(super) fn system_utility_call(
        &mut self,
        api: User32Api32,
        a: &[u32],
        m: &mut AddressSpace,
        c: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        use User32Api32::*;
        let value = match api {
            GetDesktopWindow => return self.desktop_window(),
            GetMenuCheckMarkDimensions => {
                let size = crate::windows::user32::MenuMetrics32::check_size();
                size.x as u32 | ((size.y as u32) << 16)
            }
            PostThreadMessageA => {
                if a[0] != c.thread_id {
                    return Err(User32Error32::Win32(1444));
                }
                return self.post_message(&[0, a[1], a[2], a[3]], c);
            }
            InSendMessage => return Err(User32Error32::Win32(120)),
            DefFrameProcA | DefMDIChildProcA => return Err(User32Error32::Win32(120)),
            TranslateMDISysAccel => return Err(User32Error32::Win32(120)),
            LockWindowUpdate => {
                if a[0] == 0 {
                    self.compatibility.locked = None;
                    1
                } else {
                    let h = WindowHandle32::try_from(a[0])?;
                    self.windows().get(h)?;
                    if self.compatibility.locked.is_some() {
                        return Err(User32Error32::Win32(5));
                    }
                    return Err(User32Error32::Win32(120));
                }
            }
            SystemParametersInfoA => {
                let bytes: Vec<u8> = match a[0] {
                    0x30 => {
                        let mut b = Vec::new();
                        for v in [
                            self.input.desktop.left,
                            self.input.desktop.top,
                            self.input.desktop.right,
                            self.input.desktop.bottom,
                        ] {
                            b.extend_from_slice(&v.to_le_bytes());
                        }
                        b
                    }
                    0x10 => 0u32.to_le_bytes().to_vec(),
                    0x46 => 1u32.to_le_bytes().to_vec(),
                    0x68 => 3u32.to_le_bytes().to_vec(),
                    _ => return Err(User32Error32::Win32(120)),
                };
                m.write(GuestAddress(a[2]), &bytes)?;
                1
            }
            UnhookWindowsHookEx => return Err(User32Error32::Win32(1404)),
            SetWindowsHookExA | CallNextHookEx | WinHelpA | GetSystemMenu | SetMenuItemBitmaps
            | BeginDeferWindowPos | DeferWindowPos | EndDeferWindowPos | DrawFocusRect
            | GrayStringA | GetTabbedTextExtentA | TabbedTextOutA | ScrollWindow
            | ScrollWindowEx | InvalidateRgn | RedrawWindow | MessageBeep | UnpackDDElParam
            | ReuseDDElParam => return Err(User32Error32::Win32(120)),
            _ => return Err(User32Error32::Win32(120)),
        };
        Ok(User32Outcome32::Return(value))
    }
}
