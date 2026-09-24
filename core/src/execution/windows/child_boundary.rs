use super::Api;

impl Api {
    pub(super) fn requires_primary(self) -> bool {
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
                | Self::Window(_)
                | Self::Desktop(_)
                | Self::Cursor(_)
                | Self::Gdi(_)
                | Self::Graphics(_)
                | Self::Com(_)
        )
    }
}
