use super::types::{After32, Plan32, Stage32, Window32};
use crate::{
    memory::AddressSpace,
    process::windows::user32::{
        User32CallContext32, User32Context32, User32Outcome32,
        callback::{Continuation32, Pending32},
        memory::{self, Scratch32},
    },
    windows::user32::{
        Point32, User32Api32 as Api, User32Error32 as E, WindowClass32,
        menus::{MenuHandle32, MenuTracking32},
    },
};

impl User32Context32 {
    pub(in crate::process::windows::user32) fn begin_popup_menu(
        &mut self,
        api: Api,
        args: &[u32],
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        let menu = MenuHandle32::try_from(args[0])?;
        if !self.menus.popup(menu)? {
            return Err(E::Win32(87));
        }
        let flags = args[1];
        if flags & !0x1ffff != 0
            || flags & 0x0c == 0x0c
            || flags & 0x30 == 0x30
            || (api == Api::TrackPopupMenu && args[4] != 0)
        {
            return Err(E::Win32(87));
        }
        if flags & 0x8000 != 0 {
            return Err(E::Win32(50));
        }
        let owner = self.owned(
            args[if api == Api::TrackPopupMenu { 5 } else { 4 }],
            call.thread_id,
        )?;
        let exclude = if api == Api::TrackPopupMenuEx && args[5] != 0 {
            if memory::words::<1>(ram, args[5])?[0] != 20 {
                return Err(E::Win32(87));
            }
            Some(memory::rect(
                ram,
                args[5].checked_add(4).ok_or(E::Win32(87))?,
            )?)
        } else {
            None
        };
        if flags & 1 == 0
            && self
                .menu_popups
                .plans
                .iter()
                .any(|plan| plan.call.thread_id == call.thread_id)
        {
            return Err(E::Win32(1446));
        }
        if self.menu_popups.plans.len() == self.menu_popups.limit
            || self.pending.len() + 2 > self.continuation_limit
        {
            return Err(E::Capacity);
        }
        self.prepare_thread_slots()?;
        self.state.check_window_capacity(1)?;
        let token = self.check_continuation()?;
        let tracker = MenuTracking32::new(&self.menus, menu)?;
        let mut windows = Vec::new();
        windows.try_reserve_exact(32).map_err(|_| E::Allocation)?;
        windows.push(Window32 {
            handle: None,
            menu,
            position: 0,
            origin: Point32::default(),
            visual: None,
        });
        let mut bytes = [0; 64];
        for (out, unit) in bytes[32..].chunks_exact_mut(2).zip("#32768".encode_utf16()) {
            out.copy_from_slice(&unit.to_le_bytes());
        }
        let restore_capture = self.windows().capture().filter(|&window| {
            self.menu_popups.plans.iter().any(|plan| {
                plan.call.thread_id == call.thread_id
                    && plan
                        .windows
                        .iter()
                        .any(|popup| popup.handle == Some(window))
            })
        });
        let scratch = Scratch32::allocate(ram, self.scratch_bounds, &bytes)?;
        self.menu_popups.plans.push(Plan32 {
            token,
            call,
            owner,
            root: menu,
            flags,
            anchor: Point32 {
                x: args[2] as i32,
                y: args[3] as i32,
            },
            exclude,
            tracker,
            windows,
            stage: Stage32::Enter,
            after: After32::Wait,
            character: None,
            selected: None,
            command: None,
            cancelled: false,
            captured: false,
            restore_capture,
            child: None,
            completed: None,
        });
        self.next_token = token;
        self.pending.push(Pending32 {
            token,
            continuation: Continuation32::MenuPopup {
                plan: token,
                scratch,
            },
            activation: None,
        });
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }

    pub(in crate::process::windows::user32) fn install_popup_class(
        &mut self,
        procedure: crate::memory::GuestAddress,
    ) -> Result<(), E> {
        self.state.install_system_class(WindowClass32 {
            name: "#32768".encode_utf16().collect(),
            procedure,
            unicode: true,
            style: 0x800,
            background_brush: 5,
            ..Default::default()
        })?;
        self.menu_popups.procedure = Some(procedure);
        Ok(())
    }

    pub(in crate::process::windows::user32) fn end_popup_menu(
        &mut self,
        thread: u32,
    ) -> Result<User32Outcome32, E> {
        let Some(plan) = self
            .menu_popups
            .plans
            .iter_mut()
            .rev()
            .find(|plan| plan.call.thread_id == thread)
        else {
            return Ok(User32Outcome32::Return(0));
        };
        plan.cancelled = true;
        Ok(User32Outcome32::Return(1))
    }
}
