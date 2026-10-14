use super::{TaskAuthority, TaskDescriptorTable, TaskRegister, TaskReturnError};

/// descriptor admission only. cr3/ldtr/ts, outgoing saves and incoming faults remain uncommitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedTaskReturn {
    current: TaskRegister,
    target: TaskRegister,
}

impl PreparedTaskReturn {
    pub const fn current(self) -> TaskRegister {
        self.current
    }
    pub const fn target(self) -> TaskRegister {
        self.target
    }
}

pub fn prepare_return(
    authority: Option<&TaskAuthority<'_>>,
) -> Result<PreparedTaskReturn, TaskReturnError> {
    let authority = authority.ok_or(TaskReturnError::MissingAuthority)?;
    let backlink = authority.backlink()?;
    let target = admit_target(backlink, authority.descriptors())?;
    Ok(PreparedTaskReturn {
        current: authority.current(),
        target,
    })
}

fn admit_target(
    selector: u16,
    descriptors: &TaskDescriptorTable<'_>,
) -> Result<TaskRegister, TaskReturnError> {
    let error = u32::from(selector & !3);
    if selector & !3 == 0
        || selector & 4 != 0
        || u32::from(selector & !7) + 7 > u32::from(descriptors.limit())
    {
        return Err(TaskReturnError::InvalidTss(error));
    }
    let record = descriptors.record(selector)?;
    let format =
        super::register::descriptor_format(record).ok_or(TaskReturnError::InvalidTss(error))?;
    // selected p6 order (intel 3a table 10-1): presence before busy before limit.
    // it is not a universal intel/amd competing-fault order. iret ignores tss dpl/rpl.
    if !record.present() {
        return Err(TaskReturnError::NotPresent(error));
    }
    if record.descriptor_type() & 2 == 0 {
        return Err(TaskReturnError::InvalidTss(error));
    }
    let target = TaskRegister::admitted(selector, record, format);
    if target.limit() < format.minimum_limit() {
        return Err(TaskReturnError::InvalidTss(error));
    }
    Ok(target)
}

#[cfg(test)]
mod tests;
