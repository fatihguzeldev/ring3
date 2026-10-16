use super::{ProcessStartup32, bytes::StartupBytes32};
use crate::{
    loader::LoadPlan32,
    memory::{GuestAddress, PAGE_SIZE, PageRange, Permissions},
    process::{EngineInstance, HostError},
    windows::{process::ProcessError32, text::CodePage32},
};

impl EngineInstance {
    pub(in crate::process) fn stage_process_startup(
        &self,
        plan: &mut LoadPlan32,
        teb: u32,
        thread_ranges: [(u64, u64); 2],
        image_base: u32,
        root_name: &str,
    ) -> Result<ProcessStartup32, HostError> {
        let pages = (self.process_runtime.launch.startup_bytes / PAGE_SIZE as usize) as u32;
        let mut reservations = self.guest_address_reservations()?;
        for (start, end) in thread_ranges {
            reservations.push(
                PageRange::new(
                    GuestAddress(start as u32),
                    ((end - start) / u64::from(PAGE_SIZE)) as u32,
                )
                .map_err(HostError::Memory)?,
            );
        }
        let mut candidate = 0x7000_0000u32;
        let range = loop {
            let bounds = PageRange::new(
                GuestAddress(candidate),
                (0x8000_0000 - candidate) / PAGE_SIZE,
            )
            .map_err(HostError::Memory)?;
            let range = self
                .memory()?
                .find_unmapped_range(bounds, pages, 65536)
                .map_err(HostError::Memory)?;
            let start = range.first as u64 * u64::from(PAGE_SIZE);
            let end = start + u64::from(pages) * u64::from(PAGE_SIZE);
            let occupied = reservations
                .iter()
                .filter(|range| {
                    start < (range.first + range.count) as u64 * u64::from(PAGE_SIZE)
                        && range.first as u64 * u64::from(PAGE_SIZE) < end
                })
                .map(|range| (range.first + range.count) as u64 * u64::from(PAGE_SIZE))
                .max();
            if occupied.is_none()
                && !plan.intersects_image(GuestAddress(start as u32), pages * PAGE_SIZE)
            {
                break range;
            }
            candidate = u32::try_from(occupied.unwrap_or(start + 65536).div_ceil(65536) * 65536)
                .map_err(|_| HostError::WindowsProcess(ProcessError32::Capacity))?;
            if candidate >= 0x8000_0000 {
                return Err(HostError::WindowsProcess(ProcessError32::Capacity));
            }
        };
        let base = (range.first as u32) * PAGE_SIZE;
        let launch = &self.process_runtime.launch;
        let image_path = if self.process_runtime.configured {
            launch.image_path.clone()
        } else if root_name.contains('\\') {
            root_name.to_owned()
        } else {
            format!("C:\\game\\{root_name}")
        };
        let command_line = if self.process_runtime.configured {
            launch.command_line.clone()
        } else {
            root_name.into()
        };
        let mut bytes = StartupBytes32::new(base, launch.startup_bytes);
        let image = bytes.wide(&image_path).map_err(HostError::WindowsProcess)?;
        let command = bytes
            .wide(&command_line)
            .map_err(HostError::WindowsProcess)?;
        let cwd_units = if self.process_runtime.configured {
            launch.current_directory.encode_utf16().collect::<Vec<_>>()
        } else {
            self.configured_file_current_directory().map_or_else(
                || launch.current_directory.encode_utf16().collect(),
                <[u16]>::to_vec,
            )
        };
        let cwd = bytes
            .wide_units(&cwd_units)
            .map_err(HostError::WindowsProcess)?;
        let mut ansi = CodePage32::Windows1252
            .encode(&command_line.encode_utf16().collect::<Vec<_>>(), false)
            .map_err(|_| HostError::WindowsProcess(ProcessError32::InvalidParameter))?
            .0;
        ansi.push(0);
        let command_a = bytes.put(&ansi, 1).map_err(HostError::WindowsProcess)?;
        let environment = self
            .process_runtime
            .environment
            .block()
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let environment_length = environment.len();
        let environment = bytes
            .put(&environment, 2)
            .map_err(HostError::WindowsProcess)?;
        // pe32 offsets are serialized independently of the host abi.
        bytes.word(0x08, image_base);
        bytes.word(0x0c, base + 0x200);
        bytes.word(0x10, base + 0x100);
        bytes.word(0x100, 0x100);
        bytes.word(0x104, 0x100);
        bytes.word(0x108, 1);
        for (index, handle) in self.process_runtime.standard_handles.iter().enumerate() {
            bytes.word(0x118 + index * 4, *handle);
        }
        bytes.unicode(0x124, cwd);
        bytes.unicode(0x138, image);
        bytes.unicode(0x140, command);
        bytes.word(0x148, environment);
        bytes.word(0x190, self.process_runtime.startup_flags);
        bytes.word(0x200, 0x30);
        bytes.bytes[0x204] = 1;
        let (loader_entries, loader_initialization) = self.stage_process_loader(
            &mut bytes,
            plan.additions(),
            plan.initialization_order(),
            &image_path,
        )?;
        for (index, page) in bytes.bytes.chunks_exact(PAGE_SIZE as usize).enumerate() {
            plan.add_page(
                GuestAddress(base + index as u32 * PAGE_SIZE),
                Permissions::READ_WRITE,
                page,
            )
            .map_err(HostError::Module)?;
        }
        plan.stage_write(GuestAddress(teb + 0x30), &base.to_le_bytes())
            .map_err(HostError::Module)?;
        plan.stage_write(GuestAddress(teb + 0x20), &launch.process_id.to_le_bytes())
            .map_err(HostError::Module)?;
        plan.stage_write(GuestAddress(teb + 0x24), &1u32.to_le_bytes())
            .map_err(HostError::Module)?;
        Ok(ProcessStartup32 {
            range,
            peb: base,
            command_line: [command_a, command.0],
            image_path,
            bump: bytes.used,
            environment_blocks: Vec::new(),
            current_environment: (environment, environment_length),
            free_blocks: Vec::new(),
            allocation_revision: 0,
            current_directory: (cwd.0, usize::from(cwd.1) + 2),
            loader_entries,
            loader_initialization,
        })
    }
}
