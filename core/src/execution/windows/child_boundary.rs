use super::{Api, creation, desktop};

impl Api {
    pub(super) fn requires_primary(self) -> bool {
        if let Self::Window(call) = self {
            return !matches!(
                call,
                creation::Call::Rectangle
                    | creation::Call::ClientRectangle
                    | creation::Call::Parent
                    | creation::Call::GetLong
            );
        }
        if let Self::Desktop(call) = self {
            return !matches!(
                call,
                desktop::Call::Desktop
                    | desktop::Call::Find
                    | desktop::Call::IsWindow
                    | desktop::Call::DlgItem
                    | desktop::Call::Top
                    | desktop::Call::Window
            );
        }
        matches!(
            self,
            Self::SendMessage
                | Self::PostMessage
                | Self::PeekMessage
                | Self::GetMessage
                | Self::TranslateMessage
                | Self::DispatchMessage
                | Self::SetWindowText
                | Self::EnableWindow
                | Self::EndDialog
                | Self::SetWindowPos
                | Self::DestroyWindow
                | Self::ShowWindow
                | Self::UpdateWindow
                | Self::InvalidateRect
                | Self::SetForegroundWindow
                | Self::CreateDialog
                | Self::CallNextHook
                | Self::Cursor(_)
                | Self::Gdi(_)
                | Self::Graphics(_)
                | Self::Com(_)
                | Self::Input(_)
        )
    }
}
