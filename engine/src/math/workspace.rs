use astro_float_num::Consts;

use super::NumericalError;

#[derive(Default)]
pub(crate) struct NumericalWorkspace {
    constants: Option<Consts>,
}

impl NumericalWorkspace {
    pub(crate) fn constants(&mut self) -> Result<&mut Consts, NumericalError> {
        if self.constants.is_none() {
            self.constants = Some(Consts::new().map_err(|_| NumericalError::NumericalFailure)?);
        }
        Ok(self.constants.as_mut().expect("initialized constants"))
    }
}
