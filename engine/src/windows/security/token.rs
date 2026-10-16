use super::{SecurityError32 as Error, Sid32};

#[derive(Clone, Copy, Debug)]
pub struct TokenGroup32 {
    pub sid: Sid32,
    pub deny_only: bool,
}
#[derive(Clone, Debug)]
pub struct Token32 {
    user: Sid32,
    primary_group: Sid32,
    groups: [Option<TokenGroup32>; 16],
    security_privilege: bool,
    restore_privilege: bool,
}
impl Token32 {
    pub fn virtual_process() -> Self {
        let mut groups = [None; 16];
        for (slot, sid) in groups.iter_mut().zip([
            Sid32::everyone(),
            Sid32::new(5, &[11]).unwrap(),
            Sid32::new(5, &[32, 545]).unwrap(),
        ]) {
            *slot = Some(TokenGroup32 {
                sid,
                deny_only: false,
            });
        }
        Self {
            user: Sid32::virtual_user(),
            primary_group: Sid32::new(5, &[32, 545]).unwrap(),
            groups,
            security_privilege: false,
            restore_privilege: false,
        }
    }
    pub fn user(&self) -> Sid32 {
        self.user
    }
    pub fn primary_group(&self) -> Sid32 {
        self.primary_group
    }
    pub fn matches(&self, sid: Sid32, deny: bool) -> bool {
        self.user == sid
            || self
                .groups
                .iter()
                .flatten()
                .any(|group| group.sid == sid && (deny || !group.deny_only))
    }
    pub fn may_set_owner(&self, owner: Sid32) -> bool {
        owner == self.user || self.restore_privilege
    }
    pub fn check_security_privilege(&self) -> Result<(), Error> {
        if self.security_privilege {
            Ok(())
        } else {
            Err(Error::PrivilegeNotHeld)
        }
    }
}
