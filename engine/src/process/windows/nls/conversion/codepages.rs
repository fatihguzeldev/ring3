use super::super::{NlsRuntime32, buffers};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::nls::{NlsApi32, NlsError32},
};
impl NlsRuntime32 {
    pub(in crate::process::windows::nls) fn codepages(
        &self,
        memory: &mut AddressSpace,
        api: NlsApi32,
        thread: u32,
        args: &[u32],
    ) -> Result<u32, NlsError32> {
        use NlsApi32::*;
        match api {
            GetACP => Ok(self.profile.ansi.id()),
            GetOEMCP => Ok(self.profile.oem.id()),
            IsValidCodePage => Ok(u32::from(self.code_page(args[0], thread).is_ok())),
            IsDBCSLeadByte => Ok(u32::from(self.profile.ansi.is_lead_byte(args[0] as u8))),
            IsDBCSLeadByteEx => {
                let cp = self.code_page(args[0], thread)?;
                Ok(u32::from(cp.is_lead_byte(args[1] as u8)))
            }
            GetCPInfo | GetCPInfoExA | GetCPInfoExW => {
                let cp = self.code_page(args[0], thread)?;
                let extended = api != GetCPInfo;
                let wide = api == GetCPInfoExW;
                if extended && args[1] != 0 {
                    return Err(NlsError32::InvalidFlags);
                }
                let mut bytes = vec![
                    0;
                    if !extended {
                        20
                    } else if wide {
                        544
                    } else {
                        284
                    }
                ];
                bytes[..4].copy_from_slice(&cp.maximum_char_size().to_le_bytes());
                bytes[6..18].copy_from_slice(&cp.lead_bytes());
                bytes[4] = b'?';
                if extended {
                    bytes[18..20].copy_from_slice(&0x3fu16.to_le_bytes());
                    bytes[20..24].copy_from_slice(&cp.id().to_le_bytes());
                    let name = cp.name();
                    let units = name.encode_utf16().chain([0]).collect::<Vec<_>>();
                    let encoded = buffers::encoded(&units, wide, self.profile.ansi)?;
                    bytes[24..24 + encoded.len()].copy_from_slice(&encoded);
                }
                let address = args[if extended { 2 } else { 1 }];
                if address == 0 {
                    return Err(NlsError32::InvalidParameter);
                }
                memory.write(GuestAddress(address), &bytes)?;
                Ok(1)
            }
            _ => unreachable!(),
        }
    }
}
