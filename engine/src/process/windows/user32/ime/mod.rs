mod preferences;

use super::User32Context32;
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{imm32::ImmApi32 as Api, user32::User32Error32, window::WindowHandle32},
};

impl User32Context32 {
    pub(in crate::process) fn complete_imm32_call(
        &mut self,
        api: Api,
        args: &[u32],
        thread: u32,
        code_page: crate::windows::text::CodePage32,
        memory: &mut AddressSpace,
    ) -> Result<u32, User32Error32> {
        if let Some(value) = self.imm_preferences(api, args, thread, code_page, memory)? {
            return Ok(value);
        }
        match api {
            Api::ImmCreateContext => return Ok(self.ime.create_context(thread).unwrap_or(0)),
            Api::ImmDestroyContext => {
                return Ok(u32::from(self.ime.destroy_context(args[0], thread).is_ok()));
            }
            Api::ImmGetOpenStatus => {
                return Ok(self
                    .ime
                    .context(args[0])
                    .map_or(0, |context| u32::from(context.open)));
            }
            Api::ImmSetOpenStatus => {
                let Ok(context) = self.ime.context_mut(args[0], thread) else {
                    return Ok(0);
                };
                context.open = args[1] != 0;
                return Ok(1);
            }
            Api::ImmGetConversionStatus => {
                let Ok(context) = self.ime.context(args[0]) else {
                    return Ok(0);
                };
                let conversion = context.conversion.to_le_bytes();
                let sentence = context.sentence.to_le_bytes();
                let mut spans = Vec::new();
                spans
                    .try_reserve_exact(2)
                    .map_err(|_| User32Error32::Allocation)?;
                if args[1] != 0 {
                    spans.push((GuestAddress(args[1]), conversion.as_slice()));
                }
                if args[2] != 0 {
                    spans.push((GuestAddress(args[2]), sentence.as_slice()));
                }
                memory.write_spans(&spans)?;
                return Ok(1);
            }
            Api::ImmSetConversionStatus => {
                let Ok(context) = self.ime.context_mut(args[0], thread) else {
                    return Ok(0);
                };
                context.conversion = args[1];
                context.sentence = args[2];
                return Ok(1);
            }
            _ => {}
        }
        let Some(owner) = WindowHandle32::try_from(args[0])
            .ok()
            .and_then(|window| self.windows().get(window).ok())
            .map(|window| window.description.thread_id)
        else {
            return Ok(0);
        };
        Ok(match api {
            Api::ImmGetContext => self.ime.window_context(args[0], owner).unwrap_or(0),
            Api::ImmAssociateContext if owner == thread => {
                self.ime.associate(args[0], thread, args[1]).unwrap_or(0)
            }
            Api::ImmReleaseContext => u32::from(
                self.ime.context(args[1]).is_ok()
                    && self
                        .ime
                        .associated_context(args[0], owner)
                        .is_some_and(|context| context == args[1]),
            ),
            _ => 0,
        })
    }
}
