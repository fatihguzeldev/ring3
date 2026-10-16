use super::super::{NlsRuntime32, buffers};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{
        nls::{NlsApi32, NlsError32},
        text::CodePage32,
    },
};
impl NlsRuntime32 {
    pub(in crate::process::windows::nls) fn codepages(
        &self,
        memory: &mut AddressSpace,
        api: NlsApi32,
        args: &[u32],
    ) -> Result<u32, NlsError32> {
        use NlsApi32::*;
        match api {
            GetACP => Ok(self.profile.ansi.id()),
            GetOEMCP => Ok(self.profile.oem.id()),
            IsValidCodePage => Ok(u32::from(self.profile.code_page(args[0]).is_ok())),
            IsDBCSLeadByte => Ok(0),
            IsDBCSLeadByteEx => {
                self.profile.code_page(args[0])?;
                Ok(0)
            }
            GetCPInfo | GetCPInfoExA | GetCPInfoExW => {
                let cp = self.profile.code_page(args[0])?;
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
                bytes[..4].copy_from_slice(
                    &(if cp == CodePage32::Utf8 { 4u32 } else { 1 }).to_le_bytes(),
                );
                bytes[4] = b'?';
                if extended {
                    bytes[18..20].copy_from_slice(&0x3fu16.to_le_bytes());
                    bytes[20..24].copy_from_slice(&cp.id().to_le_bytes());
                    let name = match cp {
                        CodePage32::Windows1252 => "1252 (ANSI - Latin I)",
                        CodePage32::Oem437 => "437 (OEM - United States)",
                        CodePage32::Utf8 => "65001 (Unicode - UTF-8)",
                    };
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
