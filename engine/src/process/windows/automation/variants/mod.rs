mod coercion;
mod date;
mod owned;
pub(super) use owned::OwnedValuePlan32;

#[cfg(test)]
mod tests;

use crate::{
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError},
    windows::automation::{AutomationError32 as E, Oleaut32Api32 as Api, variants::Variant32},
};
pub(super) enum Coerced32 {
    Value(Variant32),
    String(Vec<u8>),
}
pub(super) enum VariantOperation32 {
    Complete(u32),
    Publish {
        destination: u32,
        plan: OwnedValuePlan32,
    },
}
impl EngineInstance {
    pub(in crate::process::windows::automation) fn automation_variant_call(
        &mut self,
        api: Api,
        args: &[u32],
    ) -> Result<u32, HostError> {
        match self.prepare_automation_variant_operation(api, args)? {
            VariantOperation32::Complete(value) => Ok(value),
            VariantOperation32::Publish {
                destination,
                mut plan,
            } => {
                self.publish_retained_variant_plan(destination, &mut plan)?;
                Ok(0)
            }
        }
    }

    pub(super) fn prepare_automation_variant_operation(
        &mut self,
        api: Api,
        args: &[u32],
    ) -> Result<VariantOperation32, HostError> {
        if args[0] == 0 {
            return Err(HostError::Automation(E::InvalidParameter));
        }
        self.memory()?
            .check_span(GuestAddress(args[0]), 16, Access::Write)
            .map_err(HostError::Memory)?;
        if api == Api::VariantInit {
            self.memory
                .as_mut()
                .unwrap()
                .write(GuestAddress(args[0]), &Variant32::EMPTY.0)
                .map_err(HostError::Memory)?;
            return Ok(VariantOperation32::Complete(0));
        }
        let destination = self.read_automation_variant(args[0], false)?;
        if api == Api::VariantCopy && args[0] == args[1] {
            return Ok(VariantOperation32::Complete(0));
        }
        if api == Api::VariantClear {
            let plan = self.plan_owned_variant(destination.cleared(), destination, true)?;
            return Ok(VariantOperation32::Publish {
                destination: args[0],
                plan,
            });
        }
        if args[1] == 0 {
            return Err(HostError::Automation(E::InvalidParameter));
        }
        let indirect = api != Api::VariantCopy;
        let source = self.read_automation_variant(args[1], indirect)?;
        let converted = if matches!(api, Api::VariantCopy | Api::VariantCopyInd) {
            Coerced32::Value(source)
        } else {
            let (locale, flags, target) = if api == Api::VariantChangeTypeEx {
                (args[2], args[3] as u16, args[4] as u16)
            } else {
                (self.windows_user_locale(), args[2] as u16, args[3] as u16)
            };
            if matches!(source.vartype(), 9 | 13)
                && source.pointer() != 0
                && matches!(target, 9 | 13)
                && target != source.vartype()
            {
                return Err(HostError::Automation(E::NotImplemented));
            }
            if source.vartype() == 9
                && source.pointer() != 0
                && !matches!(target, 9 | 13)
                && flags & 1 == 0
            {
                if target & 0xf000 != 0 {
                    return Err(HostError::Automation(E::BadVariantType));
                }
                Variant32::new(target, &[])
                    .validate()
                    .map_err(HostError::Automation)?;
                return Err(HostError::Automation(E::NotImplemented));
            }
            self.coerce_automation_variant(source, locale, flags, target)?
        };
        let plan = match converted {
            Coerced32::String(bytes) => self.plan_owned_string(&bytes, destination)?,
            Coerced32::Value(value) => self.plan_owned_variant(value, destination, false)?,
        };
        Ok(VariantOperation32::Publish {
            destination: args[0],
            plan,
        })
    }
    pub(in crate::process::windows::automation) fn read_automation_variant(
        &self,
        pointer: u32,
        indirect: bool,
    ) -> Result<Variant32, HostError> {
        let mut bytes = [0; 16];
        self.memory()?
            .read(GuestAddress(pointer), &mut bytes)
            .map_err(HostError::Memory)?;
        let mut value = Variant32(bytes);
        let mut seen = [0u32; 128];
        for depth in 0..self.automation.limits.nesting {
            value.validate().map_err(HostError::Automation)?;
            if !indirect || !value.by_ref() {
                return Ok(value);
            }
            let at = value.pointer();
            if at == 0 || seen[..depth].contains(&at) {
                return Err(HostError::Automation(E::InvalidParameter));
            }
            seen[depth] = at;
            if value.base() == 12 && !value.array() {
                self.memory()?
                    .read(GuestAddress(at), &mut bytes)
                    .map_err(HostError::Memory)?;
                value = Variant32(bytes);
            } else {
                let count = value.value_bytes();
                let mut dereferenced = Variant32::new(value.vartype() & !0x4000, &[]);
                let destination = if value.base() == 14 && !value.array() {
                    &mut dereferenced.0[..]
                } else {
                    &mut dereferenced.0[8..8 + count]
                };
                self.memory()?
                    .read(GuestAddress(at), destination)
                    .map_err(HostError::Memory)?;
                if value.base() == 14 && !value.array() {
                    dereferenced.0[..2].copy_from_slice(&14u16.to_le_bytes());
                }
                value = dereferenced;
            }
        }
        Err(HostError::Automation(E::InvalidParameter))
    }
}
