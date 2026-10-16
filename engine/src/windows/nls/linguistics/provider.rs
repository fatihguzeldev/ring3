use icu_collator::provider::{
    Baked, CollationDiacriticsV1, CollationJamoV1, CollationMetadataV1, CollationReorderingV1,
    CollationRootV1, CollationSpecialPrimariesV1, CollationTailoringV1,
};
use icu_normalizer::provider::{NormalizerNfdDataV1, NormalizerNfdTablesV1};
use icu_provider::prelude::*;
use std::cell::RefCell;

// only the dotted and dotless i tailoring differs under filesystem casing.
pub(super) struct FileSystemCollationProvider32;
const I_FAMILY: [u32; 4] = [0x49, 0x69, 0x130, 0x131];
// icu_collator 2.1.1 elements.rs: the ce32 fallback tag delegates to the root data.
const ROOT_FALLBACK: u32 = 0xc0;
const MAX_TAILORINGS: usize = 16;
struct CachedTailoring32 {
    locale: String,
    attributes: String,
    payload: DataPayload<CollationTailoringV1>,
}
thread_local! { static TAILORINGS: RefCell<Vec<CachedTailoring32>> = const { RefCell::new(Vec::new()) }; }

macro_rules! forward {
    ($provider:path; $($marker:ty),+ $(,)?) => {$ (
        impl DataProvider<$marker> for FileSystemCollationProvider32 {
            fn load(&self, request: DataRequest) -> Result<DataResponse<$marker>, DataError> {
                DataProvider::<$marker>::load(&$provider, request)
            }
        }
    )+};
}
forward!(Baked;
    CollationRootV1, CollationDiacriticsV1, CollationJamoV1,
    CollationMetadataV1, CollationReorderingV1, CollationSpecialPrimariesV1
);
forward!(icu_normalizer::provider::Baked; NormalizerNfdDataV1, NormalizerNfdTablesV1);

fn adapted(
    payload: DataPayload<CollationTailoringV1>,
) -> Result<DataPayload<CollationTailoringV1>, DataError> {
    let source = payload
        .get_static()
        .ok_or(DataError::custom("nonstatic collation data"))?;
    let replacements = I_FAMILY.map(|point| source.trie.get32(point));
    // shared ce32 values would change unrelated characters or contractions.
    for range in source.trie.iter_ranges() {
        if range.value != ROOT_FALLBACK
            && replacements.contains(&range.value)
            && range.range.clone().any(|point| !I_FAMILY.contains(&point))
        {
            return Err(DataError::custom("shared filesystem casing tailoring"));
        }
    }
    let mut data = source.clone();
    data.trie = source.trie.try_alloc_map_value(|value| {
        Ok::<_, DataError>(if replacements.contains(&value) {
            ROOT_FALLBACK
        } else {
            value
        })
    })?;
    Ok(DataPayload::from_owned(data))
}
impl DataProvider<CollationTailoringV1> for FileSystemCollationProvider32 {
    fn load(&self, request: DataRequest) -> Result<DataResponse<CollationTailoringV1>, DataError> {
        let mut response = DataProvider::<CollationTailoringV1>::load(&Baked, request)?;
        let locale = request.id.locale.to_string();
        let attributes = request.id.marker_attributes.as_str();
        TAILORINGS.with(|cache| {
            let mut cache = cache.borrow_mut();
            if let Some(row) = cache
                .iter()
                .find(|row| row.locale == locale && row.attributes == attributes)
            {
                response.payload = row.payload.clone();
                return Ok(response);
            }
            if cache.len() >= MAX_TAILORINGS {
                return Err(DataError::custom("collation cache capacity"));
            }
            let payload = adapted(response.payload)?;
            cache
                .try_reserve(1)
                .map_err(|_| DataError::custom("collation cache allocation"))?;
            cache.push(CachedTailoring32 {
                locale,
                attributes: attributes.to_owned(),
                payload: payload.clone(),
            });
            response.payload = payload;
            Ok(response)
        })
    }
}
