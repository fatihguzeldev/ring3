mod delivery;
mod input;
mod painting;
use super::super::{
    User32CallContext32, User32Context32, User32Outcome32,
    callback::Continuation32,
    memory::{self, Scratch32},
};
use crate::{
    memory::{Access, AddressSpace, GuestAddress, PAGE_SIZE, PageRange},
    process::{HostError, windows::gdi::GdiRuntime32},
    windows::{
        comctl32::CommonControlState32 as State,
        gdi::Region32,
        user32::{Point32, Rect32, User32Api32, User32Error32 as E},
        window::WindowHandle32,
    },
};
#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) enum NumericAction32 {
    Track {
        position: i32,
        code: u32,
        key: bool,
    },
    Spin {
        position: i32,
        delta: Option<i32>,
        range: Option<(i32, i32)>,
        base: Option<u32>,
    },
    Read {
        legacy: bool,
    },
    Bind {
        buddy: Option<WindowHandle32>,
    },
    End,
    Cancel,
}
#[derive(Clone, Copy)]
enum Arm32 {
    Thumb,
    Page(Point32),
    Spin(i32),
}
#[derive(Clone, Copy)]
enum Stage32 {
    Focus,
    Capture,
    Arm,
    ReadBuddy,
    Parsed,
    Notify,
    Apply,
    Buddy,
    Scroll,
    EndScroll,
    Release,
    LayoutBuddy,
    LayoutControl,
    SelectBuddy,
    Finish,
}
#[derive(Clone, Copy)]
enum Child32 {
    Focus,
    Capture,
    ReadBuddy,
    Notify,
    Buddy,
    Scroll,
    EndScroll,
    Release,
    LayoutBuddy,
    LayoutControl,
    SelectBuddy,
}
#[derive(Clone, Copy)]
struct PositionOutput32 {
    address: GuestAddress,
    identity: u64,
    first: u32,
    mappings: [u64; 2],
    count: u32,
}
impl PositionOutput32 {
    fn capture(ram: &mut AddressSpace, address: u32) -> Result<Self, E> {
        let address = GuestAddress(address);
        ram.check_span(address, 4, Access::Write)?;
        let first = address.0 / PAGE_SIZE;
        let last = ((u64::from(address.0) + 3) / u64::from(PAGE_SIZE)) as u32;
        ram.observe_mapping_incarnations(PageRange::new(
            GuestAddress(first * PAGE_SIZE),
            last - first + 1,
        )?)?;
        let mut mappings = [0; 2];
        for page in first..=last {
            mappings[(page - first) as usize] =
                ram.mapping_incarnation_at(GuestAddress(page * PAGE_SIZE))?;
        }
        Ok(Self {
            address,
            identity: ram.identity(),
            first,
            mappings,
            count: last - first + 1,
        })
    }
    fn check(self, ram: &AddressSpace) -> Result<(), E> {
        if ram.identity() != self.identity {
            return Err(E::InvalidRequest);
        }
        for page in 0..self.count {
            if ram.mapping_incarnation_at(GuestAddress((self.first + page) * PAGE_SIZE))?
                != self.mappings[page as usize]
            {
                return Err(E::InvalidRequest);
            }
        }
        ram.check_span(self.address, 4, Access::Write)?;
        Ok(())
    }
}
#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct NumericCall32 {
    window: WindowHandle32,
    parent: Option<WindowHandle32>,
    call: User32CallContext32,
    scratch: Scratch32,
    action: NumericAction32,
    stage: Stage32,
    result: u32,
    start: i32,
    error: bool,
    arm: Option<Arm32>,
    focus: Option<WindowHandle32>,
    release: bool,
    output: Option<PositionOutput32>,
    buddy_wide: bool,
    buddy_text: bool,
    layout: Option<(Rect32, Rect32)>,
    child: Option<(u64, Child32)>,
    completed: Option<u64>,
}
impl NumericCall32 {
    pub(in crate::process::windows::user32) fn scratch(self) -> Scratch32 {
        self.scratch
    }
    pub(in crate::process::windows::user32) fn check_output(
        self,
        ram: &AddressSpace,
    ) -> Result<(), E> {
        if let Some(output) = self.output {
            output.check(ram)?;
        }
        Ok(())
    }
}
impl User32Context32 {
    pub(super) fn numeric_changed(&mut self, window: WindowHandle32) {
        let extent = self
            .windows()
            .get(window)
            .expect("same numeric control owner")
            .description
            .extent;
        self.state
            .set_update_region(
                window,
                Region32::rectangle(Rect32 {
                    left: 0,
                    top: 0,
                    right: extent.width as i32,
                    bottom: extent.height as i32,
                }),
                true,
            )
            .expect("numeric paint was preflighted");
    }
    fn start_numeric(
        &mut self,
        window: WindowHandle32,
        action: NumericAction32,
        result: u32,
        output: u32,
        arm: Option<Arm32>,
        release: bool,
        focus: Option<WindowHandle32>,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        self.prepare_thread_slots()?;
        if self.pending.len() + 3 > self.continuation_limit {
            return Err(E::Capacity);
        }
        self.next_token.checked_add(3).ok_or(E::Capacity)?;
        let output = if output != 0 {
            Some(PositionOutput32::capture(ram, output)?)
        } else {
            None
        };
        let parent = self.state.record(window)?.parent;
        let start = match self.state.common_state(window)? {
            State::Trackbar(value) => value.position,
            State::UpDown(value) => value.position,
            _ => return Err(E::InvalidRequest),
        };
        let scratch = Scratch32::allocate(ram, self.scratch_bounds, &[0; 256])?;
        let stage = if focus.is_some() {
            Stage32::Focus
        } else if arm.is_some() {
            Stage32::Capture
        } else {
            Stage32::Arm
        };
        self.start(Continuation32::Numeric(NumericCall32 {
            window,
            parent,
            call,
            scratch,
            action,
            stage,
            result,
            start,
            error: false,
            arm,
            focus,
            release,
            output,
            buddy_wide: true,
            buddy_text: false,
            layout: None,
            child: None,
            completed: None,
        }))
    }
    pub(super) fn begin_trackbar_change(
        &mut self,
        window: WindowHandle32,
        position: i32,
        code: u32,
        capture: bool,
        release: bool,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        self.start_numeric(
            window,
            NumericAction32::Track {
                position,
                code,
                key: false,
            },
            0,
            0,
            capture.then_some(Arm32::Thumb),
            release,
            None,
            ram,
            call,
        )
    }
    pub(super) fn begin_updown_set(
        &mut self,
        window: WindowHandle32,
        position: i32,
        result: u32,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        self.start_numeric(
            window,
            NumericAction32::Spin {
                position,
                delta: None,
                range: None,
                base: None,
            },
            result,
            0,
            None,
            false,
            None,
            ram,
            call,
        )
    }
    pub(super) fn begin_updown_range(
        &mut self,
        window: WindowHandle32,
        minimum: i32,
        maximum: i32,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        let position = self.updown(window)?.position;
        self.start_numeric(
            window,
            NumericAction32::Spin {
                position,
                delta: None,
                range: Some((minimum, maximum)),
                base: None,
            },
            0,
            0,
            None,
            false,
            None,
            ram,
            call,
        )
    }
    pub(super) fn begin_updown_base(
        &mut self,
        window: WindowHandle32,
        base: u32,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        let value = self.updown(window)?;
        self.start_numeric(
            window,
            NumericAction32::Spin {
                position: value.position,
                delta: None,
                range: None,
                base: Some(base),
            },
            value.base,
            0,
            None,
            false,
            None,
            ram,
            call,
        )
    }
    pub(super) fn begin_updown_read(
        &mut self,
        window: WindowHandle32,
        output: u32,
        legacy: bool,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        self.start_numeric(
            window,
            NumericAction32::Read { legacy },
            0,
            output,
            None,
            false,
            None,
            ram,
            call,
        )
    }
    pub(super) fn bind_updown_buddy(
        &mut self,
        window: WindowHandle32,
        buddy: Option<WindowHandle32>,
        result: u32,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        if self.updown_buddy(window)? == buddy {
            return Ok(User32Outcome32::Return(result));
        }
        if buddy == Some(window) {
            return Err(E::Win32(87));
        }
        if let Some(buddy) = buddy {
            self.owned(buddy.get(), call.thread_id)?;
        }
        self.start_numeric(
            window,
            NumericAction32::Bind { buddy },
            result,
            0,
            None,
            false,
            None,
            ram,
            call,
        )
    }
    fn numeric_call(&self, token: u64) -> Result<NumericCall32, E> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)
            .ok_or(E::InvalidToken)?;
        let Continuation32::Numeric(call) = pending.continuation else {
            return Err(E::InvalidToken);
        };
        Ok(call)
    }
    fn publish_numeric(&mut self, token: u64, call: NumericCall32) {
        self.pending
            .iter_mut()
            .find(|pending| pending.token == token)
            .expect("same numeric continuation owner")
            .continuation = Continuation32::Numeric(call);
    }
    pub(in crate::process::windows::user32) fn numeric_wait(&self, token: u64) -> bool {
        self.pending.last().is_some_and(|pending| {
            pending.token == token && matches!(pending.continuation, Continuation32::Numeric(_))
        })
    }
    pub(in crate::process::windows::user32) fn preview_numeric_input_flags(
        &self,
        token: u64,
    ) -> Result<Option<(bool, bool)>, E> {
        if !self.numeric_wait(token) {
            return Ok(None);
        }
        let value = self.numeric_call(token)?;
        if self.windows().get(value.window).is_err() {
            return Ok(None);
        }
        match value.stage {
            Stage32::Focus => {
                let focus = value
                    .focus
                    .filter(|&focus| self.windows().get(focus).is_ok());
                self.windows()
                    .check_set_focus(value.call.thread_id, focus)?;
                Ok(Some((focus.is_some(), self.windows().capture().is_some())))
            }
            Stage32::Capture => {
                self.windows()
                    .check_set_capture(value.call.thread_id, Some(value.window))?;
                Ok(Some((self.windows().focus().is_some(), true)))
            }
            Stage32::Release if self.windows().capture() == Some(value.window) => {
                self.windows()
                    .check_set_capture(value.call.thread_id, None)?;
                Ok(Some((self.windows().focus().is_some(), false)))
            }
            _ => Ok(None),
        }
    }
}
fn child_token(outcome: User32Outcome32) -> Option<u64> {
    match outcome {
        User32Outcome32::Callback(callback) => Some(callback.continuation),
        User32Outcome32::Wait { continuation } => Some(continuation),
        _ => None,
    }
}
