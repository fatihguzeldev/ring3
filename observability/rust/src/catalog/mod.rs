mod generated;
mod selection;

pub use generated::{CATALOG_VERSION, CATEGORIES, categories};
pub use selection::CategorySelection;

const fn source_fingerprint(source: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut index = 0;
    while index < source.len() {
        hash = (hash ^ source[index] as u64).wrapping_mul(0x100_0000_01b3);
        index += 1;
    }
    hash
}

const _: () = assert!(
    source_fingerprint(include_bytes!("../../../catalog/categories.json"))
        == generated::CATALOG_SOURCE_FINGERPRINT,
    "stale observability catalog; run node observability/catalog/generate.mjs"
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Category(&'static str);

impl Category {
    pub(crate) const fn new_unchecked(id: &'static str) -> Self {
        Self(id)
    }

    pub fn from_id(id: &str) -> Option<Self> {
        CATEGORIES
            .iter()
            .find(|descriptor| descriptor.id == id)
            .map(|descriptor| Self(descriptor.id))
    }

    pub const fn id(self) -> &'static str {
        self.0
    }

    pub fn descriptor(self) -> &'static CategoryDescriptor {
        CATEGORIES
            .iter()
            .find(|descriptor| descriptor.id == self.0)
            .expect("generated category must have a catalog descriptor")
    }

    pub(crate) fn is_within(self, parent: Self) -> bool {
        self == parent
            || self
                .0
                .strip_prefix(parent.0)
                .is_some_and(|suffix| suffix.starts_with('.'))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CategoryDescriptor {
    pub id: &'static str,
    pub key: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub parent: Option<&'static str>,
    pub selectable: bool,
}
