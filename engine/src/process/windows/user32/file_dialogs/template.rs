use crate::windows::user32::dialogs::{
    DialogItem32, DialogName32, DialogTemplate32, DialogUnits32,
};

pub(super) const FILES: u32 = 0x0460;
pub(super) const FILTER: u32 = 0x0470;
pub(super) const DIRECTORY: u32 = 0x0471;
pub(super) const NAME: u32 = 0x0480;
pub(super) const READ_ONLY: u32 = 0x0410;
pub(super) const HOOK: u32 = 0x04a0;

pub(super) fn template(
    title: Vec<u16>,
    save: bool,
    readonly: bool,
    hook: bool,
    multiple: bool,
) -> DialogTemplate32 {
    let mut items = vec![
        item(0x0443, 0x82, "Look in:", [8, 9, 42, 12], 0),
        item(DIRECTORY, 0x85, "", [54, 7, 198, 90], 0x0001_0003),
        item(
            FILES,
            0x83,
            "",
            [8, 27, 244, 90],
            0x00a1_0001 | if multiple { 0x800 } else { 0 },
        ),
        item(0x0442, 0x82, "File name:", [8, 127, 46, 12], 0),
        item(NAME, 0x81, "", [58, 125, 146, 14], 0x0081_0080),
        item(0x0441, 0x82, "Files of type:", [8, 147, 46, 12], 0),
        item(FILTER, 0x85, "", [58, 145, 146, 70], 0x0001_0003),
        item(
            1,
            0x80,
            if save { "Save" } else { "Open" },
            [212, 125, 40, 14],
            0x0001_0001,
        ),
        item(2, 0x80, "Cancel", [212, 145, 40, 14], 0x0001_0000),
    ];
    if readonly {
        items.push(item(
            READ_ONLY,
            0x80,
            "Read only",
            [8, 167, 120, 12],
            0x0001_0003,
        ));
    }
    if hook {
        let mut child = item(HOOK, 0, "", [0, 0, 0, 0], 0);
        child.class = DialogName32::Text("#32770".encode_utf16().collect());
        child.style &= !0x1000_0000;
        items.push(child);
    }
    DialogTemplate32 {
        extended: false,
        help: 0,
        style: 0x90c8_0080,
        ex_style: 1,
        units: DialogUnits32 {
            x: 10,
            y: 10,
            width: 260,
            height: 184,
        },
        menu: DialogName32::Empty,
        class: DialogName32::Empty,
        title,
        font: None,
        items,
        length: 0,
    }
}

fn item(id: u32, class: u16, title: &str, units: [i16; 4], style: u32) -> DialogItem32 {
    DialogItem32 {
        help: 0,
        style: style | 0x5000_0000,
        ex_style: 0,
        units: DialogUnits32 {
            x: units[0],
            y: units[1],
            width: units[2],
            height: units[3],
        },
        id,
        class: DialogName32::Ordinal(class),
        title: DialogName32::Text(title.encode_utf16().collect()),
        creation: Vec::new(),
    }
}
