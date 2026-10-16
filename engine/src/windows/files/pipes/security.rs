use crate::windows::security::{
    AccessAce32, Dacl32, SecurityDescriptor32, SecurityError32, Sid32, Token32,
};

pub(crate) fn default_security(token: &Token32) -> Result<SecurityDescriptor32, SecurityError32> {
    let mut descriptor = SecurityDescriptor32::for_token(token)?;
    let Dacl32::Acl(acl) = &mut descriptor.dacl else {
        unreachable!()
    };
    acl.aces
        .try_reserve_exact(3)
        .map_err(|_| SecurityError32::Allocation)?;
    for (sid, mask) in [
        (Sid32::new(5, &[32, 544])?, 0x001f_01ff),
        (Sid32::everyone(), 0x0012_0089),
        (Sid32::new(5, &[7])?, 0x0012_0089),
    ] {
        acl.aces.push(AccessAce32 {
            kind: 0,
            flags: 0,
            mask,
            sid,
        });
    }
    for ace in &mut acl.aces {
        ace.flags = 0;
    }
    Ok(descriptor)
}

use super::{PipeContext32, PipeEndpointId32, PreparedPipeSecurity32};
use crate::files::FileError32;
impl PipeContext32 {
    pub(crate) fn security_endpoint(&self, name: &[u16]) -> Result<PipeEndpointId32, FileError32> {
        let name = super::pipe_name(name)?.ok_or(FileError32::InvalidName)?;
        self.instances
            .iter()
            .flatten()
            .find(|instance| instance.name.as_ref() == Some(&name) && instance.server.is_some())
            .and_then(|instance| instance.server)
            .ok_or(FileError32::NotFound)
    }
    pub(crate) fn query_security(
        &self,
        id: PipeEndpointId32,
        information: u32,
        token: &Token32,
        granted: Option<u32>,
    ) -> Result<SecurityDescriptor32, FileError32> {
        super::super::security::query(self.security(id)?, information, token, granted)
    }
    pub(crate) fn prepare_security(
        &self,
        id: PipeEndpointId32,
        information: u32,
        supplied: SecurityDescriptor32,
        token: &Token32,
        granted: Option<u32>,
    ) -> Result<PreparedPipeSecurity32, FileError32> {
        let slot = self.endpoint(id)?.instance;
        let instance = self.instances[slot].as_ref().unwrap();
        let descriptor = super::super::security::change(
            &instance.security,
            information,
            supplied,
            token,
            granted,
        )?;
        self.next_revision()?;
        let matches = |index: usize, value: &super::Instance32| {
            index == slot || instance.name.is_some() && value.name == instance.name
        };
        let count = self
            .instances
            .iter()
            .enumerate()
            .filter(|(index, value)| value.as_ref().is_some_and(|value| matches(*index, value)))
            .count();
        let mut updates = Vec::new();
        updates
            .try_reserve_exact(count)
            .map_err(|_| FileError32::Allocation)?;
        for (index, value) in self.instances.iter().enumerate() {
            if value.as_ref().is_some_and(|value| matches(index, value)) {
                updates.push((index, descriptor.snapshot()?));
            }
        }
        Ok(PreparedPipeSecurity32 {
            revision: self.revision,
            updates,
        })
    }
    pub(crate) fn commit_security(&mut self, prepared: PreparedPipeSecurity32) {
        assert_eq!(self.revision, prepared.revision);
        for (slot, descriptor) in prepared.updates {
            self.instances[slot].as_mut().unwrap().security = descriptor;
        }
        self.revision += 1;
    }
}
