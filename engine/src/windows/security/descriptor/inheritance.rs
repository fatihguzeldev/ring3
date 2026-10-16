use super::*;

fn inherit_acl(
    parent: &Dacl32,
    child: &mut Dacl32,
    owner: Sid32,
    group: Sid32,
    directory: bool,
    mapping: super::super::GenericMapping32,
) -> Result<(), Error> {
    let Dacl32::Acl(parent) = parent else {
        if *parent == Dacl32::Null && *child == Dacl32::Absent {
            *child = Dacl32::Null;
        }
        return Ok(());
    };
    let mut inherited = Vec::new();
    inherited
        .try_reserve_exact(parent.aces.len().checked_mul(2).ok_or(Error::Capacity)?)
        .map_err(|_| Error::Allocation)?;
    for original in &parent.aces {
        if if directory {
            original.flags & 3 == 0
        } else {
            original.flags & 1 == 0
        } {
            continue;
        }
        let mut ace = *original;
        ace.flags |= 0x10;
        if directory {
            if ace.flags & 2 != 0 {
                ace.flags &= !8;
            } else {
                ace.flags |= 8;
            }
            if ace.flags & 4 != 0 {
                if ace.flags & 8 != 0 {
                    continue;
                }
                ace.flags &= !7;
            }
        } else {
            ace.flags &= !15;
        }
        let replacement = if ace.sid == Sid32::creator_owner() {
            Some(owner)
        } else if ace.sid == Sid32::creator_group() {
            Some(group)
        } else {
            None
        };
        if let Some(sid) = replacement
            && ace.flags & 8 == 0
        {
            let propagation = (directory && ace.flags & 3 != 0).then_some(AccessAce32 {
                flags: ace.flags | 8,
                ..ace
            });
            ace.sid = sid;
            ace.flags &= !15;
            ace.mask = mapping.map(ace.mask);
            inherited.push(ace);
            if let Some(propagation) = propagation {
                inherited.push(propagation);
            }
        } else {
            if ace.flags & 8 == 0 {
                ace.mask = mapping.map(ace.mask);
            }
            inherited.push(ace);
        }
    }
    if !inherited.is_empty() {
        match child {
            Dacl32::Acl(acl) => {
                acl.aces
                    .try_reserve(inherited.len())
                    .map_err(|_| Error::Allocation)?;
                acl.aces.extend(inherited);
            }
            Dacl32::Absent => {
                *child = Dacl32::Acl(Acl32 {
                    revision: parent.revision,
                    aces: inherited,
                })
            }
            Dacl32::Null => {}
        }
    }
    Ok(())
}
impl SecurityDescriptor32 {
    pub fn inherit_parent_dacl(
        &mut self,
        parent: &Self,
        directory: bool,
        mapping: super::super::GenericMapping32,
    ) -> Result<(), Error> {
        if self.control & 0x1000 != 0 {
            return Ok(());
        }
        let owner = self.owner.ok_or(Error::InvalidDescriptor)?;
        let group = self.group.ok_or(Error::InvalidDescriptor)?;
        if let Dacl32::Acl(acl) = &mut self.dacl {
            acl.aces.retain(|ace| ace.flags & 0x10 == 0);
        }
        inherit_acl(
            &parent.dacl,
            &mut self.dacl,
            owner,
            group,
            directory,
            mapping,
        )?;
        self.control = (self.control & !0x100) | 0x400;
        self.to_self_relative()?;
        Ok(())
    }
    pub fn inherited(
        parent: &Self,
        explicit: Option<Self>,
        token: &Token32,
        directory: bool,
        mapping: super::super::GenericMapping32,
    ) -> Result<Self, Error> {
        let mut child = explicit.unwrap_or(Self {
            owner: None,
            group: None,
            dacl: Dacl32::Absent,
            sacl: Dacl32::Absent,
            control: SELF_RELATIVE,
            resource_manager: 0,
        });
        if let Some(owner) = child.owner
            && !token.may_set_owner(owner)
        {
            return Err(Error::AccessDenied);
        }
        if child.sacl != Dacl32::Absent {
            token.check_security_privilege()?;
        }
        let owner = *child.owner.get_or_insert(token.user());
        let group = *child.group.get_or_insert(token.primary_group());
        if child.control & 0x1000 == 0
            && (child.dacl == Dacl32::Absent || child.control & 0x100 != 0)
        {
            inherit_acl(
                &parent.dacl,
                &mut child.dacl,
                owner,
                group,
                directory,
                mapping,
            )?;
        }
        if child.control & 0x2000 == 0 {
            inherit_acl(
                &parent.sacl,
                &mut child.sacl,
                owner,
                group,
                directory,
                mapping,
            )?;
        }
        if child.dacl == Dacl32::Absent {
            child.dacl = Self::for_token(token)?.dacl;
        }
        child.to_self_relative()?;
        Ok(child)
    }
}
