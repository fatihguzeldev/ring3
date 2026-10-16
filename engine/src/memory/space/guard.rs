use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GuardPage32 {
    address: GuestAddress,
    identity: u64,
    mapping: u64,
    permissions: Permissions,
}

impl GuardPage32 {
    pub(crate) fn address(self) -> GuestAddress {
        self.address
    }

    pub(crate) fn is_current(self, memory: &AddressSpace) -> bool {
        self.identity == memory.identity
            && memory.mapping_version_at(self.address) == Ok(self.mapping)
            && memory.permissions_at(self.address) == Ok(self.permissions)
    }

    pub(crate) fn clear(self) -> PrivatePageChange32 {
        PrivatePageChange32::Protect {
            address: self.address,
            permissions: self.permissions.without_guard(),
        }
    }
}

impl AddressSpace {
    pub(crate) fn guard_page(&self, address: GuestAddress) -> Option<GuardPage32> {
        let address = GuestAddress(address.0 / PAGE_SIZE * PAGE_SIZE);
        let permissions = self.permissions_at(address).ok()?;
        permissions.is_guarded().then(|| GuardPage32 {
            address,
            identity: self.identity,
            mapping: self.mapping_version_at(address).unwrap(),
            permissions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_permissions_keep_public_bits_and_ordinary_access_contracts() {
        for permissions in [Permissions::READ, Permissions::READ_WRITE, Permissions::ALL] {
            let guard = permissions.with_guard();
            assert!(guard.is_guarded());
            assert_eq!(guard.without_guard(), permissions);
            assert_eq!(guard.with_guard(), guard);
            for access in [Access::Read, Access::Write, Access::Execute] {
                assert!(!guard.allows(access));
            }
        }
        assert_eq!(Permissions::from_bits(0x38), None);
        assert_eq!(Permissions::from_bits(0x78), None);
    }

    #[test]
    fn clearing_guard_and_exception_writes_share_failure_neutral_ram_transaction() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::READ_WRITE.with_guard(),
            )
            .unwrap();
        let guard = memory.guard_page(GuestAddress(0x1234)).unwrap();
        let version = memory.transaction_version();
        assert!(
            memory
                .apply_private_pages(
                    &[guard.clear()],
                    &[
                        (GuestAddress(0x1010), &[0x55]),
                        (GuestAddress(0x8000), &[1])
                    ]
                )
                .is_err()
        );
        assert!(guard.is_current(&memory));
        assert_eq!(memory.transaction_version(), version);
        assert!(memory.read(GuestAddress(0x1010), &mut [0]).is_err());
        memory
            .apply_private_pages(&[guard.clear()], &[(GuestAddress(0x1010), &[0x55])])
            .unwrap();
        assert_eq!(memory.guard_page(GuestAddress(0x1010)), None);
        let mut byte = [0];
        memory.read(GuestAddress(0x1010), &mut byte).unwrap();
        assert_eq!(byte, [0x55]);
        assert_eq!(memory.transaction_version(), version + 1);
    }

    #[test]
    fn guard_witness_rejects_same_rights_reprotect_and_replacement_mapping() {
        let mut memory = AddressSpace::new(1).unwrap();
        let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
        memory
            .map_zeroed(range, Permissions::READ.with_guard())
            .unwrap();
        let original = memory.guard_page(GuestAddress(0x1000)).unwrap();
        memory
            .protect(range, Permissions::READ.with_guard())
            .unwrap();
        assert!(!original.is_current(&memory));
        let next = memory.guard_page(GuestAddress(0x1000)).unwrap();
        memory.unmap(range).unwrap();
        memory
            .map_zeroed(range, Permissions::READ.with_guard())
            .unwrap();
        assert!(!next.is_current(&memory));
    }
}
