use super::super::{NlsRuntime32, buffers};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::nls::{NlsApi32, NlsError32},
};
impl NlsRuntime32 {
    pub(in crate::process::windows::nls) fn convert(
        &self,
        memory: &mut AddressSpace,
        api: NlsApi32,
        thread: u32,
        args: &[u32],
    ) -> Result<u32, NlsError32> {
        let cp = self.code_page(args[0], thread)?;
        if args[2] == args[4] || args[3] == 0 || (args[3] as i32) < -1 || (args[5] as i32) < 0 {
            return Err(NlsError32::InvalidParameter);
        }
        if api == NlsApi32::MultiByteToWideChar {
            let input = buffers::bytes(
                memory,
                args[2],
                args[3] as i32,
                1,
                self.profile.max_text_units,
            )?;
            let units = cp
                .decode_windows(&input, args[1])
                .map_err(buffers::conversion_error)?;
            let output = buffers::encoded(&units, true, cp)?;
            return buffers::output(memory, args[4], args[5] as i32, &output, 2);
        }
        if matches!(
            cp,
            crate::windows::text::CodePage32::Utf8
                | crate::windows::text::CodePage32::Legacy(54936)
        ) && (args[6] != 0 || args[7] != 0)
        {
            return Err(NlsError32::InvalidParameter);
        }
        let input = buffers::units(
            memory,
            args[2],
            args[3] as i32,
            true,
            cp,
            self.profile.max_text_units,
        )?;
        let default = if args[6] == 0 {
            vec![b'?']
        } else {
            let first = buffers::fixed::<1>(memory, args[6])?[0];
            if cp.is_lead_byte(first) {
                buffers::fixed::<2>(memory, args[6])?.to_vec()
            } else {
                vec![first]
            }
        };
        let (output, used) = cp
            .encode_windows_default(&input, args[1], &default)
            .map_err(buffers::conversion_error)?;
        let capacity = args[5] as i32;
        if capacity != 0 && output.len() > capacity as usize {
            return Err(NlsError32::InsufficientBuffer);
        }
        if capacity != 0 && args[4] == 0 {
            return Err(NlsError32::InvalidParameter);
        }
        let flag = u32::from(used).to_le_bytes();
        let mut spans = [(GuestAddress(0), &[][..]); 2];
        let mut count = 0;
        if capacity != 0 {
            spans[count] = (GuestAddress(args[4]), &output);
            count += 1;
        }
        if args[7] != 0 {
            spans[count] = (GuestAddress(args[7]), &flag);
            count += 1;
        }
        memory.write_spans(&spans[..count])?;
        Ok(output.len() as u32)
    }
}
