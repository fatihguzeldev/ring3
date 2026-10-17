use super::*;
use crate::process::windows::user32::{callback::Continuation32, memory::Scratch32};
#[derive(Clone, Copy)]
enum Action32 {
    Change(u64),
    Notice,
    CreateHeader,
    LayoutHeader,
    DeleteAll(u64),
}
#[derive(Clone, Copy)]
enum Stage32 {
    Focus,
    Capture,
    Release,
    Changing,
    Apply,
    Changed,
    Insert,
    Next,
    Notify,
    CreateHeader,
    LayoutHeader,
    DeleteAll,
    DeleteNext,
    DeleteNotify,
    Finish,
}
#[derive(Clone, Copy)]
enum Child32 {
    Focus,
    Capture,
    Release,
    Changing,
    Changed,
    Notify,
    Header,
    Layout,
    DeleteAll,
    DeleteItem,
}
#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct ListViewCall32 {
    window: WindowHandle32,
    parent: Option<WindowHandle32>,
    call: User32CallContext32,
    action: Action32,
    stage: Stage32,
    scratch: Option<Scratch32>,
    note: [u32; 12],
    result: u32,
    click: Option<(i32, i32, i32, Point32)>,
    child: Option<(u64, Child32)>,
    completed: Option<u64>,
    suppress: bool,
    capture: bool,
}
impl ListViewCall32 {
    pub(in crate::process::windows::user32) fn scratch(self) -> Option<Scratch32> {
        self.scratch
    }
}
impl User32Context32 {
    fn prepare_listview_call(
        &mut self,
        window: WindowHandle32,
        action: Action32,
        stage: Stage32,
        ram: &mut AddressSpace,
        call: User32CallContext32,
        result: u32,
        notify: bool,
    ) -> Result<ListViewCall32, E> {
        self.prepare_thread_slots()?;
        if self.pending.len() + 2 > self.continuation_limit {
            return Err(E::Capacity);
        }
        self.next_token.checked_add(2).ok_or(E::Capacity)?;
        let data = self.state.record(window)?;
        let note = [window.get(), data.menu, 0, u32::MAX, 0, 0, 0, 0, 0, 0, 0, 0];
        let parent = data.parent;
        let scratch = if notify {
            Some(Scratch32::allocate(
                ram,
                self.scratch_bounds,
                &note
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )?)
        } else {
            None
        };
        Ok(ListViewCall32 {
            window,
            parent,
            call,
            action,
            stage,
            scratch,
            note,
            result,
            click: None,
            child: None,
            completed: None,
            suppress: false,
            capture: false,
        })
    }
    pub(super) fn prepare_listview_notice(
        &mut self,
        window: WindowHandle32,
        code: i32,
        index: i32,
        subitem: i32,
        new: u32,
        old: u32,
        changed: u32,
        data: u32,
        point: Point32,
        result: u32,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<ListViewCall32, E> {
        let mut state = self.prepare_listview_call(
            window,
            Action32::Notice,
            Stage32::Notify,
            ram,
            call,
            result,
            true,
        )?;
        state.note = [
            window.get(),
            self.state.record(window)?.menu,
            code as u32,
            index as u32,
            subitem as u32,
            new,
            old,
            changed,
            point.x as u32,
            point.y as u32,
            data,
            u32::from(call.control) * 2 + u32::from(call.shift) * 4,
        ];
        Ok(state)
    }
    pub(super) fn publish_listview_call(
        &mut self,
        call: ListViewCall32,
    ) -> Result<User32Outcome32, E> {
        self.start(Continuation32::ListView(call))
    }
    pub(super) fn begin_listview_changes(
        &mut self,
        window: WindowHandle32,
        mut patches: Vec<Patch32>,
        ram: &mut AddressSpace,
        call: User32CallContext32,
        focus: bool,
        result: u32,
        click: Option<(i32, i32, i32, Point32)>,
    ) -> Result<User32Outcome32, E> {
        self.normalize_listview_selection(window, &mut patches)?;
        let limit = self.continuation_limit;
        let change = self.listview_mut(window)?.prepare_change(patches, limit)?;
        let mut state = self.prepare_listview_call(
            window,
            Action32::Change(change.id),
            if focus {
                Stage32::Focus
            } else {
                Stage32::Changing
            },
            ram,
            call,
            result,
            true,
        )?;
        state.click = click;
        state.capture = focus;
        self.listview_mut(window)?.publish_change(change);
        self.publish_listview_call(state)
    }
    fn normalize_listview_selection(
        &self,
        window: WindowHandle32,
        patches: &mut Vec<Patch32>,
    ) -> Result<(), E> {
        let focused = patches
            .iter()
            .rev()
            .find(|patch| patch.mask & 8 != 0 && patch.state_mask & 1 != 0 && patch.state & 1 != 0)
            .map(|patch| patch.key);
        let selected = if self.state.record(window)?.style & 4 != 0 {
            patches
                .iter()
                .rev()
                .find(|patch| {
                    patch.mask & 8 != 0 && patch.state_mask & 2 != 0 && patch.state & 2 != 0
                })
                .map(|patch| patch.key)
        } else {
            None
        };
        if let Some(winner) = selected {
            for patch in patches.iter_mut() {
                if patch.key != winner && patch.mask & 8 != 0 && patch.state_mask & 2 != 0 {
                    patch.state &= !2;
                }
            }
            for item in &self.listview(window)?.items {
                if item.key == winner || item.state & 2 == 0 {
                    continue;
                }
                if let Some(patch) = patches.iter_mut().find(|patch| patch.key == item.key) {
                    patch.mask |= 8;
                    patch.state_mask |= 2;
                    patch.state &= !2;
                } else {
                    patches.try_reserve(1).map_err(|_| E::Allocation)?;
                    patches.insert(
                        0,
                        Patch32 {
                            key: item.key,
                            revision: item.revision,
                            subitem: 0,
                            mask: 8,
                            state: 0,
                            state_mask: 2,
                            image: 0,
                            data: 0,
                            indent: 0,
                            text: None,
                        },
                    );
                }
            }
        }
        for winner in [focused, selected].into_iter().flatten() {
            if let Some(index) = patches.iter().position(|patch| patch.key == winner) {
                let target = patches.remove(index);
                patches.push(target);
            }
        }
        Ok(())
    }
    pub(super) fn begin_listview_insertion(
        &mut self,
        window: WindowHandle32,
        index: usize,
        item: Item32,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        self.listview_mut(window)?.prepare_insert(&item)?;
        let single = self.state.record(window)?.style & 4 != 0;
        let exclusive = (item.state & 1) | if single { item.state & 2 } else { 0 };
        let list = self.listview(window)?;
        let mut patches = Vec::new();
        patches
            .try_reserve_exact(list.items.len())
            .map_err(|_| E::Allocation)?;
        for old in &list.items {
            if old.state & exclusive != 0 {
                patches.push(Patch32 {
                    key: old.key,
                    revision: old.revision,
                    subitem: 0,
                    mask: 8,
                    state: 0,
                    state_mask: exclusive,
                    image: 0,
                    data: 0,
                    indent: 0,
                    text: None,
                });
            }
        }
        let limit = self.continuation_limit;
        let mut change = self.listview_mut(window)?.prepare_change(patches, limit)?;
        change.insertion = Some((index, item));
        let state = self.prepare_listview_call(
            window,
            Action32::Change(change.id),
            Stage32::Changing,
            ram,
            call,
            u32::MAX,
            true,
        )?;
        self.listview_mut(window)?.publish_change(change);
        self.publish_listview_call(state)
    }
    pub(super) fn begin_listview_delete_all(
        &mut self,
        window: WindowHandle32,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        let list = self.listview(window)?;
        let mut patches = Vec::new();
        patches
            .try_reserve_exact(list.items.len())
            .map_err(|_| E::Allocation)?;
        for item in &list.items {
            patches.push(Patch32 {
                key: item.key,
                revision: item.revision,
                subitem: 0,
                mask: 0,
                state: 0,
                state_mask: 0,
                image: 0,
                data: item.data,
                indent: 0,
                text: None,
            });
        }
        let limit = self.continuation_limit;
        let change = self.listview_mut(window)?.prepare_change(patches, limit)?;
        let state = self.prepare_listview_call(
            window,
            Action32::DeleteAll(change.id),
            Stage32::DeleteAll,
            ram,
            call,
            1,
            true,
        )?;
        self.listview_mut(window)?.publish_change(change);
        self.publish_listview_call(state)
    }
    pub(super) fn begin_listview_click(
        &mut self,
        window: WindowHandle32,
        code: i32,
        index: i32,
        subitem: i32,
        point: Point32,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        let data = self
            .listview(window)?
            .items
            .get(index as usize)
            .map_or(0, |item| item.data);
        let mut state = self.prepare_listview_notice(
            window, code, index, subitem, 0, 0, 0, data, point, 0, ram, call,
        )?;
        if self.windows().capture() == Some(window) {
            state.stage = Stage32::Release;
        }
        self.publish_listview_call(state)
    }
    fn listview_call(&self, token: u64) -> Result<ListViewCall32, E> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)
            .ok_or(E::InvalidToken)?;
        let Continuation32::ListView(call) = pending.continuation else {
            return Err(E::InvalidToken);
        };
        Ok(call)
    }
    fn save_listview_call(&mut self, token: u64, call: ListViewCall32) {
        self.pending
            .iter_mut()
            .find(|pending| pending.token == token)
            .expect("same list-view continuation")
            .continuation = Continuation32::ListView(call);
    }
    pub(in crate::process::windows::user32) fn listview_wait(&self, token: u64) -> bool {
        self.pending.last().is_some_and(|pending| {
            pending.token == token && matches!(pending.continuation, Continuation32::ListView(_))
        })
    }
    pub(in crate::process::windows::user32) fn pending_listview_message(
        &self,
        token: u64,
    ) -> Result<Option<[u32; 4]>, E> {
        let state = self.listview_call(token)?;
        if state.child.is_some()
            || !matches!(
                state.stage,
                Stage32::Changing
                    | Stage32::Changed
                    | Stage32::Notify
                    | Stage32::DeleteAll
                    | Stage32::DeleteNotify
            )
            || self.windows().get(state.window).is_err()
        {
            return Ok(None);
        }
        if matches!(state.stage, Stage32::Changing) {
            let Action32::Change(id) = state.action else {
                return Err(E::InvalidToken);
            };
            let list = self.listview(state.window)?;
            let Some(change) = list.change(id) else {
                return Ok(None);
            };
            let Some(patch) = change.patches.get(change.cursor) else {
                return Ok(None);
            };
            if list
                .index(patch.key)
                .is_none_or(|index| list.items[index].revision != patch.revision)
            {
                return Ok(None);
            }
        }
        Ok(state
            .parent
            .filter(|parent| self.windows().get(*parent).is_ok())
            .map(|parent| {
                [
                    parent.get(),
                    0x4e,
                    state.note[1],
                    state.scratch.unwrap().address.0,
                ]
            }))
    }
    pub(in crate::process::windows::user32) fn preview_listview_input_flags(
        &self,
        token: u64,
    ) -> Result<Option<(bool, bool)>, E> {
        if !self.listview_wait(token) {
            return Ok(None);
        }
        let state = self.listview_call(token)?;
        if self.windows().get(state.window).is_err() {
            return Ok(None);
        }
        match state.stage {
            Stage32::Focus => {
                self.windows()
                    .check_set_focus(state.call.thread_id, Some(state.window))?;
                Ok(Some((true, self.windows().capture().is_some())))
            }
            Stage32::Capture => {
                self.windows()
                    .check_set_capture(state.call.thread_id, Some(state.window))?;
                Ok(Some((self.windows().focus().is_some(), true)))
            }
            Stage32::Release if self.windows().capture() == Some(state.window) => {
                self.windows()
                    .check_set_capture(state.call.thread_id, None)?;
                Ok(Some((self.windows().focus().is_some(), false)))
            }
            _ => Ok(None),
        }
    }
    fn send_listview_note(
        &mut self,
        token: u64,
        mut state: ListViewCall32,
        kind: Child32,
        ram: &mut AddressSpace,
    ) -> Result<Option<User32Outcome32>, E> {
        let scratch = state.scratch.ok_or(E::InvalidToken)?;
        scratch.check(ram)?;
        let bytes = state
            .note
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        ram.write(scratch.address, &bytes)?;
        if let Some(message) = self.pending_listview_message(token)? {
            let outcome = self.send_message(&message, state.call)?;
            let child = child_token(outcome).ok_or(E::InvalidRequest)?;
            state.child = Some((child, kind));
            state.completed = None;
            self.save_listview_call(token, state);
            Ok(Some(outcome))
        } else {
            self.after_listview_child(&mut state, kind, 0);
            self.save_listview_call(token, state);
            Ok(None)
        }
    }
    fn after_listview_child(&mut self, state: &mut ListViewCall32, kind: Child32, result: u32) {
        state.stage = match kind {
            Child32::Focus => {
                if state.capture {
                    Stage32::Capture
                } else {
                    Stage32::Changing
                }
            }
            Child32::Capture => Stage32::Changing,
            Child32::Release => Stage32::Notify,
            Child32::Changing if result != 0 => {
                state.result = 0;
                Stage32::Next
            }
            Child32::Changing => Stage32::Apply,
            Child32::Changed => Stage32::Next,
            Child32::Notify => Stage32::Finish,
            Child32::Header => {
                if result == 0 {
                    state.result = u32::MAX;
                }
                if result != 0
                    && let Ok(header) = WindowHandle32::try_from(result)
                    && self.windows().get(header).is_ok()
                    && let Ok(list) = self.listview_mut(state.window)
                {
                    list.header = Some(header);
                }
                Stage32::Finish
            }
            Child32::Layout => Stage32::Finish,
            Child32::DeleteAll => {
                state.suppress = result != 0;
                Stage32::DeleteNext
            }
            Child32::DeleteItem => Stage32::DeleteNext,
        };
    }
    pub(in crate::process::windows::user32) fn resume_listview_call(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        let mut state = self.listview_call(token)?;
        if state.child.is_some() {
            return Err(E::InvalidToken);
        }
        if let Some(scratch) = state.scratch {
            scratch.check(ram)?;
        }
        if self.windows().get(state.window).is_err() {
            state.stage = Stage32::Finish;
        }
        match state.stage {
            Stage32::Focus => {
                state.stage = if state.capture {
                    Stage32::Capture
                } else {
                    Stage32::Changing
                };
                if self.windows().focus() != Some(state.window) {
                    let outcome = self.prepare_arguments(
                        User32Api32::SetFocus,
                        &[state.window.get()],
                        ram,
                        state.call,
                    )?;
                    if let Some(child) = child_token(outcome) {
                        state.child = Some((child, Child32::Focus));
                        state.completed = None;
                        self.save_listview_call(token, state);
                        return Ok(outcome);
                    }
                }
            }
            Stage32::Capture | Stage32::Release => {
                let capture = matches!(state.stage, Stage32::Capture);
                let capture_arguments = [state.window.get()];
                let outcome = self.prepare_arguments(
                    if capture {
                        User32Api32::SetCapture
                    } else {
                        User32Api32::ReleaseCapture
                    },
                    if capture { &capture_arguments } else { &[] },
                    ram,
                    state.call,
                )?;
                if let Some(child) = child_token(outcome) {
                    state.child = Some((
                        child,
                        if capture {
                            Child32::Capture
                        } else {
                            Child32::Release
                        },
                    ));
                    state.completed = None;
                    self.save_listview_call(token, state);
                    return Ok(outcome);
                }
                state.stage = if capture {
                    Stage32::Changing
                } else {
                    Stage32::Notify
                };
            }
            Stage32::Changing => {
                let Action32::Change(id) = state.action else {
                    return Err(E::InvalidToken);
                };
                let list = self.listview(state.window)?;
                let patch = list
                    .change(id)
                    .and_then(|change| change.patches.get(change.cursor));
                if let Some(patch) = patch
                    && let Some(index) = list.index(patch.key)
                    && list.items[index].revision == patch.revision
                {
                    let item = &list.items[index];
                    state.note[2] = (-100i32) as u32;
                    state.note[3] = index as u32;
                    state.note[4] = patch.subitem as u32;
                    state.note[5] = if patch.mask & 8 != 0 {
                        (item.state & !patch.state_mask) | (patch.state & patch.state_mask)
                    } else {
                        item.state
                    };
                    state.note[6] = item.state;
                    state.note[7] = patch.mask;
                    state.note[10] = item.data;
                    self.save_listview_call(token, state);
                    if let Some(outcome) =
                        self.send_listview_note(token, state, Child32::Changing, ram)?
                    {
                        return Ok(outcome);
                    }
                    return Ok(User32Outcome32::Wait {
                        continuation: token,
                    });
                } else if patch.is_some() {
                    state.stage = Stage32::Next
                } else {
                    state.stage = if list
                        .change(id)
                        .is_some_and(|change| change.insertion.is_some())
                    {
                        Stage32::Insert
                    } else {
                        Stage32::Finish
                    }
                }
            }
            Stage32::Apply => {
                let Action32::Change(id) = state.action else {
                    return Err(E::InvalidToken);
                };
                self.state.check_paint_change()?;
                let single = self.state.record(state.window)?.style & 4 != 0;
                if self.listview_mut(state.window)?.apply(id, single)? {
                    self.listview_changed(state.window);
                    state.stage = Stage32::Changed;
                } else {
                    state.result = 0;
                    state.stage = Stage32::Next;
                }
            }
            Stage32::Changed => {
                state.note[2] = (-101i32) as u32;
                self.save_listview_call(token, state);
                if let Some(outcome) =
                    self.send_listview_note(token, state, Child32::Changed, ram)?
                {
                    return Ok(outcome);
                }
                return Ok(User32Outcome32::Wait {
                    continuation: token,
                });
            }
            Stage32::Next => {
                let Action32::Change(id) = state.action else {
                    return Err(E::InvalidToken);
                };
                self.listview_mut(state.window)?.advance(id);
                if self
                    .listview(state.window)?
                    .change(id)
                    .is_some_and(|change| change.cursor < change.patches.len())
                {
                    state.stage = Stage32::Changing
                } else if let Some((code, index, sub, point)) = state.click.take() {
                    state.note[2] = code as u32;
                    state.note[3] = index as u32;
                    state.note[4] = sub as u32;
                    state.note[8] = point.x as u32;
                    state.note[9] = point.y as u32;
                    state.note[10] = self
                        .listview(state.window)?
                        .items
                        .get(index as usize)
                        .map_or(0, |item| item.data);
                    state.stage = Stage32::Notify;
                } else {
                    state.stage = if self
                        .listview(state.window)?
                        .change(id)
                        .is_some_and(|change| change.insertion.is_some())
                    {
                        Stage32::Insert
                    } else {
                        Stage32::Finish
                    };
                }
            }
            Stage32::Insert => {
                let Action32::Change(id) = state.action else {
                    return Err(E::InvalidToken);
                };
                self.state.check_paint_change()?;
                let single = self.state.record(state.window)?.style & 4 != 0;
                if let Some((index, new, data)) = self
                    .listview_mut(state.window)?
                    .finish_insertion(id, single)?
                {
                    self.listview_changed(state.window);
                    state.result = index as u32;
                    state.note = [
                        state.window.get(),
                        state.note[1],
                        (-102i32) as u32,
                        index as u32,
                        0,
                        new,
                        0,
                        0,
                        0,
                        0,
                        data,
                        0,
                    ];
                    state.stage = Stage32::Notify;
                } else {
                    state.stage = Stage32::Finish;
                }
            }
            Stage32::Notify => {
                if let Some(outcome) =
                    self.send_listview_note(token, state, Child32::Notify, ram)?
                {
                    return Ok(outcome);
                }
                return Ok(User32Outcome32::Wait {
                    continuation: token,
                });
            }
            Stage32::DeleteAll => {
                state.note[2] = (-104i32) as u32;
                state.note[3] = u32::MAX;
                self.save_listview_call(token, state);
                if let Some(outcome) =
                    self.send_listview_note(token, state, Child32::DeleteAll, ram)?
                {
                    return Ok(outcome);
                }
                return Ok(User32Outcome32::Wait {
                    continuation: token,
                });
            }
            Stage32::DeleteNext => {
                let Action32::DeleteAll(id) = state.action else {
                    return Err(E::InvalidToken);
                };
                let list = self.listview(state.window)?;
                let patch = list
                    .change(id)
                    .and_then(|change| change.patches.get(change.cursor));
                if let Some(patch) = patch {
                    let key = patch.key;
                    let index = list.index(key);
                    self.state.check_paint_change()?;
                    if let Some(index) = index {
                        let item = self.listview_mut(state.window)?.delete(index);
                        self.listview_changed(state.window);
                        state.note = [
                            state.window.get(),
                            state.note[1],
                            (-103i32) as u32,
                            index as u32,
                            0,
                            0,
                            item.state,
                            0,
                            0,
                            0,
                            item.data,
                            0,
                        ];
                    }
                    self.listview_mut(state.window)?.advance(id);
                    if index.is_some() && !state.suppress {
                        state.stage = Stage32::DeleteNotify;
                    }
                } else {
                    state.stage = Stage32::Finish;
                }
            }
            Stage32::DeleteNotify => {
                if let Some(outcome) =
                    self.send_listview_note(token, state, Child32::DeleteItem, ram)?
                {
                    return Ok(outcome);
                }
                return Ok(User32Outcome32::Wait {
                    continuation: token,
                });
            }
            Stage32::CreateHeader => {
                let atom = self
                    .state
                    .common_atom(crate::windows::comctl32::CommonControlKind32::Header)
                    .ok_or(E::Win32(1407))?;
                let extent = self.windows().get(state.window)?.description.extent;
                let style = self.state.record(state.window)?.style;
                let height = state.note[0];
                let outcome = self.create_window(
                    User32Api32::CreateWindowExW,
                    &[
                        0,
                        atom as u32,
                        0,
                        0x40000002
                            | if style & 3 == 1 && style & 0x4000 == 0 {
                                0x10000000
                            } else {
                                0
                            },
                        0,
                        0,
                        extent.width,
                        height,
                        state.window.get(),
                        0,
                        state.call.instance,
                        0,
                    ],
                    ram,
                    state.call,
                )?;
                let child = child_token(outcome).ok_or(E::InvalidRequest)?;
                state.child = Some((child, Child32::Header));
                state.completed = None;
                self.save_listview_call(token, state);
                return Ok(outcome);
            }
            Stage32::LayoutHeader => {
                let header = self
                    .listview(state.window)?
                    .header
                    .ok_or(E::InvalidRequest)?;
                let extent = self.windows().get(state.window)?.description.extent;
                let position = self.prepare_window_position(
                    User32Api32::SetWindowPos,
                    &[header.get(), 0, 0, 0, extent.width, state.note[0], 0x14],
                    ram,
                    state.call,
                )?;
                let outcome = self.start(position)?;
                let child = child_token(outcome).ok_or(E::InvalidRequest)?;
                state.child = Some((child, Child32::Layout));
                state.completed = None;
                self.save_listview_call(token, state);
                return Ok(outcome);
            }
            Stage32::Finish => {
                if let Some(scratch) = state.scratch {
                    scratch.release(ram)?;
                }
                if let Action32::Change(id) | Action32::DeleteAll(id) = state.action
                    && let Ok(list) = self.listview_mut(state.window)
                {
                    list.retire(id);
                }
                self.pending.pop();
                return Ok(User32Outcome32::Return(state.result));
            }
        }
        self.save_listview_call(token, state);
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }
    pub(super) fn begin_listview_header(
        &mut self,
        window: WindowHandle32,
        ram: &mut AddressSpace,
        gdi: &GdiRuntime32,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        if self.listview(window)?.header.is_some() {
            return Ok(User32Outcome32::Return(0));
        }
        if self.listview_wait(self.pending.last().map_or(0, |entry| entry.token))
            && let Some(pending) = self.pending.last()
        {
            return self.resume_listview_call(pending.token, ram);
        }
        let font = self.listview_font(window).map_err(|_| E::InvalidRequest)?;
        let height = gdi
            .font_metrics(font)
            .map_err(|error| E::Win32(error.win32()))?
            .height
            .saturating_add(2)
            .max(1);
        let mut state = self.prepare_listview_call(
            window,
            Action32::CreateHeader,
            Stage32::CreateHeader,
            ram,
            call,
            0,
            false,
        )?;
        state.note[0] = height as u32;
        self.publish_listview_call(state)
    }
    pub(super) fn layout_listview_header(
        &mut self,
        window: WindowHandle32,
        ram: &mut AddressSpace,
        gdi: &GdiRuntime32,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        if self.listview(window)?.header.is_none() {
            return Ok(User32Outcome32::Return(0));
        }
        let font = self.listview_font(window).map_err(|_| E::InvalidRequest)?;
        let height = gdi
            .font_metrics(font)
            .map_err(|error| E::Win32(error.win32()))?
            .height
            .saturating_add(2)
            .max(1);
        let mut state = self.prepare_listview_call(
            window,
            Action32::LayoutHeader,
            Stage32::LayoutHeader,
            ram,
            call,
            0,
            false,
        )?;
        state.note[0] = height as u32;
        self.publish_listview_call(state)
    }
    pub(in crate::process::windows::user32) fn complete_listview_child(
        &mut self,
        child: u64,
        outcome: User32Outcome32,
    ) -> Result<User32Outcome32, E> {
        let User32Outcome32::Return(result) = outcome else {
            return Ok(outcome);
        };
        let Some(pending) = self.pending.last() else {
            return Ok(outcome);
        };
        let Continuation32::ListView(mut state) = pending.continuation else {
            return Ok(outcome);
        };
        let Some((expected, kind)) = state.child else {
            return Ok(outcome);
        };
        if expected != child {
            return Ok(outcome);
        }
        let token = pending.token;
        state.child = None;
        state.completed = Some(child);
        self.after_listview_child(&mut state, kind, result);
        self.save_listview_call(token, state);
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }
    pub(in crate::process::windows::user32) fn retry_listview_completion(
        &self,
        child: u64,
    ) -> Option<User32Outcome32> {
        let pending = self.pending.last()?;
        let Continuation32::ListView(state) = pending.continuation else {
            return None;
        };
        (state.child.is_none() && state.completed == Some(child)).then_some(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }
}
fn child_token(outcome: User32Outcome32) -> Option<u64> {
    match outcome {
        User32Outcome32::Callback(callback) => Some(callback.continuation),
        User32Outcome32::Wait { continuation } => Some(continuation),
        _ => None,
    }
}
