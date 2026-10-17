macro_rules! catalog {
    ($($variant:ident=$id:expr,$name:literal,$words:expr;)+) => {
        #[derive(Clone,Copy,Debug,PartialEq,Eq)] #[repr(u32)] pub enum ProcessApi32 {$($variant=$id,)+}
        impl ProcessApi32 {
            pub const ALL:&'static [Self]=&[$(Self::$variant,)+];
            pub fn id(self)->u32{self as u32}
            pub fn from_id(id:u32)->Option<Self>{Self::ALL.iter().copied().find(|api|api.id()==id)}
            pub fn name(self)->&'static str{match self{$(Self::$variant=>$name,)+}}
            pub fn stack_words(self)->u32{match self{$(Self::$variant=>$words,)+}}
        }
    }
}
catalog! {
 GetCurrentProcessId=0x000c_0001,"GetCurrentProcessId",0;
 GetCurrentThreadId=0x000c_0002,"GetCurrentThreadId",0;
 GetCurrentThread=0x000c_0003,"GetCurrentThread",0;
 GetCommandLineA=0x000c_0004,"GetCommandLineA",0;
 GetCommandLineW=0x000c_0005,"GetCommandLineW",0;
 GetEnvironmentVariableA=0x000c_0006,"GetEnvironmentVariableA",3;
 GetEnvironmentVariableW=0x000c_0007,"GetEnvironmentVariableW",3;
 SetEnvironmentVariableA=0x000c_0008,"SetEnvironmentVariableA",2;
 SetEnvironmentVariableW=0x000c_0009,"SetEnvironmentVariableW",2;
 ExpandEnvironmentStringsA=0x000c_000a,"ExpandEnvironmentStringsA",3;
 ExpandEnvironmentStringsW=0x000c_000b,"ExpandEnvironmentStringsW",3;
 GetEnvironmentStringsW=0x000c_000c,"GetEnvironmentStringsW",0;
 FreeEnvironmentStringsW=0x000c_000d,"FreeEnvironmentStringsW",1;
 GetEnvironmentStringsA=0x000c_000e,"GetEnvironmentStringsA",0;
 FreeEnvironmentStringsA=0x000c_000f,"FreeEnvironmentStringsA",1;
 QueryPerformanceCounter=0x000c_0010,"QueryPerformanceCounter",1;
 QueryPerformanceFrequency=0x000c_0011,"QueryPerformanceFrequency",1;
 GetTickCount=0x000c_0012,"GetTickCount",0;
 GetTickCount64=0x000c_0013,"GetTickCount64",0;
 GetSystemTimeAsFileTime=0x000c_0014,"GetSystemTimeAsFileTime",1;
 GetStartupInfoA=0x000c_0015,"GetStartupInfoA",1;
 GetStartupInfoW=0x000c_0016,"GetStartupInfoW",1;
 GetModuleFileNameA=0x000c_0017,"GetModuleFileNameA",3;
 GetModuleFileNameW=0x000c_0018,"GetModuleFileNameW",3;
 IsDebuggerPresent=0x000c_0019,"IsDebuggerPresent",0;
 CheckRemoteDebuggerPresent=0x000c_001a,"CheckRemoteDebuggerPresent",2;
 GetSystemInfo=0x000c_001b,"GetSystemInfo",1;
 GetVersion=0x000c_001c,"GetVersion",0;
 GetVersionExA=0x000c_001d,"GetVersionExA",1;
 GetVersionExW=0x000c_001e,"GetVersionExW",1;
 EncodePointer=0x000c_0020,"EncodePointer",1;
 DecodePointer=0x000c_0021,"DecodePointer",1;
 IsProcessorFeaturePresent=0x000c_0022,"IsProcessorFeaturePresent",1;
 GetModuleHandleExA=0x000c_0023,"GetModuleHandleExA",3;
 GetModuleHandleExW=0x000c_0024,"GetModuleHandleExW",3;
 GetStdHandle=0x000c_0025,"GetStdHandle",1;
 SetStdHandle=0x000c_0026,"SetStdHandle",2;
 GetHandleInformation=0x000c_0027,"GetHandleInformation",2;
 SetHandleInformation=0x000c_0028,"SetHandleInformation",3;
 GetComputerNameA=0x000c_0029,"GetComputerNameA",2;
 GetComputerNameW=0x000c_002a,"GetComputerNameW",2;
 CreateProcessA=0x000c_002b,"CreateProcessA",10;
 CreateProcessW=0x000c_002c,"CreateProcessW",10;
 GetExitCodeProcess=0x000c_002d,"GetExitCodeProcess",2;
 GetPriorityClass=0x000c_002e,"GetPriorityClass",1;
 SetPriorityClass=0x000c_002f,"SetPriorityClass",2;
 GetProcessVersion=0x000c_0030,"GetProcessVersion",1;
 TerminateProcess=0x000c_0031,"TerminateProcess",2;
 SetHandleCount=0x000c_0032,"SetHandleCount",1;
 SetLocalTime=0x000c_0033,"SetLocalTime",1;
 Beep=0x000c_0034,"Beep",2;
 OutputDebugStringA=0x000c_0035,"OutputDebugStringA",1;
 MulDiv=0x000c_0036,"MulDiv",3;
 GlobalAddAtomA=0x000c_0037,"GlobalAddAtomA",1;
 GlobalFindAtomA=0x000c_0038,"GlobalFindAtomA",1;
 GlobalDeleteAtom=0x000c_0039,"GlobalDeleteAtom",1;
 GlobalGetAtomNameA=0x000c_003a,"GlobalGetAtomNameA",3;
}
