use super::Intent32;
use crate::{
    process::{
        EngineInstance, HostError,
        windows::user32::{User32CallContext32, User32Outcome32, memory},
    },
    windows::{
        user32::{
            User32Api32 as Api, User32Error32 as Error,
            clipboard::{ClipboardContent32 as Content, ClipboardOwner32},
        },
        window::WindowHandle32,
    },
};

impl EngineInstance {
    pub(in crate::process::windows::user32) fn execute_clipboard_call(
        &mut self,
        api: Api,
        args: &[u32],
        call: User32CallContext32,
    ) -> Result<User32Outcome32, HostError> {
        if args.len() != api.stack_words() as usize {
            return Err(HostError::User32(Error::InvalidRequest));
        }
        self.user32
            .as_mut()
            .ok_or(HostError::InvalidRequest)?
            .last_error = None;
        let value = match api {
            Api::OpenClipboard => {
                let window = if args[0] == 0 {
                    None
                } else {
                    let handle = WindowHandle32::try_from(args[0])
                        .map_err(Error::from)
                        .map_err(HostError::User32)?;
                    self.user32
                        .as_ref()
                        .unwrap()
                        .windows()
                        .get(handle)
                        .map_err(Error::from)
                        .map_err(HostError::User32)?;
                    Some(handle)
                };
                self.user32
                    .as_mut()
                    .unwrap()
                    .clipboard
                    .open(ClipboardOwner32 {
                        thread: call.thread_id,
                        window,
                    })
                    .map_err(HostError::User32)?;
                1
            }
            Api::CloseClipboard => {
                let locale = self.user32.as_ref().unwrap().input.layout.handle() & 0xffff;
                self.user32
                    .as_mut()
                    .unwrap()
                    .clipboard
                    .close(call.thread_id, locale)
                    .map_err(HostError::User32)?;
                1
            }
            Api::EmptyClipboard => {
                let context = self.user32.as_ref().unwrap();
                let open = context
                    .clipboard
                    .check_open(call.thread_id)
                    .map_err(HostError::User32)?;
                let owner = ClipboardOwner32 {
                    thread: open
                        .window
                        .and_then(|window| context.windows().get(window).ok())
                        .map_or(call.thread_id, |record| record.description.thread_id),
                    window: open.window,
                };
                let message = context
                    .clipboard
                    .owner()
                    .and_then(|owner| owner.window)
                    .filter(|&window| context.windows().get(window).is_ok())
                    .map(|window| [window.get(), 0x307, 0, 0]);
                return self
                    .user32
                    .as_mut()
                    .unwrap()
                    .begin_clipboard(Intent32::Empty(owner), call.thread_id, message)
                    .map_err(HostError::User32);
            }
            Api::GetClipboardData => {
                let context = self.user32.as_ref().unwrap();
                context
                    .clipboard
                    .check_open(call.thread_id)
                    .map_err(HostError::User32)?;
                let source = match context.clipboard.get(args[0]) {
                    Some(Content::Delayed) => Some(args[0]),
                    Some(Content::Synthesized {
                        source,
                        memory: None,
                    }) if context.clipboard.get(source) == Some(Content::Delayed) => Some(source),
                    _ => None,
                };
                if let Some(source) = source {
                    let message = context
                        .clipboard
                        .owner()
                        .and_then(|owner| owner.window)
                        .filter(|&window| context.windows().get(window).is_ok())
                        .map(|window| [window.get(), 0x305, source, 0]);
                    return self
                        .user32
                        .as_mut()
                        .unwrap()
                        .begin_clipboard(Intent32::Get(args[0]), call.thread_id, message)
                        .map_err(HostError::User32);
                }
                self.resolve_clipboard_data(args[0])?
            }
            Api::SetClipboardData => {
                return self.set_clipboard_data(args[0], args[1], call.thread_id);
            }
            Api::IsClipboardFormatAvailable => {
                u32::from(self.user32.as_ref().unwrap().clipboard.available(args[0]))
            }
            Api::CountClipboardFormats => {
                self.user32.as_ref().unwrap().clipboard.entries().len() as u32
            }
            Api::EnumClipboardFormats => {
                let context = self.user32.as_mut().unwrap();
                let next = context
                    .clipboard
                    .next(call.thread_id, args[0])
                    .map_err(HostError::User32)?;
                if next.is_none() {
                    context.last_error = Some(0);
                }
                next.unwrap_or(0)
            }
            Api::RegisterClipboardFormatA | Api::RegisterClipboardFormatW => {
                let name = memory::string(
                    self.memory.as_ref().ok_or(HostError::Closed)?,
                    args[0],
                    api.wide(),
                    255,
                    call.ansi,
                )
                .map_err(HostError::User32)?;
                self.user32
                    .as_mut()
                    .unwrap()
                    .clipboard
                    .register(&name)
                    .map_err(HostError::User32)?
            }
            Api::GetClipboardFormatNameA | Api::GetClipboardFormatNameW => {
                if args[2] == 0 {
                    return Err(HostError::User32(Error::Win32(122)));
                }
                if (args[2] as i32) < 0 {
                    return Err(HostError::User32(Error::Win32(87)));
                }
                let Some(name) = self
                    .user32
                    .as_ref()
                    .unwrap()
                    .clipboard
                    .registered_name(args[0])
                else {
                    return Err(HostError::User32(Error::Win32(87)));
                };
                memory::encode_text(
                    self.memory.as_mut().ok_or(HostError::Closed)?,
                    args[1],
                    args[2] as usize,
                    name,
                    api.wide(),
                    call,
                )
                .map_err(HostError::User32)?
            }
            Api::GetClipboardOwner => self
                .user32
                .as_ref()
                .unwrap()
                .clipboard
                .owner()
                .and_then(|owner| owner.window)
                .map_or(0, WindowHandle32::get),
            Api::GetOpenClipboardWindow => self
                .user32
                .as_ref()
                .unwrap()
                .clipboard
                .opened_by()
                .and_then(|owner| owner.window)
                .map_or(0, WindowHandle32::get),
            Api::GetClipboardSequenceNumber => self.user32.as_ref().unwrap().clipboard.sequence(),
            Api::GetPriorityClipboardFormat => {
                let count = args[1] as i32;
                if count < 0 || count > 65535 {
                    return Err(HostError::User32(Error::Win32(87)));
                }
                let mut formats = Vec::new();
                formats
                    .try_reserve_exact(count as usize)
                    .map_err(|_| HostError::User32(Error::Allocation))?;
                for index in 0..count as u32 {
                    let address = args[0]
                        .checked_add(index * 4)
                        .ok_or_else(|| HostError::User32(Error::Win32(87)))?;
                    formats.push(
                        memory::words::<1>(self.memory.as_ref().ok_or(HostError::Closed)?, address)
                            .map_err(HostError::User32)?[0],
                    );
                }
                self.user32.as_ref().unwrap().clipboard.priority(&formats) as u32
            }
            _ => return Err(HostError::User32(Error::InvalidRequest)),
        };
        Ok(User32Outcome32::Return(value))
    }
}
