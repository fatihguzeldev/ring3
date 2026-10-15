use super::{
    execute::X87Error,
    value::{Ext80, Rounded},
};

pub(super) use crate::math::extended::transcendental::Function;

pub(super) fn evaluate(
    function: Function,
    x: Ext80,
    y: Ext80,
    control: u16,
    workspace: &mut crate::math::NumericalWorkspace,
) -> Result<Rounded, X87Error> {
    crate::math::extended::transcendental::evaluate(function, x, y, control, workspace)
        .map_err(|_| X87Error::NumericalFailure)
}
