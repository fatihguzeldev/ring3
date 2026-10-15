use super::*;

#[derive(Debug, PartialEq)]
struct ExchangeState {
    backing: Vec<u8>,
    mappings: Vec<u32>,
    permissions: Vec<Permissions>,
    free: Vec<u32>,
    versions: Vec<PageVersion>,
    identity: u64,
    version: u64,
    code_version: u64,
}

fn exchange_state(space: &AddressSpace) -> ExchangeState {
    ExchangeState {
        backing: space.backing.to_vec(),
        mappings: space.mappings.to_vec(),
        permissions: space.permissions.to_vec(),
        free: space.free.clone(),
        versions: space.versions.to_vec(),
        identity: space.identity,
        version: space.version,
        code_version: space.code_validation_version(),
    }
}

mod add;
mod bits;
mod compare;
mod exchange;
mod logical;
