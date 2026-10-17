use super::{
    User32Callback32, User32Context32, User32Outcome32,
    memory::{self, Scratch32},
};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{
        user32::{Point32, User32Error32},
        window::{WindowHandle32, WindowMessage32},
    },
};

#[derive(Clone)]
pub(super) struct Pending32 {
    pub token: u64,
    pub continuation: Continuation32,
    pub activation: Option<crate::windows::activation::CapturedActivation32>,
}

#[derive(Clone, Copy)]
pub(super) enum CreateStage32 {
    NonClientCreate,
    NonClientSize,
    Create,
    Size,
    Move,
    Show,
}

#[derive(Clone, Copy)]
pub(super) enum PositionStage32 {
    Changing,
    NonClientSize,
    Changed,
}

#[derive(Clone, Copy)]
pub(super) enum Continuation32 {
    MenuBar(super::menus::bar::MenuBar32),
    CommonLayout(super::common::layout::CommonLayout32),
    TooltipTrack(super::common::tooltip::tracking::TooltipTrack32),
    TabSelection(super::common::tab::input::TabSelect32),
    Header(super::common::header::notifications::HeaderCall32),
    StatusDraw(super::common::status::owner::StatusDraw32),
    FileDialog(super::file_dialogs::FileDialogCall32),
    FilePrompt(super::file_dialogs::FilePrompt32),
    MenuPopup {
        plan: u64,
        scratch: Scratch32,
    },
    Accelerator(super::accelerators::Dispatch32),
    Clipboard(super::clipboard::ClipboardCall32),
    SystemColors(super::system_colors::ColorChange32),
    UiState(super::ui::UiChange32),
    ResourceDialog {
        plan: u64,
        scratch: Scratch32,
        child: Option<u64>,
        completed: Option<u64>,
    },
    ResourceProcedure {
        window: WindowHandle32,
        procedure: GuestAddress,
        arguments: [u32; 4],
        wide: bool,
    },
    ControlPaint(super::controls::painting::ControlPaint32),
    ControlDropdown(super::controls::Dropdown32),
    ControlFocus(super::controls::Focus32),
    ControlClose(super::controls::Close32),
    QueryPaint(super::gdi::QueryPaint32),
    SentPaint(super::gdi::SentPaint32),
    Paint {
        window: WindowHandle32,
        output: super::gdi::PaintOutput32,
        dc: u32,
    },
    Dialog {
        id: u64,
    },
    DialogHelp {
        id: u64,
        scratch: Scratch32,
    },
    DialogRemoteHelp {
        id: u64,
        scratch: Scratch32,
        ready: Option<u32>,
    },
    Show(super::activation::Show32),
    Destroy {
        plan: u64,
    },
    Retrieve {
        api: crate::windows::user32::User32Api32,
        arguments: [u32; 5],
        thread: u32,
    },
    RemoteCall {
        ready: Option<u32>,
    },
    Timer {
        procedure: GuestAddress,
        arguments: [u32; 4],
    },
    Call {
        procedure: GuestAddress,
        arguments: [u32; 4],
    },
    Sequence {
        events: [Option<WindowMessage32>; 6],
        index: usize,
        result: u32,
    },
    Create {
        window: WindowHandle32,
        thread: u32,
        scratch: Scratch32,
        stage: CreateStage32,
        visible: bool,
    },
    Position {
        window: WindowHandle32,
        scratch: Scratch32,
        stage: PositionStage32,
    },
    MessageWait {
        thread: u32,
        output: u32,
        window: u32,
        min: u32,
        max: u32,
    },
    Style {
        window: WindowHandle32,
        index: i32,
        old: u32,
        scratch: Scratch32,
        changed: bool,
    },
}

impl User32Context32 {
    pub(super) fn next_outcome(&mut self) -> Result<User32Outcome32, User32Error32> {
        let pending = self
            .pending
            .last()
            .cloned()
            .ok_or(User32Error32::InvalidToken)?;
        let (procedure, arguments) = match pending.continuation {
            Continuation32::ResourceDialog { .. }
            | Continuation32::FilePrompt(_)
            | Continuation32::MenuPopup { .. }
            | Continuation32::MenuBar(_)
            | Continuation32::CommonLayout(_)
            | Continuation32::TooltipTrack(_)
            | Continuation32::Header(_)
            | Continuation32::TabSelection(_)
            | Continuation32::StatusDraw(_)
            | Continuation32::Accelerator(_)
            | Continuation32::Clipboard(_)
            | Continuation32::SystemColors(_)
            | Continuation32::UiState(_)
            | Continuation32::SentPaint(_) => {
                return Ok(User32Outcome32::Wait {
                    continuation: pending.token,
                });
            }
            Continuation32::ResourceProcedure {
                procedure,
                arguments,
                ..
            } => (procedure, arguments),
            Continuation32::FileDialog(call) => self.file_dialog_callback(call)?,
            Continuation32::ControlPaint(paint) => self.control_paint_callback(paint)?,
            Continuation32::ControlFocus(focus) => {
                let event = focus
                    .events
                    .get(focus.index)
                    .copied()
                    .flatten()
                    .ok_or(User32Error32::InvalidToken)?;
                (
                    event.procedure,
                    [
                        event.window.get(),
                        event.message,
                        event.wparam,
                        event.lparam,
                    ],
                )
            }
            Continuation32::ControlClose(close) => {
                let Some(event) = close.events.get(close.index).copied().flatten() else {
                    return self.complete_control_close(close);
                };
                (
                    event.procedure,
                    [
                        event.window.get(),
                        event.message,
                        event.wparam,
                        event.lparam,
                    ],
                )
            }
            Continuation32::ControlDropdown(drop) => {
                let Some(event) = drop.events.get(drop.index).copied().flatten() else {
                    return self.complete_control_dropdown(drop);
                };
                (
                    event.procedure,
                    [
                        event.window.get(),
                        event.message,
                        event.wparam,
                        event.lparam,
                    ],
                )
            }
            Continuation32::QueryPaint(paint) => (
                self.windows().get(paint.window)?.description.procedure,
                [paint.window.get(), 0x14, paint.dc, 0],
            ),
            Continuation32::Paint { window, dc, .. } => (
                self.windows().get(window)?.description.procedure,
                [window.get(), 0x14, dc, 0],
            ),
            Continuation32::Dialog { .. } | Continuation32::DialogRemoteHelp { .. } => {
                return Ok(User32Outcome32::Wait {
                    continuation: pending.token,
                });
            }
            Continuation32::DialogHelp { .. } => return Err(User32Error32::InvalidToken),
            Continuation32::Show(show) => return self.show_outcome(show),
            Continuation32::Destroy { plan } => return self.destroy_outcome(plan),
            Continuation32::Call {
                procedure,
                arguments,
            }
            | Continuation32::Timer {
                procedure,
                arguments,
            } => (procedure, arguments),
            Continuation32::Sequence {
                events,
                index,
                result,
            } => {
                if let Some(event) = events.get(index).copied().flatten() {
                    (
                        event.procedure,
                        [
                            event.window.get(),
                            event.message,
                            event.wparam,
                            event.lparam,
                        ],
                    )
                } else {
                    self.pending.pop();
                    return Ok(User32Outcome32::Return(result));
                }
            }
            Continuation32::Create {
                window,
                scratch,
                stage,
                ..
            } => {
                let record = self.windows().get(window)?;
                let position = self.state.client_position(window)?;
                let (message, wparam, lparam) = match stage {
                    CreateStage32::NonClientCreate => (0x0081, 0, scratch.address.0),
                    CreateStage32::NonClientSize => (0x0083, 0, scratch.address.0 + 48),
                    CreateStage32::Create => (0x0001, 0, scratch.address.0),
                    CreateStage32::Size => (
                        0x0005,
                        0,
                        (record.description.extent.width & 0xffff)
                            | (record.description.extent.height << 16),
                    ),
                    CreateStage32::Move => (
                        0x0003,
                        0,
                        (position.x as u32 & 0xffff) | ((position.y as u32) << 16),
                    ),
                    CreateStage32::Show => (0x0018, 1, 0),
                };
                (
                    record.description.procedure,
                    [window.get(), message, wparam, lparam],
                )
            }
            Continuation32::Position {
                window,
                scratch,
                stage,
            } => {
                let (message, wparam, lparam) = match stage {
                    PositionStage32::Changing => (0x0046, 0, scratch.address.0),
                    PositionStage32::NonClientSize => (0x0083, 0, scratch.address.0 + 28),
                    PositionStage32::Changed => (0x0047, 0, scratch.address.0),
                };
                (
                    self.windows().get(window)?.description.procedure,
                    [window.get(), message, wparam, lparam],
                )
            }
            Continuation32::Retrieve { .. }
            | Continuation32::RemoteCall { .. }
            | Continuation32::MessageWait { .. } => {
                return Ok(User32Outcome32::Wait {
                    continuation: pending.token,
                });
            }
            Continuation32::Style {
                window,
                index,
                scratch,
                changed,
                ..
            } => (
                self.windows().get(window)?.description.procedure,
                [
                    window.get(),
                    if changed { 0x007d } else { 0x007c },
                    index as u32,
                    scratch.address.0,
                ],
            ),
        };
        Ok(User32Outcome32::Callback(User32Callback32 {
            count: 4,
            procedure,
            arguments,
            continuation: pending.token,
        }))
    }

    pub(crate) fn complete(
        &mut self,
        token: u64,
        result: u32,
        memory: &mut AddressSpace,
    ) -> Result<User32Outcome32, User32Error32> {
        self.check_menu_bar_scratch(memory)?;
        self.check_resource_dialog_scratch(memory)?;
        if let Some(outcome) = self.retry_menu_bar_completion(token) {
            return Ok(outcome);
        }
        if let Some(outcome) = self.retry_popup_completion(token) {
            return outcome;
        }
        if let Some(outcome) = self.retry_resource_dialog_completion(token, memory) {
            return outcome;
        }
        if let Some(outcome) = self.retry_clipboard_completion(token) {
            return Ok(outcome);
        }
        if let Some(outcome) = self.retry_ui_completion(token) {
            return Ok(outcome);
        }
        if let Some(outcome) = self.retry_system_colors_completion(token) {
            return Ok(outcome);
        }
        if let Some(outcome) = self.retry_sent_paint_completion(token) {
            return Ok(outcome);
        }
        if let Some(outcome) = self.retry_accelerator_completion(token) {
            return Ok(outcome);
        }
        if let Some(outcome) = self.retry_common_layout_completion(token) {
            return Ok(outcome);
        }
        if let Some(outcome) = self.retry_status_draw_completion(token) {
            return Ok(outcome);
        }
        if let Some(outcome) = self.retry_tab_selection_completion(token) {
            return Ok(outcome);
        }
        if let Some(outcome) = self.retry_tooltip_completion(token) {
            return Ok(outcome);
        }
        if let Some(outcome) = self.retry_header_completion(token) {
            return Ok(outcome);
        }
        let outcome = self.complete_step(token, result, memory)?;
        let outcome = self.complete_resource_dialog_child(token, outcome, memory)?;
        let outcome = self.complete_accelerator_child(token, outcome)?;
        let outcome = self.complete_clipboard_child(token, outcome)?;
        let outcome = self.complete_sent_paint_child(token, outcome)?;
        let outcome = self.complete_system_colors_child(token, outcome)?;
        let outcome = self.complete_ui_child(token, outcome)?;
        let outcome = self.complete_menu_bar_child(token, outcome)?;
        let outcome = self.complete_common_layout_child(token, outcome)?;
        let outcome = self.complete_tooltip_child(token, outcome)?;
        let outcome = self.complete_header_child(token, outcome)?;
        let outcome = self.complete_tab_selection_child(token, outcome)?;
        let outcome = self.complete_status_draw_child(token, outcome)?;
        self.complete_popup_child(token, outcome)
    }

    fn complete_step(
        &mut self,
        token: u64,
        result: u32,
        memory: &mut AddressSpace,
    ) -> Result<User32Outcome32, User32Error32> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)
            .cloned()
            .ok_or(User32Error32::InvalidToken)?;
        match pending.continuation {
            Continuation32::ResourceDialog { .. }
            | Continuation32::FilePrompt(_)
            | Continuation32::MenuPopup { .. }
            | Continuation32::MenuBar(_)
            | Continuation32::CommonLayout(_)
            | Continuation32::TooltipTrack(_)
            | Continuation32::Header(_)
            | Continuation32::TabSelection(_)
            | Continuation32::StatusDraw(_)
            | Continuation32::Accelerator(_)
            | Continuation32::Clipboard(_)
            | Continuation32::SystemColors(_)
            | Continuation32::UiState(_)
            | Continuation32::SentPaint(_) => Err(User32Error32::InvalidToken),
            Continuation32::ResourceProcedure {
                window,
                arguments,
                wide,
                ..
            } => self.complete_dialog_procedure(window, arguments, wide, result, memory),
            Continuation32::ControlDropdown(drop) => self.complete_control_dropdown(drop),
            Continuation32::ControlFocus(focus) => self.complete_control_focus(focus, memory),
            Continuation32::ControlClose(close) => self.complete_control_close(close),
            Continuation32::FileDialog(call) => {
                self.complete_file_dialog_hook(call, result, memory)
            }
            Continuation32::ControlPaint(_) | Continuation32::QueryPaint(_) => {
                Err(User32Error32::InvalidRequest)
            }
            Continuation32::Paint { window, output, dc } => {
                self.windows().get(window)?;
                output.complete(memory, result != 0)?;
                self.pending.pop();
                Ok(User32Outcome32::Return(dc))
            }
            Continuation32::Dialog { .. } | Continuation32::DialogRemoteHelp { .. } => {
                Err(User32Error32::InvalidToken)
            }
            Continuation32::DialogHelp { id, scratch } => {
                self.complete_dialog_help(id, scratch, memory)
            }
            Continuation32::Show(show) => self.complete_show(show),
            Continuation32::Destroy { plan } => self.complete_destroy(plan, memory),
            Continuation32::Timer { .. } => {
                self.pending.pop();
                Ok(User32Outcome32::Return(0))
            }
            Continuation32::Call { .. } => {
                self.pending.pop();
                Ok(User32Outcome32::Return(result))
            }
            Continuation32::Sequence {
                events,
                index,
                result: outer_result,
            } => {
                if events.get(index + 1).copied().flatten().is_some() {
                    self.pending.last_mut().unwrap().continuation = Continuation32::Sequence {
                        events,
                        index: index + 1,
                        result: outer_result,
                    };
                    self.next_outcome()
                } else {
                    self.pending.pop();
                    Ok(User32Outcome32::Return(outer_result))
                }
            }
            Continuation32::Create {
                window,
                thread,
                scratch,
                stage,
                visible,
            } => {
                scratch.check(memory)?;
                if self.windows().get(window).is_err() {
                    scratch.release(memory)?;
                    self.pending.pop();
                    return Ok(User32Outcome32::Return(0));
                }
                match stage {
                    CreateStage32::NonClientCreate if result == 0 => {
                        return self.fail_creation(window, thread, scratch, false);
                    }
                    CreateStage32::Create if result == u32::MAX => {
                        return self.fail_creation(window, thread, scratch, true);
                    }
                    CreateStage32::NonClientSize => {
                        let client = memory::rect(memory, scratch.address.0 + 48)?;
                        let extent = client.extent()?;
                        let mut record = self.windows().get(window)?;
                        let mut data = self.state.record(window)?;
                        data.client_origin = Point32 {
                            x: client.left,
                            y: client.top,
                        };
                        record.description.extent = extent;
                        self.state
                            .windows_mut()
                            .update_description(window, record.description)?;
                        self.state.update(data)?;
                    }
                    CreateStage32::Move if visible => {
                        let mut record = self.windows().get(window)?;
                        record.description.visible = true;
                        self.state
                            .windows_mut()
                            .update_description(window, record.description)?;
                    }
                    CreateStage32::Show => {
                        scratch.release(memory)?;
                        return self.activate_created_window(window, thread);
                    }
                    CreateStage32::Move => {
                        scratch.release(memory)?;
                        self.pending.pop();
                        return Ok(User32Outcome32::Return(window.get()));
                    }
                    _ => {}
                }
                let next = match stage {
                    CreateStage32::NonClientCreate => CreateStage32::NonClientSize,
                    CreateStage32::NonClientSize => CreateStage32::Create,
                    CreateStage32::Create => CreateStage32::Size,
                    CreateStage32::Size => CreateStage32::Move,
                    CreateStage32::Move => CreateStage32::Show,
                    CreateStage32::Show => unreachable!(),
                };
                self.pending.last_mut().unwrap().continuation = Continuation32::Create {
                    window,
                    thread,
                    scratch,
                    stage: next,
                    visible,
                };
                self.next_outcome()
            }
            Continuation32::Style {
                window,
                index,
                old,
                scratch,
                changed,
            } => {
                if self.windows().get(window).is_err() {
                    scratch.release(memory)?;
                    self.pending.pop();
                    return Ok(User32Outcome32::Return(0));
                }
                if changed {
                    scratch.release(memory)?;
                    self.pending.pop();
                    Ok(User32Outcome32::Return(old))
                } else {
                    scratch.check(memory)?;
                    let [_, new] = memory::words(memory, scratch.address.0)?;
                    self.apply_style(window, index, new)?;
                    self.pending.last_mut().unwrap().continuation = Continuation32::Style {
                        window,
                        index,
                        old,
                        scratch,
                        changed: true,
                    };
                    self.next_outcome()
                }
            }
            Continuation32::Position {
                window,
                scratch,
                stage,
            } => {
                scratch.check(memory)?;
                if self.windows().get(window).is_err() {
                    scratch.release(memory)?;
                    self.pending.pop();
                    return Ok(User32Outcome32::Return(0));
                }
                if matches!(stage, PositionStage32::Changed) {
                    scratch.release(memory)?;
                    self.pending.pop();
                    return Ok(User32Outcome32::Return(1));
                }
                if matches!(stage, PositionStage32::Changing) {
                    let [hwnd, _, x, y, width, height, flags] =
                        memory::words::<7>(memory, scratch.address.0)?;
                    if hwnd != window.get() {
                        return Err(User32Error32::InvalidRequest);
                    }
                    super::lifecycle::check_position_flags(flags)?;
                    let data = self.state.record(window)?;
                    let outer = self.position_rect(window, x, y, width, height, flags)?;
                    if flags & 0x0001 == 0 || flags & 0x0020 != 0 {
                        memory::write_rect(memory, scratch.address.0 + 28, outer)?;
                        self.pending.last_mut().unwrap().continuation = Continuation32::Position {
                            window,
                            scratch,
                            stage: PositionStage32::NonClientSize,
                        };
                        return self.next_outcome();
                    }
                    let record = self.windows().get(window)?;
                    let left = outer
                        .left
                        .checked_add(
                            data.client_origin
                                .x
                                .checked_sub(data.outer.left)
                                .ok_or(User32Error32::InvalidRequest)?,
                        )
                        .ok_or(User32Error32::InvalidRequest)?;
                    let top = outer
                        .top
                        .checked_add(
                            data.client_origin
                                .y
                                .checked_sub(data.outer.top)
                                .ok_or(User32Error32::InvalidRequest)?,
                        )
                        .ok_or(User32Error32::InvalidRequest)?;
                    let client = crate::windows::user32::Rect32 {
                        left,
                        top,
                        right: left
                            .checked_add(record.description.extent.width as i32)
                            .ok_or(User32Error32::InvalidRequest)?,
                        bottom: top
                            .checked_add(record.description.extent.height as i32)
                            .ok_or(User32Error32::InvalidRequest)?,
                    };
                    self.publish_position(window, outer, client, flags)?;
                } else {
                    let [_, _, x, y, width, height, flags] =
                        memory::words::<7>(memory, scratch.address.0)?;
                    super::lifecycle::check_position_flags(flags)?;
                    let outer = self.position_rect(window, x, y, width, height, flags)?;
                    let client = memory::rect(memory, scratch.address.0 + 28)?;
                    self.publish_position(window, outer, client, flags)?;
                }
                self.pending.last_mut().unwrap().continuation = Continuation32::Position {
                    window,
                    scratch,
                    stage: PositionStage32::Changed,
                };
                self.next_outcome()
            }
            Continuation32::Retrieve { .. }
            | Continuation32::RemoteCall { .. }
            | Continuation32::MessageWait { .. } => Err(User32Error32::InvalidToken),
        }
    }

    pub(super) fn sequence(
        &mut self,
        events: [Option<WindowMessage32>; 6],
        result: u32,
    ) -> Result<User32Outcome32, User32Error32> {
        if events[0].is_none() {
            return Ok(User32Outcome32::Return(result));
        }
        self.start(Continuation32::Sequence {
            events,
            index: 0,
            result,
        })
    }

    fn fail_creation(
        &mut self,
        window: WindowHandle32,
        thread: u32,
        scratch: Scratch32,
        created: bool,
    ) -> Result<User32Outcome32, User32Error32> {
        let outcome = self.begin_destroy(window, thread, 0, Some(scratch), created, true)?;
        self.last_error = Some(1407);
        Ok(outcome)
    }
}
