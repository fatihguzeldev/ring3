use super::FileCallOutcome32;
use crate::{
    files::FileError32, memory::AddressSpace, process::windows::files::FileRuntime32,
    windows::files::FileApi32,
};

impl FileRuntime32 {
    pub(in crate::process::windows::files) fn execute_ordinary_with_security(
        &mut self,
        memory: &mut AddressSpace,
        security: &crate::windows::security::SecurityRuntime32,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        if matches!(
            api,
            FileApi32::GetPrivateProfileIntA
                | FileApi32::GetPrivateProfileStringA
                | FileApi32::GetProfileIntA
                | FileApi32::WritePrivateProfileStringA
        ) {
            return self.profile_call(memory, security.token(), api, args);
        }
        if matches!(api, FileApi32::CreateFileA | FileApi32::CreateFileW) {
            return self.open_file_with_security(memory, api, args, security.token());
        }
        if api == FileApi32::SetFileInformationByHandle {
            return self.set_file_information(memory, args, security.token());
        }
        self.check_ordinary_security(memory, api, args, security.token())?;
        if matches!(
            api,
            FileApi32::CreateDirectoryA
                | FileApi32::CreateDirectoryW
                | FileApi32::CreateDirectoryExA
                | FileApi32::CreateDirectoryExW
        ) {
            return self.create_directory_with_security(memory, api, args, security.token());
        }
        self.execute_ordinary(memory, api, args)
    }
    pub(in crate::process::windows::files) fn execute_ordinary(
        &mut self,
        memory: &mut AddressSpace,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        use FileApi32::*;
        match api {
            GetErrorMode | SetErrorMode | GetThreadErrorMode | SetThreadErrorMode => {
                self.file_error_mode(memory, api, args)
            }
            GetDriveTypeA
            | GetDriveTypeW
            | GetLogicalDrives
            | GetLogicalDriveStringsA
            | GetLogicalDriveStringsW
            | GetDiskFreeSpaceA
            | GetDiskFreeSpaceW
            | GetDiskFreeSpaceExA
            | GetDiskFreeSpaceExW
            | GetVolumeInformationA
            | GetVolumeInformationW => self.query_volume(memory, api, args),
            FindFirstFileA | FindFirstFileW | FindFirstFileExA | FindFirstFileExW
            | FindNextFileA | FindNextFileW | FindClose => self.search_file(memory, api, args),
            CreateDirectoryA
            | CreateDirectoryW
            | CreateDirectoryExA
            | CreateDirectoryExW
            | RemoveDirectoryA
            | RemoveDirectoryW
            | DeleteFileA
            | DeleteFileW
            | SetFileAttributesA
            | SetFileAttributesW
            | SetCurrentDirectoryA
            | SetCurrentDirectoryW
            | CopyFileA
            | CopyFileW
            | CopyFileExA
            | CopyFileExW
            | MoveFileA
            | MoveFileW
            | MoveFileExA
            | MoveFileExW
            | MoveFileWithProgressA
            | MoveFileWithProgressW
            | ReplaceFileA
            | ReplaceFileW => self.mutate_path(memory, api, args),
            GetFileAttributesA | GetFileAttributesW | GetFileAttributesExA
            | GetFileAttributesExW | GetCurrentDirectoryA | GetCurrentDirectoryW
            | GetFullPathNameA | GetFullPathNameW | GetLongPathNameA | GetLongPathNameW
            | GetShortPathNameA | GetShortPathNameW | GetWindowsDirectoryA
            | GetWindowsDirectoryW | GetSystemDirectoryA | GetSystemDirectoryW => {
                self.query_path(memory, api, args)
            }
            GetTempPathA | GetTempPathW | GetTempFileNameA | GetTempFileNameW => {
                self.temporary_path(memory, api, args)
            }
            SearchPathA | SearchPathW => self.search_path(memory, api, args),
            GetFileInformationByHandleEx => self.file_information_ex(memory, args),
            SetFileInformationByHandle => self.set_file_information(
                memory,
                args,
                &crate::windows::security::Token32::virtual_process(),
            ),
            GetFinalPathNameByHandleA | GetFinalPathNameByHandleW => {
                self.final_file_path(memory, api, args)
            }
            LockFile | UnlockFile => self.lock_file_call(api, args),
            CreateFileA | CreateFileW => self.open_file(memory, api, args),
            ReadFile | WriteFile => self.transfer_file(memory, api, args),
            SetFilePointer | SetFilePointerEx | GetFileSize | GetFileSizeEx | SetEndOfFile => {
                self.position_file(memory, api, args)
            }
            FlushFileBuffers
            | GetFileType
            | GetFileInformationByHandle
            | GetFileTime
            | SetFileTime => self.file_metadata(memory, api, args),
            _ => Err(FileError32::Unsupported),
        }
    }
}
