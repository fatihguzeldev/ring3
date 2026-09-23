use super::DispatchError;

#[derive(Clone, Copy)]
pub(super) enum Call {
    Initialize,
    Uninitialize,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x474 => Some(Self::Initialize),
            0x478 => Some(Self::Uninitialize),
            _ => None,
        }
    }

    pub(super) fn resolve(name: &str) -> Option<u32> {
        match name {
            "CoInitialize" => Some(0x474),
            "CoUninitialize" => Some(0x478),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Initialize => 1,
            Self::Uninitialize => 0,
        }
    }
}

#[derive(Default)]
pub(super) struct Com {
    references: u32,
}

impl Com {
    pub(super) fn dispatch(
        &mut self,
        call: Call,
        args: &[u32],
    ) -> Result<Option<u32>, DispatchError> {
        match call {
            Call::Initialize => {
                if args[0] != 0 {
                    return Ok(Some(0x8007_0057));
                }
                let result = u32::from(self.references != 0);
                self.references = self
                    .references
                    .checked_add(1)
                    .ok_or(DispatchError::Unsupported)?;
                Ok(Some(result))
            }
            Call::Uninitialize => {
                self.references = self.references.saturating_sub(1);
                Ok(None)
            }
        }
    }
}
