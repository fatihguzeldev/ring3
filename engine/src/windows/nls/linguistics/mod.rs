mod classification;
mod collation;
mod mapping;
pub use classification::character_types;
pub use collation::{compare, sort_key};
pub use mapping::map_string;

mod width;
