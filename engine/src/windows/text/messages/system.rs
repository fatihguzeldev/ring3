pub(crate) fn system_message(code: u32) -> Option<&'static str> {
    Some(match code {
        0 => "The operation completed successfully.\r\n",
        1 => "Incorrect function.\r\n",
        2 => "The system cannot find the file specified.\r\n",
        3 => "The system cannot find the path specified.\r\n",
        5 => "Access is denied.\r\n",
        6 => "The handle is invalid.\r\n",
        8 => "Not enough memory resources are available to process this command.\r\n",
        14 => "Not enough storage is available to complete this operation.\r\n",
        18 => "There are no more files.\r\n",
        32 => "The process cannot access the file because it is being used by another process.\r\n",
        38 => "Reached the end of the file.\r\n",
        50 => "The request is not supported.\r\n",
        80 => "The file exists.\r\n",
        87 => "The parameter is incorrect.\r\n",
        109 => "The pipe has been ended.\r\n",
        112 => "There is not enough space on the disk.\r\n",
        122 => "The data area passed to a system call is too small.\r\n",
        126 => "The specified module could not be found.\r\n",
        127 => "The specified procedure could not be found.\r\n",
        183 => "Cannot create a file when that file already exists.\r\n",
        193 => "%1 is not a valid Win32 application.\r\n",
        206 => "The filename or extension is too long.\r\n",
        234 => "More data is available.\r\n",
        259 => "No more data is available.\r\n",
        317 => {
            "The system cannot find message text for message number 0x%1 in the message file for %2.\r\n"
        }
        487 => "Attempt to access invalid address.\r\n",
        995 => {
            "The I/O operation has been aborted because of either a thread exit or an application request.\r\n"
        }
        996 => "Overlapped I/O event is not in a signaled state.\r\n",
        997 => "Overlapped I/O operation is in progress.\r\n",
        998 => "Invalid access to memory location.\r\n",
        1114 => "A dynamic link library (DLL) initialization routine failed.\r\n",
        1130 => "Not enough server storage is available to process this command.\r\n",
        1450 => "Insufficient system resources exist to complete the requested service.\r\n",
        1812 => "The specified image file did not contain a resource section.\r\n",
        1813 => "The specified resource type cannot be found in the image file.\r\n",
        1814 => "The specified resource name cannot be found in the image file.\r\n",
        1815 => "The specified resource language ID cannot be found in the image file.\r\n",
        _ => return None,
    })
}
