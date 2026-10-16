use crate::windows::CallingConvention32;
macro_rules! catalog {
    ($($variant:ident=$id:expr,$words:expr,$name:literal;)+) => {
        #[derive(Clone,Copy,Debug,PartialEq,Eq)] #[repr(u32)] pub enum NlsApi32 {$($variant=0x0015_0000 | $id,)+}
        impl NlsApi32 {
            pub const ALL:&'static [Self]=&[$(Self::$variant,)+];
            pub const fn id(self)->u32{self as u32}
            pub fn from_id(id:u32)->Option<Self>{Self::ALL.iter().copied().find(|api|api.id()==id)}
            pub const fn name(self)->&'static str{match self{$(Self::$variant=>$name,)+}}
            pub const fn stack_words(self)->u32{match self{$(Self::$variant=>$words,)+}}
            pub const fn convention(self)->CallingConvention32 { CallingConvention32::Stdcall }
        }
    }
}
catalog! {
 GetACP=1,0,"GetACP"; GetOEMCP=2,0,"GetOEMCP"; IsValidCodePage=3,1,"IsValidCodePage"; GetCPInfo=4,2,"GetCPInfo"; GetCPInfoExA=5,3,"GetCPInfoExA"; GetCPInfoExW=6,3,"GetCPInfoExW";
 MultiByteToWideChar=7,6,"MultiByteToWideChar"; WideCharToMultiByte=8,8,"WideCharToMultiByte";
 LCMapStringA=9,6,"LCMapStringA"; LCMapStringW=10,6,"LCMapStringW"; CompareStringA=11,6,"CompareStringA"; CompareStringW=12,6,"CompareStringW"; CompareStringOrdinal=13,5,"CompareStringOrdinal";
 GetStringTypeA=14,5,"GetStringTypeA"; GetStringTypeW=15,4,"GetStringTypeW"; GetStringTypeExA=16,5,"GetStringTypeExA"; GetStringTypeExW=17,5,"GetStringTypeExW";
 GetLocaleInfoA=18,4,"GetLocaleInfoA"; GetLocaleInfoW=19,4,"GetLocaleInfoW"; GetUserDefaultLCID=20,0,"GetUserDefaultLCID"; GetSystemDefaultLCID=21,0,"GetSystemDefaultLCID";
 GetThreadLocale=22,0,"GetThreadLocale"; SetThreadLocale=23,1,"SetThreadLocale"; GetUserDefaultLangID=24,0,"GetUserDefaultLangID"; GetSystemDefaultLangID=25,0,"GetSystemDefaultLangID";
 GetThreadUILanguage=26,0,"GetThreadUILanguage"; SetThreadUILanguage=27,1,"SetThreadUILanguage"; IsValidLocale=28,2,"IsValidLocale";
 GetSystemTime=29,1,"GetSystemTime"; GetLocalTime=30,1,"GetLocalTime"; SystemTimeToFileTime=31,2,"SystemTimeToFileTime"; FileTimeToSystemTime=32,2,"FileTimeToSystemTime";
 FileTimeToLocalFileTime=33,2,"FileTimeToLocalFileTime"; LocalFileTimeToFileTime=34,2,"LocalFileTimeToFileTime"; CompareFileTime=35,2,"CompareFileTime";
 GetTimeZoneInformation=36,1,"GetTimeZoneInformation"; SystemTimeToTzSpecificLocalTime=37,3,"SystemTimeToTzSpecificLocalTime"; TzSpecificLocalTimeToSystemTime=38,3,"TzSpecificLocalTimeToSystemTime";
 DosDateTimeToFileTime=39,3,"DosDateTimeToFileTime"; FileTimeToDosDateTime=40,3,"FileTimeToDosDateTime";
 GetDateFormatA=41,6,"GetDateFormatA"; GetDateFormatW=42,6,"GetDateFormatW"; GetTimeFormatA=43,6,"GetTimeFormatA"; GetTimeFormatW=44,6,"GetTimeFormatW";
 GetCalendarInfoA=45,6,"GetCalendarInfoA"; GetCalendarInfoW=46,6,"GetCalendarInfoW";
 LstrlenA=47,1,"lstrlenA"; LstrlenW=48,1,"lstrlenW"; LstrcmpA=49,2,"lstrcmpA"; LstrcmpW=50,2,"lstrcmpW"; LstrcmpiA=51,2,"lstrcmpiA"; LstrcmpiW=52,2,"lstrcmpiW";
 LstrcpyA=53,2,"lstrcpyA"; LstrcpyW=54,2,"lstrcpyW"; LstrcpynA=55,3,"lstrcpynA"; LstrcpynW=56,3,"lstrcpynW"; LstrcatA=57,2,"lstrcatA"; LstrcatW=58,2,"lstrcatW";
 IsDBCSLeadByte=59,1,"IsDBCSLeadByte"; IsDBCSLeadByteEx=60,2,"IsDBCSLeadByteEx";
 GetLocaleInfoEx=61,4,"GetLocaleInfoEx"; GetUserDefaultLocaleName=62,2,"GetUserDefaultLocaleName";
 IsValidLocaleName=63,1,"IsValidLocaleName"; LCIDToLocaleName=64,4,"LCIDToLocaleName"; LocaleNameToLCID=65,2,"LocaleNameToLCID";
}
