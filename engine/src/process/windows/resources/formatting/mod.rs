mod arguments;
mod output;
mod sources;

use super::{ResourceError32 as E, types::ResourceCallOutcome32 as Outcome};
use crate::{
    process::EngineInstance,
    windows::{resources::ResourceApi32 as Api, text::messages},
};

impl From<messages::MessageError32> for E {
    fn from(error: messages::MessageError32) -> Self {
        match error {
            messages::MessageError32::Memory(error) => Self::Memory(error),
            _ => Self::Win32(error.win32()),
        }
    }
}
impl EngineInstance {
    pub(super) fn resource_message(&mut self, api: Api, args: &[u32]) -> Result<Outcome, E> {
        let flags = args[0];
        if flags & !0x3fff != 0 || flags & 0x1c00 == 0 || flags & 0x400 != 0 && flags & 0x1800 != 0
        {
            return Err(E::InvalidParameter);
        }
        let source = self.message_source(flags, args[1], args[2], args[3], api.wide())?;
        let arguments = arguments::Arguments32::new(
            self.memory.as_ref().ok_or(E::NotConfigured)?,
            self.windows_ansi_code_page(),
            flags,
            args[6],
        )?;
        let formatted = messages::format_message(&source, flags, api.wide(), &arguments)?;
        self.publish_message(api.wide(), flags, args[4], args[5], &formatted)
    }
}
