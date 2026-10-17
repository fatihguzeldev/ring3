use super::{FileEntry32, template};
use crate::{
    process::windows::user32::User32Context32,
    windows::{controls::ControlState32, user32::User32Error32 as E, window::WindowHandle32},
};

impl User32Context32 {
    pub(super) fn file_dialog_control(
        &self,
        root: WindowHandle32,
        id: u32,
    ) -> Result<WindowHandle32, E> {
        self.state.dialog_item(root, id)?.ok_or(E::Win32(1400))
    }
    pub(super) fn populate_file_dialog(&mut self, token: u64) -> Result<(), E> {
        let plan = &self.file_dialogs.plans[self.file_dialogs.index(token)?];
        let root = plan.root.ok_or(E::InvalidToken)?;
        let files = self.file_dialog_control(root, template::FILES)?;
        let directory = self.file_dialog_control(root, template::DIRECTORY)?;
        let filter = self.file_dialog_control(root, template::FILTER)?;
        let name = self.file_dialog_control(root, template::NAME)?;
        let mut files_state = list_state(self.state.control_state(files)?);
        for entry in &plan.entries {
            files_state.insert(files_state.items.len(), entry.name.clone())?;
        }
        let mut directory_state = list_state(self.state.control_state(directory)?);
        directory_state.insert(0, plan.directory.clone())?;
        directory_state.selected = Some(0);
        let mut filter_state = list_state(self.state.control_state(filter)?);
        if let Some(custom) = &plan.request.filters.custom {
            filter_state.insert(0, custom.label.clone())?;
        }
        for entry in &plan.request.filters.entries {
            filter_state.insert(filter_state.items.len(), entry.label.clone())?;
        }
        filter_state.selected = if plan.filter_index == 0 && plan.request.filters.custom.is_some() {
            Some(0)
        } else {
            Some(
                plan.filter_index.max(1) as usize - 1
                    + usize::from(plan.request.filters.custom.is_some()),
            )
            .filter(|&index| index < filter_state.items.len())
        };
        let text = plan
            .request
            .initial_file
            .iter()
            .rposition(|&unit| unit == 92)
            .map_or(plan.request.initial_file.as_slice(), |at| {
                &plan.request.initial_file[at + 1..]
            })
            .to_vec();
        let readonly = self.state.dialog_item(root, template::READ_ONLY)?;
        if let Some(readonly) = readonly {
            self.state.control_state(readonly)?;
        }
        let checked = plan.request.input.flag(1);
        self.state.check_control_changes(4)?;
        // every allocation and HWND lookup precedes the infallible prepared control publication.
        *self.state.control(files)? = files_state;
        *self.state.control(directory)? = directory_state;
        *self.state.control(filter)? = filter_state;
        self.state.set_text(name, text)?;
        if let Some(readonly) = readonly {
            self.state.control(readonly)?.check = u32::from(checked);
        }
        self.state.invalidate_control(files)?;
        self.state.invalidate_control(directory)?;
        self.state.invalidate_control(filter)?;
        self.state.invalidate_control(name)?;
        Ok(())
    }
    pub(super) fn replace_file_dialog_entries(
        &mut self,
        token: u64,
        directory: Vec<u16>,
        entries: Vec<FileEntry32>,
        filter_index: u32,
    ) -> Result<(), E> {
        let index = self.file_dialogs.index(token)?;
        let root = self.file_dialogs.plans[index].root.ok_or(E::InvalidToken)?;
        let files = self.file_dialog_control(root, template::FILES)?;
        let combo = self.file_dialog_control(root, template::DIRECTORY)?;
        let mut files_state = list_state(self.state.control_state(files)?);
        for entry in &entries {
            files_state.insert(files_state.items.len(), entry.name.clone())?;
        }
        let mut directory_state = list_state(self.state.control_state(combo)?);
        directory_state.insert(0, directory.clone())?;
        directory_state.selected = Some(0);
        self.state.check_control_changes(2)?;
        *self.state.control(files)? = files_state;
        *self.state.control(combo)? = directory_state;
        let plan = &mut self.file_dialogs.plans[index];
        plan.directory = directory;
        plan.entries = entries;
        plan.filter_index = filter_index;
        self.state.invalidate_control(files)?;
        self.state.invalidate_control(combo)?;
        Ok(())
    }
}

fn list_state(old: &ControlState32) -> ControlState32 {
    ControlState32 {
        font: old.font,
        item_height: old.item_height,
        drop_height: old.drop_height,
        popup: old.popup,
        ..Default::default()
    }
}
