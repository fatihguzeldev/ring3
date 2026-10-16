use icu_experimental::displaynames::provider::{
    Baked, LanguageDisplayNamesV1, LocaleDisplayNamesV1, RegionDisplayNamesV1,
    ScriptDisplayNamesV1, VariantDisplayNamesV1,
};
use icu_provider::prelude::*;

pub(super) struct DisplayProvider32 {
    pub(super) needs_variants: bool,
}

macro_rules! forward {
    ($($marker:ty),+ $(,)?) => {$ (
        impl DataProvider<$marker> for DisplayProvider32 {
            fn load(&self, request: DataRequest) -> Result<DataResponse<$marker>, DataError> {
                DataProvider::<$marker>::load(&Baked, request)
            }
        }
    )+};
}
forward!(
    LocaleDisplayNamesV1,
    LanguageDisplayNamesV1,
    ScriptDisplayNamesV1,
    RegionDisplayNamesV1
);

impl DataProvider<VariantDisplayNamesV1> for DisplayProvider32 {
    fn load(&self, request: DataRequest) -> Result<DataResponse<VariantDisplayNamesV1>, DataError> {
        if !self.needs_variants {
            // icu's locale formatter requires this dataset but never reads it for a variant-free target.
            return Ok(DataResponse {
                metadata: Default::default(),
                payload: DataPayload::from_owned(Default::default()),
            });
        }
        DataProvider::<VariantDisplayNamesV1>::load(&Baked, request)
    }
}
