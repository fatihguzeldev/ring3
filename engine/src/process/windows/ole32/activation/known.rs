use super::super::memory::failure;
use crate::{
    process::EngineInstance,
    windows::{
        com::InterfaceId,
        ole32::{HResult32, KnownClass32},
    },
};
impl EngineInstance {
    pub(in crate::process::windows::ole32) fn activate_known_com_class(
        &mut self,
        class: KnownClass32,
        iid: InterfaceId,
        out: u32,
    ) -> Result<u32, HResult32> {
        let gates = self.module_gates().map_err(|_| HResult32::UNEXPECTED)?;
        let memory = self.memory.as_mut().ok_or(HResult32::UNEXPECTED)?;
        let result = match class {
            KnownClass32::DirectInput | KnownClass32::DirectInput8 => {
                if let Some(input) = self.dinput.as_mut() {
                    input
                        .activate_class(
                            memory,
                            &gates,
                            class == KnownClass32::DirectInput8,
                            iid,
                            out,
                        )
                        .map_err(|error| HResult32(error.0))
                } else {
                    Err(HResult32::CLASS_NOT_REGISTERED)
                }
            }
            _ => {
                if let (Some(sound), Some(media)) = (self.dsound.as_mut(), self.media.as_mut()) {
                    sound
                        .activate_class(memory, &mut media.audio, &gates, class, iid, out)
                        .map_err(|error| HResult32(error.0))
                } else {
                    Err(HResult32::CLASS_NOT_REGISTERED)
                }
            }
        };
        match result {
            Ok(value) => Ok(value),
            Err(error) => failure(memory, out, error),
        }
    }
}
