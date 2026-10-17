use crate::{
    memory::{Access, AddressSpace, GuestAddress, PAGE_SIZE, PageRange},
    process::windows::user32::{User32CallContext32, memory::Scratch32},
    windows::{
        user32::{Rect32, User32Error32 as E},
        window::WindowHandle32,
    },
};

#[derive(Clone, Copy)]
pub(super) struct RectOutput32 {
    address: GuestAddress,
    identity: u64,
    first: u32,
    count: u32,
    mappings: [u64; 2],
}

impl RectOutput32 {
    pub(super) fn new(ram: &mut AddressSpace, raw: u32) -> Result<Self, E> {
        let address = GuestAddress(raw);
        ram.check_span(address, 16, Access::Write)?;
        let first = raw / PAGE_SIZE;
        let last = ((u64::from(raw) + 15) / u64::from(PAGE_SIZE)) as u32;
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
            count: last - first + 1,
            mappings,
        })
    }

    pub(super) fn address(self) -> GuestAddress {
        self.address
    }

    pub(super) fn check(self, ram: &AddressSpace) -> Result<(), E> {
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
        ram.check_span(self.address, 16, Access::Write)?;
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage32 {
    Changing,
    PrepareNonClient,
    NonClient,
    Publish,
    NonClientPaint,
    Changed,
    Measure(usize),
    MeasureCall(usize),
    MeasureRead(usize),
    Layout,
    Paint,
    Draw(usize),
    DrawCall(usize),
    DrawFinish(usize),
    Finish,
}

#[derive(Clone, Copy)]
pub(super) enum Purpose32 {
    Frame,
    NonClient { output: RectOutput32, base: Rect32 },
    Paint,
}

#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct MenuBar32 {
    pub(super) window: WindowHandle32,
    pub(super) menu: u32,
    pub(super) call: User32CallContext32,
    pub(super) scratch: Scratch32,
    pub(super) purpose: Purpose32,
    pub(super) stage: Stage32,
    pub(super) child: Option<u64>,
    pub(super) completed: Option<u64>,
    pub(super) measure_revision: u64,
    pub(super) measure_padding: i32,
    pub(super) dc: Option<u32>,
}

impl MenuBar32 {
    pub(in crate::process::windows::user32) fn scratch(self) -> Scratch32 {
        self.scratch
    }
    pub(in crate::process::windows::user32) fn check(self, ram: &AddressSpace) -> Result<(), E> {
        self.scratch.check(ram)?;
        if let Purpose32::NonClient { output, .. } = self.purpose {
            output.check(ram)?;
        }
        Ok(())
    }
}
