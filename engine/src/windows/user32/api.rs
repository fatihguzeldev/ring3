use crate::{
    loader::{SystemExport32, SystemExportTarget32, SystemModule32},
    windows::CallingConvention32,
};

macro_rules! api_catalog {
    ($( $variant:ident = $id:literal, $words:literal; )+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub enum User32Api32 { $( $variant = $id, )+ }

        impl User32Api32 {
            pub fn from_id(id: u32) -> Option<Self> { match id { $( $id => Some(Self::$variant), )+ _ => None } }
            pub fn name(self) -> &'static str { let name = match self { $( Self::$variant => stringify!($variant), )+ }; match name { "WsprintfA" => "wsprintfA", "WvsprintfA" => "wvsprintfA", _ => name } }
            pub fn stack_words(self) -> u32 { match self { $( Self::$variant => $words, )+ } }
            pub fn catalog() -> impl ExactSizeIterator<Item = Self> { [$( Self::$variant, )+].into_iter() }
        }
    };
}

api_catalog! {
    RegisterClassA = 0x0002_0001, 1;
    RegisterClassW = 0x0002_0002, 1;
    RegisterClassExA = 0x0002_0003, 1;
    RegisterClassExW = 0x0002_0004, 1;
    UnregisterClassA = 0x0002_0005, 2;
    UnregisterClassW = 0x0002_0006, 2;
    CreateWindowExA = 0x0002_0007, 12;
    CreateWindowExW = 0x0002_0008, 12;
    DestroyWindow = 0x0002_0009, 1;
    GetClientRect = 0x0002_000a, 2;
    GetWindowRect = 0x0002_000b, 2;
    IsWindow = 0x0002_000c, 1;
    GetFocus = 0x0002_000d, 0;
    SetFocus = 0x0002_000e, 1;
    GetCapture = 0x0002_000f, 0;
    SetCapture = 0x0002_0010, 1;
    ReleaseCapture = 0x0002_0011, 0;
    ShowWindow = 0x0002_0012, 2;
    MoveWindow = 0x0002_0013, 6;
    SetWindowPos = 0x0002_0014, 7;
    PeekMessageA = 0x0002_0015, 5;
    PeekMessageW = 0x0002_0016, 5;
    GetMessageA = 0x0002_0017, 4;
    GetMessageW = 0x0002_0018, 4;
    TranslateMessage = 0x0002_0019, 1;
    DispatchMessageA = 0x0002_001a, 1;
    DispatchMessageW = 0x0002_001b, 1;
    PostMessageA = 0x0002_001c, 4;
    PostMessageW = 0x0002_001d, 4;
    PostQuitMessage = 0x0002_001e, 1;
    DefWindowProcA = 0x0002_001f, 4;
    DefWindowProcW = 0x0002_0020, 4;
    GetWindowLongA = 0x0002_0021, 2;
    GetWindowLongW = 0x0002_0022, 2;
    SetWindowLongA = 0x0002_0023, 3;
    SetWindowLongW = 0x0002_0024, 3;
    CallWindowProcA = 0x0002_0025, 5;
    CallWindowProcW = 0x0002_0026, 5;
    SendMessageA = 0x0002_0027, 4;
    SendMessageW = 0x0002_0028, 4;
    MsgWaitForMultipleObjects = 0x0002_0029, 5;
    MsgWaitForMultipleObjectsEx = 0x0002_002a, 5;
    GetAsyncKeyState = 0x0002_002b, 1;
    GetKeyState = 0x0002_002c, 1;
    GetKeyboardState = 0x0002_002d, 1;
    SetKeyboardState = 0x0002_002e, 1;
    MapVirtualKeyA = 0x0002_002f, 2;
    MapVirtualKeyW = 0x0002_0030, 2;
    MapVirtualKeyExA = 0x0002_0031, 3;
    MapVirtualKeyExW = 0x0002_0032, 3;
    GetCursorPos = 0x0002_0033, 1;
    SetCursorPos = 0x0002_0034, 2;
    GetDoubleClickTime = 0x0002_0035, 0;
    SetDoubleClickTime = 0x0002_0036, 1;
    GetKeyboardLayout = 0x0002_0037, 1;
    GetKeyboardLayoutList = 0x0002_0038, 2;
    ActivateKeyboardLayout = 0x0002_0039, 2;
    LoadKeyboardLayoutA = 0x0002_003a, 2;
    LoadKeyboardLayoutW = 0x0002_003b, 2;
    UnloadKeyboardLayout = 0x0002_003c, 1;
    GetKeyboardType = 0x0002_003d, 1;
    GetKeyNameTextA = 0x0002_003e, 3;
    GetKeyNameTextW = 0x0002_003f, 3;
    ToAscii = 0x0002_0040, 5;
    ToAsciiEx = 0x0002_0041, 6;
    ToUnicode = 0x0002_0042, 6;
    ToUnicodeEx = 0x0002_0043, 7;
    VkKeyScanA = 0x0002_0044, 1;
    VkKeyScanW = 0x0002_0045, 1;
    VkKeyScanExA = 0x0002_0046, 2;
    VkKeyScanExW = 0x0002_0047, 2;
    ClipCursor = 0x0002_0048, 1;
    GetClipCursor = 0x0002_0049, 1;
    ShowCursor = 0x0002_004a, 1;
    SwapMouseButton = 0x0002_004b, 1;
    GetSystemMetrics = 0x0002_004c, 1;
    GetQueueStatus = 0x0002_004d, 1;
    GetInputState = 0x0002_004e, 0;
    ScreenToClient = 0x0002_004f, 2;
    ClientToScreen = 0x0002_0050, 2;
    GetMessagePos = 0x0002_0051, 0;
    GetMessageTime = 0x0002_0052, 0;
    GetMessageExtraInfo = 0x0002_0053, 0;
    SetMessageExtraInfo = 0x0002_0054, 1;
    SetCursor = 0x0002_0055, 1;
    GetCursor = 0x0002_0056, 0;
    GetForegroundWindow = 0x0002_0057, 0;
    GetActiveWindow = 0x0002_0058, 0;
    WaitMessage = 0x0002_0059, 0;
    SetTimer = 0x0002_005a, 4;
    KillTimer = 0x0002_005b, 2;
    MessageBoxA=0x0002_005c,4;
    MessageBoxW=0x0002_005d,4;
    MessageBoxIndirectA=0x0002_005e,1;
    MessageBoxIndirectW=0x0002_005f,1;
    GetWindowTextA=0x0002_0060,3;
    GetWindowTextW=0x0002_0061,3;
    GetWindowTextLengthA=0x0002_0062,1;
    GetWindowTextLengthW=0x0002_0063,1;
    SetWindowTextA=0x0002_0064,2;
    SetWindowTextW=0x0002_0065,2;
    CreateDialogParamA=0x0002_0066,5;
    CreateDialogParamW=0x0002_0067,5;
    CreateDialogIndirectParamA=0x0002_0068,5;
    CreateDialogIndirectParamW=0x0002_0069,5;
    DialogBoxParamA=0x0002_006a,5;
    DialogBoxParamW=0x0002_006b,5;
    DialogBoxIndirectParamA=0x0002_006c,5;
    DialogBoxIndirectParamW=0x0002_006d,5;
    EndDialog=0x0002_006e,2;
    DefDlgProcA=0x0002_006f,4;
    DefDlgProcW=0x0002_0070,4;
    GetDlgItem=0x0002_0071,2;
    GetDlgCtrlID=0x0002_0072,1;
    MapDialogRect=0x0002_0073,2;
    GetDialogBaseUnits=0x0002_0074,0;
    GetWindowContextHelpId=0x0002_0075,1;
    SetWindowContextHelpId=0x0002_0076,2;
    CreateMenu=0x0002_0077,0;
    CreatePopupMenu=0x0002_0078,0;
    DestroyMenu=0x0002_0079,1;
    IsMenu=0x0002_007a,1;
    GetMenu=0x0002_007b,1;
    SetMenu=0x0002_007c,2;
    GetMenuItemCount=0x0002_007d,1;
    GetMenuItemID=0x0002_007e,2;
    GetSubMenu=0x0002_007f,2;
    AppendMenuA=0x0002_0080,4;
    AppendMenuW=0x0002_0081,4;
    InsertMenuA=0x0002_0082,5;
    InsertMenuW=0x0002_0083,5;
    ModifyMenuA=0x0002_0084,5;
    ModifyMenuW=0x0002_0085,5;
    RemoveMenu=0x0002_0086,3;
    DeleteMenu=0x0002_0087,3;
    EnableMenuItem=0x0002_0088,3;
    CheckMenuItem=0x0002_0089,3;
    GetMenuState=0x0002_008b,3;
    GetMenuStringA=0x0002_008c,5;
    GetMenuStringW=0x0002_008d,5;
    GetMenuItemInfoA=0x0002_008e,4;
    GetMenuItemInfoW=0x0002_008f,4;
    SetMenuItemInfoA=0x0002_0090,4;
    SetMenuItemInfoW=0x0002_0091,4;
    InsertMenuItemA=0x0002_0092,4;
    InsertMenuItemW=0x0002_0093,4;
    LoadMenuA=0x0002_0094,2;
    LoadMenuW=0x0002_0095,2;
    LoadMenuIndirectA=0x0002_0096,1;
    LoadMenuIndirectW=0x0002_0097,1;
    GetMenuContextHelpId=0x0002_0098,1;
    SetMenuContextHelpId=0x0002_0099,2;
    TrackPopupMenu=0x0002_00a0,7;
    TrackPopupMenuEx=0x0002_00a1,6;
    EndMenu=0x0002_00a2,0;
    DrawMenuBar=0x0002_00a3,1;
    GetMenuItemRect=0x0002_00a4,4;
    CreateAcceleratorTableA=0x0002_00a6,2;
    CreateAcceleratorTableW=0x0002_00a7,2;
    CopyAcceleratorTableA=0x0002_00a8,3;
    CopyAcceleratorTableW=0x0002_00a9,3;
    DestroyAcceleratorTable=0x0002_00aa,1;
    LoadAcceleratorsA=0x0002_00ab,2;
    LoadAcceleratorsW=0x0002_00ac,2;
    TranslateAcceleratorA=0x0002_00ad,3;
    TranslateAcceleratorW=0x0002_00ae,3;
    OpenClipboard=0x0002_00b0,1;
    CloseClipboard=0x0002_00b1,0;
    EmptyClipboard=0x0002_00b2,0;
    GetClipboardData=0x0002_00b3,1;
    SetClipboardData=0x0002_00b4,2;
    IsClipboardFormatAvailable=0x0002_00b5,1;
    CountClipboardFormats=0x0002_00b6,0;
    EnumClipboardFormats=0x0002_00b7,1;
    RegisterClipboardFormatA=0x0002_00b8,1;
    RegisterClipboardFormatW=0x0002_00b9,1;
    GetClipboardFormatNameA=0x0002_00ba,3;
    GetClipboardFormatNameW=0x0002_00bb,3;
    GetClipboardOwner=0x0002_00bc,0;
    GetOpenClipboardWindow=0x0002_00bd,0;
    GetClipboardSequenceNumber=0x0002_00be,0;
    GetPriorityClipboardFormat=0x0002_00bf,2;
    LoadBitmapA=0x0002_00c0,2;
    LoadBitmapW=0x0002_00c1,2;
    LoadImageA=0x0002_00c2,6;
    LoadImageW=0x0002_00c3,6;
    CopyImage=0x0002_00c4,5;
    DestroyIcon=0x0002_00c5,1;
    DestroyCursor=0x0002_00c6,1;
    DrawIcon=0x0002_00c7,4;
    DrawIconEx=0x0002_00c8,9;
    LoadIconA=0x0002_00c9,2;
    LoadIconW=0x0002_00ca,2;
    LoadCursorA=0x0002_00cb,2;
    LoadCursorW=0x0002_00cc,2;
    SetSysColors=0x0002_00cd,3;
    RegisterWindowMessageA=0x0002_00ce,1;
    RegisterWindowMessageW=0x0002_00cf,1;
    AdjustWindowRectEx=0x0002_00d0,4;
    BeginDeferWindowPos=0x0002_00d1,1;
    BringWindowToTop=0x0002_00d2,1;
    CallNextHookEx=0x0002_00d3,4;
    CharNextA=0x0002_00d4,1;
    CharToOemA=0x0002_00d5,2;
    CharUpperA=0x0002_00d6,1;
    CheckDlgButton=0x0002_00d7,3;
    CheckRadioButton=0x0002_00d8,4;
    CopyRect=0x0002_00d9,2;
    DefFrameProcA=0x0002_00da,5;
    DefMDIChildProcA=0x0002_00db,4;
    DeferWindowPos=0x0002_00dc,8;
    DrawFocusRect=0x0002_00dd,2;
    EnableWindow=0x0002_00de,2;
    EndDeferWindowPos=0x0002_00df,1;
    EnumChildWindows=0x0002_00e0,3;
    EqualRect=0x0002_00e1,2;
    FindWindowA=0x0002_00e2,2;
    GetClassInfoA=0x0002_00e3,3;
    GetClassLongA=0x0002_00e4,2;
    GetClassNameA=0x0002_00e5,3;
    GetDesktopWindow=0x0002_00e6,0;
    GetDlgItemInt=0x0002_00e7,4;
    GetDlgItemTextA=0x0002_00e8,4;
    GetLastActivePopup=0x0002_00e9,1;
    GetMenuCheckMarkDimensions=0x0002_00ea,0;
    GetNextDlgGroupItem=0x0002_00eb,3;
    GetNextDlgTabItem=0x0002_00ec,3;
    GetParent=0x0002_00ed,1;
    GetPropA=0x0002_00ee,2;
    GetScrollInfo=0x0002_00ef,3;
    GetScrollPos=0x0002_00f0,2;
    GetScrollRange=0x0002_00f1,4;
    GetSystemMenu=0x0002_00f2,2;
    GetTabbedTextExtentA=0x0002_00f3,5;
    GetTopWindow=0x0002_00f4,1;
    GetWindow=0x0002_00f5,2;
    GetWindowPlacement=0x0002_00f6,2;
    GetWindowThreadProcessId=0x0002_00f7,2;
    GrayStringA=0x0002_00f8,9;
    InSendMessage=0x0002_00f9,0;
    InflateRect=0x0002_00fa,3;
    IntersectRect=0x0002_00fb,3;
    InvalidateRgn=0x0002_00fc,3;
    IsChild=0x0002_00fd,2;
    IsDialogMessageA=0x0002_00fe,2;
    IsDlgButtonChecked=0x0002_00ff,2;
    IsIconic=0x0002_0100,1;
    IsRectEmpty=0x0002_0101,1;
    IsWindowEnabled=0x0002_0102,1;
    IsWindowVisible=0x0002_0103,1;
    IsZoomed=0x0002_0104,1;
    LockWindowUpdate=0x0002_0105,1;
    MapWindowPoints=0x0002_0106,4;
    MessageBeep=0x0002_0107,1;
    OemToCharA=0x0002_0108,2;
    OffsetRect=0x0002_0109,3;
    PostThreadMessageA=0x0002_010a,4;
    PtInRect=0x0002_010b,3;
    RedrawWindow=0x0002_010c,4;
    RemovePropA=0x0002_010d,2;
    ReuseDDElParam=0x0002_010e,5;
    ScrollWindow=0x0002_010f,5;
    ScrollWindowEx=0x0002_0110,8;
    SendDlgItemMessageA=0x0002_0111,5;
    SetActiveWindow=0x0002_0112,1;
    SetDlgItemInt=0x0002_0113,4;
    SetDlgItemTextA=0x0002_0114,3;
    SetForegroundWindow=0x0002_0115,1;
    SetMenuItemBitmaps=0x0002_0116,5;
    SetParent=0x0002_0117,2;
    SetPropA=0x0002_0118,3;
    SetRect=0x0002_0119,5;
    SetRectEmpty=0x0002_011a,1;
    SetScrollInfo=0x0002_011b,4;
    SetScrollPos=0x0002_011c,4;
    SetScrollRange=0x0002_011d,5;
    SetWindowPlacement=0x0002_011e,2;
    SetWindowsHookExA=0x0002_011f,4;
    ShowOwnedPopups=0x0002_0120,2;
    ShowScrollBar=0x0002_0121,3;
    SystemParametersInfoA=0x0002_0122,4;
    TabbedTextOutA=0x0002_0123,8;
    TranslateMDISysAccel=0x0002_0124,2;
    UnhookWindowsHookEx=0x0002_0125,1;
    UnionRect=0x0002_0126,3;
    UnpackDDElParam=0x0002_0127,4;
    WinHelpA=0x0002_0128,4;
    WindowFromPoint=0x0002_0129,2;
    WsprintfA=0x0002_012a,2;
    WvsprintfA=0x0002_012b,3;
}

impl User32Api32 {
    pub(crate) fn image(self) -> bool {
        (0x0002_00c0..=0x0002_00cc).contains(&self.id())
    }
    pub(crate) fn clipboard(self) -> bool {
        (0x0002_00b0..=0x0002_00bf).contains(&self.id())
    }
    pub(crate) fn dialog_template(self) -> bool {
        matches!(
            self,
            Self::CreateDialogParamA
                | Self::CreateDialogParamW
                | Self::CreateDialogIndirectParamA
                | Self::CreateDialogIndirectParamW
                | Self::DialogBoxParamA
                | Self::DialogBoxParamW
                | Self::DialogBoxIndirectParamA
                | Self::DialogBoxIndirectParamW
        )
    }
    pub(crate) fn dialog_modal(self) -> bool {
        matches!(
            self,
            Self::DialogBoxParamA
                | Self::DialogBoxParamW
                | Self::DialogBoxIndirectParamA
                | Self::DialogBoxIndirectParamW
        )
    }
    pub(crate) fn dialog_indirect(self) -> bool {
        matches!(
            self,
            Self::CreateDialogIndirectParamA
                | Self::CreateDialogIndirectParamW
                | Self::DialogBoxIndirectParamA
                | Self::DialogBoxIndirectParamW
        )
    }
    pub(crate) fn failure_value(self) -> u32 {
        if self.dialog_modal()
            || matches!(
                self,
                Self::GetMessageA
                    | Self::GetMessageW
                    | Self::MsgWaitForMultipleObjects
                    | Self::MsgWaitForMultipleObjectsEx
                    | Self::GetMenuItemCount
                    | Self::GetMenuItemID
                    | Self::EnableMenuItem
                    | Self::CheckMenuItem
                    | Self::GetMenuState
            )
        {
            u32::MAX
        } else {
            0
        }
    }
    pub(crate) fn sent_message(self, args: &[u32]) -> Option<[u32; 4]> {
        use User32Api32::*;
        Some(match self {
            SendMessageA | SendMessageW => [args[0], args[1], args[2], args[3]],
            GetWindowTextA | GetWindowTextW => [args[0], 0x0d, args[2], args[1]],
            GetWindowTextLengthA | GetWindowTextLengthW => [args[0], 0x0e, 0, 0],
            SetWindowTextA | SetWindowTextW => [args[0], 0x0c, 0, args[1]],
            _ => return None,
        })
    }
    pub fn id(self) -> u32 {
        self as u32
    }
    pub fn convention(self) -> CallingConvention32 {
        if self == Self::WsprintfA {
            CallingConvention32::Cdecl
        } else {
            CallingConvention32::Stdcall
        }
    }
    pub fn resolve(module: &str, symbol: &str) -> Option<Self> {
        if !module.eq_ignore_ascii_case("user32.dll") {
            return None;
        }
        Self::catalog().find(|api| api.name() == symbol)
    }
    pub fn wide(self) -> bool {
        self.name().ends_with('W')
    }
}

pub fn user32_module(preferred_base: u32) -> SystemModule32 {
    SystemModule32 {
        name: "user32.dll".into(),
        preferred_base,
        exports: User32Api32::catalog()
            .map(|api| SystemExport32 {
                name: Some(api.name().into()),
                ordinal: None,
                target: SystemExportTarget32::Gate(api.id()),
            })
            .chain(crate::windows::resources::user32_resource_exports())
            .chain(crate::windows::rawinput::raw_input_exports())
            .chain(crate::windows::gdi::user32_gdi_exports())
            .chain(crate::windows::controls::builtin_exports())
            .collect(),
    }
}
