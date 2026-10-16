use super::{Enumeration32, EnumerationItem32, PendingEnum32};
use crate::{
    loader::ResourceIdentifier32 as Identifier,
    memory::{AddressSpace, GuestAddress, PageRange},
    process::windows::{
        callbacks::{GuestCallbackScratch32, WindowsCallback32, WindowsCallbackProvider32},
        resources::ResourceError32 as E,
    },
    windows::text::CodePage32,
};
#[derive(Clone, Copy)]
enum Argument32 {
    Value(u32),
    Offset(u32),
}
fn identifier(value: &Identifier, wide: bool, bytes: &mut Vec<u8>) -> Result<Argument32, E> {
    match value {
        Identifier::Id(id) => {
            if *id <= u32::from(u16::MAX) {
                Ok(Argument32::Value(*id))
            } else {
                Err(E::InvalidParameter)
            }
        }
        Identifier::Name(name) => {
            let offset = u32::try_from(bytes.len()).map_err(|_| E::Capacity)?;
            if wide {
                bytes
                    .try_reserve(name.len() * 2 + 2)
                    .map_err(|_| E::Allocation)?;
                for unit in name {
                    bytes.extend_from_slice(&unit.to_le_bytes());
                }
                bytes.extend_from_slice(&[0, 0]);
            } else {
                let encoded = CodePage32::Windows1252
                    .encode(name, false)
                    .map_err(|_| E::InvalidParameter)?
                    .0;
                bytes
                    .try_reserve(encoded.len() + 1)
                    .map_err(|_| E::Allocation)?;
                bytes.extend_from_slice(&encoded);
                bytes.push(0);
            }
            Ok(Argument32::Offset(offset))
        }
    }
}
impl PendingEnum32 {
    pub(super) fn request(
        &self,
        memory: &mut AddressSpace,
        bounds: PageRange,
        index: usize,
    ) -> Result<(WindowsCallback32, Option<GuestCallbackScratch32>), E> {
        let mut bytes = Vec::new();
        let mut fields = [Argument32::Value(0); 5];
        fields[0] = Argument32::Value(self.base);
        let count = match self.kind {
            Enumeration32::Types => {
                let EnumerationItem32::Identifier(value) = &self.items[index] else {
                    unreachable!()
                };
                fields[1] = identifier(value, self.wide, &mut bytes)?;
                fields[2] = Argument32::Value(self.parameter);
                3
            }
            Enumeration32::Names => {
                let EnumerationItem32::Identifier(value) = &self.items[index] else {
                    unreachable!()
                };
                fields[1] =
                    identifier(self.resource_type.as_ref().unwrap(), self.wide, &mut bytes)?;
                fields[2] = identifier(value, self.wide, &mut bytes)?;
                fields[3] = Argument32::Value(self.parameter);
                4
            }
            Enumeration32::Languages => {
                let EnumerationItem32::Language(value) = self.items[index] else {
                    unreachable!()
                };
                fields[1] =
                    identifier(self.resource_type.as_ref().unwrap(), self.wide, &mut bytes)?;
                fields[2] =
                    identifier(self.resource_name.as_ref().unwrap(), self.wide, &mut bytes)?;
                fields[3] = Argument32::Value(u32::from(value));
                fields[4] = Argument32::Value(self.parameter);
                5
            }
        };
        let scratch = if bytes.is_empty() {
            None
        } else {
            Some(GuestCallbackScratch32::allocate_span(
                memory, bounds, &bytes,
            )?)
        };
        let mut arguments = [0; 16];
        for (index, field) in fields.into_iter().enumerate().take(count) {
            arguments[index] = match field {
                Argument32::Value(value) => value,
                Argument32::Offset(offset) => scratch.unwrap().address.0 + offset,
            };
        }
        Ok((
            WindowsCallback32 {
                procedure: GuestAddress(self.procedure.0),
                arguments,
                count: count as u32,
                continuation: self.continuation,
                provider: WindowsCallbackProvider32::Resources,
            },
            scratch,
        ))
    }
}
