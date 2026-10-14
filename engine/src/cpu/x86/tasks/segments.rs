use super::{
    TaskAuthorityRequirement32, TaskIncomingContext32, TaskLoadState32, TaskTransitionFault32,
};
use crate::cpu::x86::{
    SegmentCache, SegmentDescriptorRecord, SegmentDescriptorTable, SegmentRegister,
};

struct Tables<'a> {
    records: &'a SegmentDescriptorTable,
    gdt_limit: u16,
    cleared: (u16, SegmentDescriptorRecord),
    ldtr: u16,
}

impl Tables<'_> {
    fn record(
        &self,
        selector: u16,
    ) -> Result<Result<SegmentDescriptorRecord, TaskTransitionFault32>, TaskAuthorityRequirement32>
    {
        let error = u32::from(selector & !3);
        if selector & !3 == 0 {
            return Ok(Err(TaskTransitionFault32::InvalidTss(0)));
        }
        if selector & 4 != 0 {
            return if self.ldtr & !3 == 0 {
                Ok(Err(TaskTransitionFault32::InvalidTss(error)))
            } else {
                Err(TaskAuthorityRequirement32::Ldt(self.ldtr))
            };
        }
        if u32::from(selector & !7) + 7 > u32::from(self.gdt_limit) {
            return Ok(Err(TaskTransitionFault32::InvalidTss(error)));
        }
        if selector & !3 == self.cleared.0 & !3 {
            return Ok(Ok(self.cleared.1));
        }
        self.records
            .raw_descriptor(selector)
            .copied()
            .map(Ok)
            .ok_or(TaskAuthorityRequirement32::Descriptor(selector))
    }
}

fn is_code(record: SegmentDescriptorRecord) -> bool {
    record.code_data() && record.descriptor_type() & 8 != 0
}
fn conforming(record: SegmentDescriptorRecord) -> bool {
    is_code(record) && record.descriptor_type() & 4 != 0
}
fn fail(mut context: TaskIncomingContext32, fault: TaskTransitionFault32) -> TaskIncomingContext32 {
    context.fail(fault);
    context
}
fn load(
    context: &mut TaskIncomingContext32,
    register: SegmentRegister,
    record: SegmentDescriptorRecord,
) -> Result<(), TaskAuthorityRequirement32> {
    let selector = context.selectors()[register.index()];
    let cache = record
        .loaded_cache()
        .ok_or(TaskAuthorityRequirement32::SegmentMode(selector))?;
    context.load(
        register,
        record.code_data_base().expect("qualified code/data"),
        cache,
    );
    Ok(())
}

/// the selected p6 table 10-1 qualification order applies after raw incoming state is loaded.
/// missing supplied records are capability boundaries, never fabricated task-switch faults.
pub(crate) fn incoming_segments(
    incoming: TaskLoadState32,
    table: &SegmentDescriptorTable,
    gdt_limit: u16,
    cleared: (u16, SegmentDescriptorRecord),
) -> Result<TaskIncomingContext32, TaskAuthorityRequirement32> {
    if incoming.flags & 0x20000 != 0 {
        return Err(TaskAuthorityRequirement32::Virtual8086);
    }
    if incoming.flags & 0x183000 != 0 {
        return Err(TaskAuthorityRequirement32::Flags(incoming.flags & 0x183000));
    }
    let mut context = TaskIncomingContext32::new(incoming);
    let tables = Tables {
        records: table,
        gdt_limit,
        cleared,
        ldtr: incoming.ldtr,
    };
    let ldt = if incoming.ldtr & !3 == 0 {
        None
    } else {
        let code = u32::from(incoming.ldtr & !3);
        if incoming.ldtr & 4 != 0 {
            return Ok(fail(context, TaskTransitionFault32::InvalidTss(code)));
        }
        let record = match tables.record(incoming.ldtr)? {
            Ok(record) => record,
            Err(fault) => return Ok(fail(context, fault)),
        };
        if record.code_data() || record.descriptor_type() != 2 {
            return Ok(fail(context, TaskTransitionFault32::InvalidTss(code)));
        }
        Some(record)
    };
    let cpl = context.current_privilege();
    let cs_selector = incoming.selectors[SegmentRegister::Cs.index()];
    let ss_selector = incoming.selectors[SegmentRegister::Ss.index()];
    let cs_code = u32::from(cs_selector & !3);
    let ss_code = u32::from(ss_selector & !3);
    let cs = tables.record(cs_selector)?;
    // code dpl precedes stack qualification; incompatible code types wait for the cs-valid row.
    if let Ok(record) = cs
        && is_code(record)
        && if conforming(record) {
            record.dpl() > cpl
        } else {
            record.dpl() != cpl
        }
    {
        return Ok(fail(context, TaskTransitionFault32::InvalidTss(cs_code)));
    }
    let ss = match tables.record(ss_selector)? {
        Ok(record) if record.code_data() && record.descriptor_type() & 0xa == 2 => record,
        Ok(_) => return Ok(fail(context, TaskTransitionFault32::InvalidTss(ss_code))),
        Err(fault) => return Ok(fail(context, fault)),
    };
    if !ss.present() {
        return Ok(fail(context, TaskTransitionFault32::StackSegment(ss_code)));
    }
    if ss.dpl() != cpl {
        return Ok(fail(context, TaskTransitionFault32::InvalidTss(ss_code)));
    }
    // availability of a fully qualified stack is independent of a later CS fault.
    // defer an invalid RPL fault to its table10-1 row, without withholding a valid cache.
    if ss_selector & 3 == u16::from(cpl) {
        load(&mut context, SegmentRegister::Ss, ss)?;
    }
    if ldt.is_some_and(|record| !record.present()) {
        return Ok(fail(
            context,
            TaskTransitionFault32::InvalidTss(u32::from(incoming.ldtr & !3)),
        ));
    }
    context.load_ldtr(ldt);
    let cs = match cs {
        Ok(record) if is_code(record) => record,
        Ok(_) => return Ok(fail(context, TaskTransitionFault32::InvalidTss(cs_code))),
        Err(fault) => return Ok(fail(context, fault)),
    };
    if !cs.present() {
        return Ok(fail(context, TaskTransitionFault32::NotPresent(cs_code)));
    }
    if cpl != 3 {
        return Err(TaskAuthorityRequirement32::Privilege(cpl));
    }
    load(&mut context, SegmentRegister::Cs, cs)?;
    if ss_selector & 3 != u16::from(cpl) {
        return Ok(fail(context, TaskTransitionFault32::InvalidTss(ss_code)));
    }
    load(&mut context, SegmentRegister::Ss, ss)?;
    let data = [
        SegmentRegister::Ds,
        SegmentRegister::Es,
        SegmentRegister::Fs,
        SegmentRegister::Gs,
    ];
    let mut records = [None; 4];
    // table 10-1 groups all data validity/readability checks before presence and privilege.
    for (index, register) in data.into_iter().enumerate() {
        let selector = incoming.selectors[register.index()];
        if selector & !3 == 0 {
            context.load(register, 0, SegmentCache::UNUSABLE);
            continue;
        }
        let record = match tables.record(selector)? {
            Ok(record) if record.code_data() => record,
            Ok(_) => {
                return Ok(fail(
                    context,
                    TaskTransitionFault32::InvalidTss(u32::from(selector & !3)),
                ));
            }
            Err(fault) => return Ok(fail(context, fault)),
        };
        records[index] = Some(record);
    }
    for (register, record) in data.into_iter().zip(records) {
        if let Some(record) = record
            && is_code(record)
            && record.descriptor_type() & 2 == 0
        {
            return Ok(fail(
                context,
                TaskTransitionFault32::InvalidTss(u32::from(
                    incoming.selectors[register.index()] & !3,
                )),
            ));
        }
    }
    for (register, record) in data.into_iter().zip(records) {
        if let Some(record) = record
            && !record.present()
        {
            return Ok(fail(
                context,
                TaskTransitionFault32::NotPresent(u32::from(
                    incoming.selectors[register.index()] & !3,
                )),
            ));
        }
    }
    for (register, record) in data.into_iter().zip(records) {
        let Some(record) = record else {
            continue;
        };
        let selector = incoming.selectors[register.index()];
        if !conforming(record) && cpl.max((selector & 3) as u8) > record.dpl() {
            return Ok(fail(
                context,
                TaskTransitionFault32::InvalidTss(u32::from(selector & !3)),
            ));
        }
        load(&mut context, register, record)?;
    }
    if context.state().eip > context.segment_image().caches[SegmentRegister::Cs.index()].limit() {
        context.fail(TaskTransitionFault32::GeneralProtection(0));
    }
    Ok(context)
}

#[cfg(test)]
#[path = "segments/tests.rs"]
mod tests;
