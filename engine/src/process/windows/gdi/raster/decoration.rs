use super::canvas::GdiCanvas32;
use crate::{
    memory::{Access, AddressSpace, GuestAddress},
    process::windows::gdi::{GdiRuntime32, memory},
    windows::{
        gdi::{GdiApi32, GdiError32},
        user32::{Point32, Rect32},
    },
};

impl GdiRuntime32 {
    pub(in crate::process::windows::gdi) fn decoration_call(
        &mut self,
        ram: &mut AddressSpace,
        api: GdiApi32,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        let logical = memory::rect(ram, args[1])?;
        let state = self.model.state(args[0])?;
        let first = state.device_point(Point32 {
            x: logical.left,
            y: logical.top,
        })?;
        let last = state.device_point(Point32 {
            x: logical.right,
            y: logical.bottom,
        })?;
        let rect = Rect32 {
            left: first.x.min(last.x),
            top: first.y.min(last.y),
            right: first.x.max(last.x),
            bottom: first.y.max(last.y),
        };
        let adjust = args[3] & 0x2000 != 0;
        if adjust {
            ram.check_span(GuestAddress(args[1]), 16, Access::Write)?;
        }
        let mut canvas = GdiCanvas32::new(self, ram, args[0])?;
        let output = if api == GdiApi32::DrawEdge {
            canvas.edge(self, rect, args[2], args[3])?
        } else {
            canvas.frame_control(self, ram, rect, args[2], args[3])?
        };
        let mut bytes = [0; 16];
        let spans = if adjust {
            let state = self.model.state(args[0])?;
            let first = state.logical_point(Point32 {
                x: output.left,
                y: output.top,
            })?;
            let last = state.logical_point(Point32 {
                x: output.right,
                y: output.bottom,
            })?;
            for (part, word) in bytes
                .chunks_exact_mut(4)
                .zip([first.x, first.y, last.x, last.y])
            {
                part.copy_from_slice(&word.to_le_bytes());
            }
            vec![(GuestAddress(args[1]), bytes.as_slice())]
        } else {
            Vec::new()
        };
        self.publish_canvas_outputs(ram, canvas, &spans)?;
        Ok(1)
    }
}
