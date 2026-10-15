mod layout;
mod mapping;

pub use layout::{KeyboardLayout32, character, compose_dead, dead_key, decode_ansi, encode_ansi};
pub use mapping::{
    key_name, map_virtual_key, map_virtual_key_for_layout, scan_code, virtual_key,
    virtual_key_for_layout,
};

#[cfg(test)]
mod tests;
