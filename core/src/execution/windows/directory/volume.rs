use super::{Access, Directory, DispatchError, GuestMemory, guest, paths, thread};

const SECTOR_BYTES: u32 = 512;
const SECTORS_PER_CLUSTER: u32 = 8;
const CLUSTER_BYTES: u64 = (SECTOR_BYTES * SECTORS_PER_CLUSTER) as u64;

impl Directory {
    pub(super) fn disk_geometry(
        &self,
        args: &[u32],
        teb: thread::Teb,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if args[1..].contains(&0) {
            return Err(DispatchError::Unsupported);
        }
        let drive = if args[0] == 0 {
            self.terminated[0]
        } else {
            let root = paths::read(memory, args[0])?;
            if root.len() != 3 || !root[0].is_ascii_alphabetic() || &root[1..] != b":\\" {
                return Err(DispatchError::Unsupported);
            }
            root[0]
        };
        if !self
            .all_paths()
            .any(|path| path[0].eq_ignore_ascii_case(&drive))
        {
            return super::failed(teb, memory, 3);
        }
        let (total, free) = self.volume_clusters(drive)?;
        for &output in &args[1..] {
            guest::check(memory, output, 4, Access::Write)?;
        }
        for (&output, value) in
            args[1..]
                .iter()
                .zip([SECTORS_PER_CLUSTER, SECTOR_BYTES, free, total])
        {
            guest::write_word(memory, output, value)?;
        }
        Ok(1)
    }

    fn volume_clusters(&self, drive: u8) -> Result<(u32, u32), DispatchError> {
        // one permanent catalog cluster plus initial payload allocation; no host geometry.
        let (mut total, mut free) = (1_u32, 0_u32);
        for file in self
            .files
            .iter()
            .filter(|file| file.path[0].eq_ignore_ascii_case(&drive))
        {
            let clusters = u32::try_from(file.size.div_ceil(CLUSTER_BYTES))
                .map_err(|_| DispatchError::Unsupported)?;
            total = total
                .checked_add(clusters)
                .ok_or(DispatchError::Unsupported)?;
            if file.removed {
                free = free
                    .checked_add(clusters)
                    .ok_or(DispatchError::Unsupported)?;
            }
        }
        Ok((total, free))
    }
}
