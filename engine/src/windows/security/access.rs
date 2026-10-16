use super::{Dacl32, SecurityDescriptor32, SecurityError32 as Error, Sid32, Token32};

#[derive(Clone, Copy, Debug)]
pub(crate) struct GenericMapping32 {
    pub read: u32,
    pub write: u32,
    pub execute: u32,
    pub all: u32,
}
pub(crate) const FILE_GENERIC_MAPPING: GenericMapping32 = GenericMapping32 {
    read: 0x0012_0089,
    write: 0x0012_0116,
    execute: 0x0012_00a0,
    all: 0x001f_01ff,
};
impl GenericMapping32 {
    pub(crate) fn map(self, access: u32) -> u32 {
        let mut mapped = access & !0xf000_0000;
        for (flag, mask) in [
            (0x8000_0000, self.read),
            (0x4000_0000, self.write),
            (0x2000_0000, self.execute),
            (0x1000_0000, self.all),
        ] {
            if access & flag != 0 {
                mapped |= mask;
            }
        }
        mapped
    }
}
impl SecurityDescriptor32 {
    pub(crate) fn check_access(
        &self,
        token: &Token32,
        desired: u32,
        mapping: GenericMapping32,
    ) -> Result<u32, Error> {
        let desired = mapping.map(desired);
        let maximum = desired & 0x0200_0000 != 0;
        if desired & !(mapping.all | 0x0300_0000) != 0 {
            return Err(Error::AccessDenied);
        }
        let mut granted = 0;
        if desired & 0x0100_0000 != 0 {
            token.check_security_privilege()?;
            granted |= 0x0100_0000;
        }
        let owner = self.owner == Some(token.user());
        let owner_restricted = matches!(&self.dacl, Dacl32::Acl(acl) if acl.aces.iter().any(|ace| ace.sid == Sid32::owner_rights() && ace.flags & 8 == 0));
        if owner && !owner_restricted {
            granted |= 0x0006_0000;
        }
        let requested = desired & !0x0300_0000;
        let mut remaining = if maximum { mapping.all } else { requested } & !granted;
        let mut denied = 0;
        match &self.dacl {
            Dacl32::Absent | Dacl32::Null => {
                granted |= remaining;
                remaining = 0;
            }
            Dacl32::Acl(acl) => {
                for ace in &acl.aces {
                    if ace.flags & 8 != 0 {
                        continue;
                    }
                    let matches = if ace.sid == Sid32::owner_rights() {
                        owner
                    } else {
                        token.matches(ace.sid, ace.kind == 1)
                    };
                    if !matches {
                        continue;
                    }
                    let mask = mapping.map(ace.mask) & remaining;
                    if ace.kind == 1 {
                        if !maximum && mask != 0 {
                            return Err(Error::AccessDenied);
                        }
                        denied |= mask;
                        remaining &= !mask;
                    } else if ace.kind == 0 {
                        granted |= mask;
                        remaining &= !mask;
                    }
                }
            }
        }
        if (!maximum && remaining != 0) || requested & !granted != 0 || requested & denied != 0 {
            return Err(Error::AccessDenied);
        }
        Ok(if maximum {
            granted & (mapping.all | 0x0100_0000)
        } else {
            desired
        })
    }
}
