use crate::windows::CallingConvention32;

macro_rules! file_apis {
    ($( $name:ident = $id:literal, $words:literal; )+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub enum FileApi32 { $( $name = 0x000a_0000 | $id, )+ }
        impl FileApi32 {
            pub const ALL: &'static [Self] = &[ $( Self::$name, )+ ];
            pub const fn id(self) -> u32 { self as u32 }
            pub fn from_id(id: u32) -> Option<Self> {
                match id { $( value if value == Self::$name.id() => Some(Self::$name), )+ _ => None }
            }
            pub const fn name(self) -> &'static str {
                match self { $( Self::$name => stringify!($name), )+ }
            }
            pub const fn stack_words(self) -> u32 {
                match self { $( Self::$name => $words, )+ }
            }
            pub const fn convention(self) -> CallingConvention32 { CallingConvention32::Stdcall }
        }
    }
}

file_apis! {
    CreateFileA = 0x01, 7;
    CreateFileW = 0x02, 7;
    ReadFile = 0x03, 5;
    WriteFile = 0x04, 5;
    SetFilePointer = 0x05, 4;
    SetFilePointerEx = 0x06, 5;
    GetFileSize = 0x07, 2;
    GetFileSizeEx = 0x08, 2;
    SetEndOfFile = 0x09, 1;
    FlushFileBuffers = 0x0a, 1;
    GetFileType = 0x0b, 1;
    GetFileInformationByHandle = 0x0c, 2;
    GetFileTime = 0x0d, 4;
    SetFileTime = 0x0e, 4;
    GetFileAttributesA = 0x0f, 1;
    GetFileAttributesW = 0x10, 1;
    GetFileAttributesExA = 0x11, 3;
    GetFileAttributesExW = 0x12, 3;
    SetFileAttributesA = 0x13, 2;
    SetFileAttributesW = 0x14, 2;
    CreateDirectoryA = 0x15, 2;
    CreateDirectoryW = 0x16, 2;
    CreateDirectoryExA = 0x17, 3;
    CreateDirectoryExW = 0x18, 3;
    RemoveDirectoryA = 0x19, 1;
    RemoveDirectoryW = 0x1a, 1;
    FindFirstFileA = 0x1b, 2;
    FindFirstFileW = 0x1c, 2;
    FindFirstFileExA = 0x1d, 6;
    FindFirstFileExW = 0x1e, 6;
    FindNextFileA = 0x1f, 2;
    FindNextFileW = 0x20, 2;
    FindClose = 0x21, 1;
    DeleteFileA = 0x22, 1;
    DeleteFileW = 0x23, 1;
    CopyFileA = 0x24, 3;
    CopyFileW = 0x25, 3;
    CopyFileExA = 0x26, 6;
    CopyFileExW = 0x27, 6;
    MoveFileA = 0x28, 2;
    MoveFileW = 0x29, 2;
    MoveFileExA = 0x2a, 3;
    MoveFileExW = 0x2b, 3;
    MoveFileWithProgressA = 0x2c, 5;
    MoveFileWithProgressW = 0x2d, 5;
    ReplaceFileA = 0x2e, 6;
    ReplaceFileW = 0x2f, 6;
    GetCurrentDirectoryA = 0x30, 2;
    GetCurrentDirectoryW = 0x31, 2;
    SetCurrentDirectoryA = 0x32, 1;
    SetCurrentDirectoryW = 0x33, 1;
    GetFullPathNameA = 0x34, 4;
    GetFullPathNameW = 0x35, 4;
    GetLongPathNameA = 0x36, 3;
    GetLongPathNameW = 0x37, 3;
    GetShortPathNameA = 0x38, 3;
    GetShortPathNameW = 0x39, 3;
    GetTempPathA = 0x3a, 2;
    GetTempPathW = 0x3b, 2;
    GetTempFileNameA = 0x3c, 4;
    GetTempFileNameW = 0x3d, 4;
    GetDriveTypeA = 0x3e, 1;
    GetDriveTypeW = 0x3f, 1;
    GetLogicalDrives = 0x40, 0;
    GetLogicalDriveStringsA = 0x41, 2;
    GetLogicalDriveStringsW = 0x42, 2;
    GetDiskFreeSpaceA = 0x43, 5;
    GetDiskFreeSpaceW = 0x44, 5;
    GetDiskFreeSpaceExA = 0x45, 4;
    GetDiskFreeSpaceExW = 0x46, 4;
    GetVolumeInformationA = 0x47, 8;
    GetVolumeInformationW = 0x48, 8;
    GetErrorMode = 0x49, 0;
    SetErrorMode = 0x4a, 1;
    GetThreadErrorMode = 0x4b, 0;
    SetThreadErrorMode = 0x4c, 2;
    GetFileInformationByHandleEx = 0x4d, 4;
    GetFinalPathNameByHandleA = 0x4e, 4;
    GetFinalPathNameByHandleW = 0x4f, 4;
    SearchPathA = 0x50, 6;
    SearchPathW = 0x51, 6;
    LockFile = 0x52, 5;
    UnlockFile = 0x53, 5;
    LockFileEx = 0x54, 6;
    UnlockFileEx = 0x55, 5;
    CreateFileMappingA = 0x80, 6;
    CreateFileMappingW = 0x81, 6;
    OpenFileMappingA = 0x82, 3;
    OpenFileMappingW = 0x83, 3;
    MapViewOfFile = 0x84, 5;
    MapViewOfFileEx = 0x85, 6;
    UnmapViewOfFile = 0x86, 1;
    FlushViewOfFile = 0x87, 2;
    GetOverlappedResult = 0x88, 4;
    GetOverlappedResultEx = 0x89, 5;
    CancelIo = 0x8a, 1;
    CancelIoEx = 0x8b, 2;
    CreateIoCompletionPort = 0x90, 4;
    PostQueuedCompletionStatus = 0x91, 4;
    GetQueuedCompletionStatus = 0x92, 5;
    GetQueuedCompletionStatusEx = 0x93, 6;
    SetFileCompletionNotificationModes = 0x94, 2;
    CreatePipe = 0xa0, 4;
    CreateNamedPipeA = 0xa1, 8;
    CreateNamedPipeW = 0xa2, 8;
    ConnectNamedPipe = 0xa3, 2;
    DisconnectNamedPipe = 0xa4, 1;
    PeekNamedPipe = 0xa5, 6;
    GetNamedPipeInfo = 0xa6, 5;
    SetNamedPipeHandleState = 0xa7, 4;
    GetNamedPipeHandleStateA = 0xa8, 7;
    GetNamedPipeHandleStateW = 0xa9, 7;
    WaitNamedPipeA = 0xaa, 2;
    WaitNamedPipeW = 0xab, 2;
    TransactNamedPipe = 0xac, 7;
    CallNamedPipeA = 0xad, 7;
    CallNamedPipeW = 0xae, 7;
    GetWindowsDirectoryA = 0xaf, 2;
    GetWindowsDirectoryW = 0xb0, 2;
    GetSystemDirectoryA = 0xb1, 2;
    GetSystemDirectoryW = 0xb2, 2;
    SetFileInformationByHandle = 0xb3, 4;
}

impl FileApi32 {
    pub const fn failure_value(self) -> u32 {
        match self {
            Self::CreateFileA
            | Self::CreateFileW
            | Self::CreateNamedPipeA
            | Self::CreateNamedPipeW
            | Self::FindFirstFileA
            | Self::FindFirstFileW
            | Self::FindFirstFileExA
            | Self::FindFirstFileExW
            | Self::GetFileAttributesA
            | Self::GetFileAttributesW
            | Self::GetFileSize
            | Self::SetFilePointer => u32::MAX,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_ia32_stack_sizes_and_reserved_gate_domains_are_exact() {
        assert_eq!(FileApi32::CreateFileW.id(), 0x000a_0002);
        assert_eq!(FileApi32::SetFilePointerEx.stack_words(), 5);
        assert_eq!(FileApi32::GetDiskFreeSpaceA.stack_words(), 5);
        assert_eq!(FileApi32::CreateFileMappingW.id(), 0x000a_0081);
        assert_eq!(FileApi32::MapViewOfFileEx.stack_words(), 6);
        for (index, api) in FileApi32::ALL.iter().enumerate() {
            assert_eq!(FileApi32::from_id(api.id()), Some(*api));
            assert!(
                !FileApi32::ALL[..index]
                    .iter()
                    .any(|prior| prior.id() == api.id() || prior.name() == api.name())
            );
        }
        assert_eq!(FileApi32::from_id(0x000a_007f), None);
        assert_eq!(FileApi32::from_id(0x000b_0001), None);
    }
}
