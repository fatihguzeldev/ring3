export const internationalIds = {
  GetUserDefaultLCID: 0x150014,
  GetThreadLocale: 0x150016,
  SetThreadLocale: 0x150017,
  GetCalendarInfoW: 0x15002e,
  IsDBCSLeadByteEx: 0x15003c,
  GetLocaleInfoEx: 0x15003d,
  GetUserDefaultLocaleName: 0x15003e,
  IsValidLocaleName: 0x15003f,
  LCIDToLocaleName: 0x150040,
  LocaleNameToLCID: 0x150041,
  EnumSystemLocalesEx: 0x150042,
};
export const internationalCallCount = 33;

export function appendInternational(b, call, data, wide) {
  const a = b.addresses;
  const turkish = wide(b, 'turkish', 'iıIİ');
  const japaneseBytes = data(b, 'japaneseBytes', Buffer.from([0x82, 0xa0]));
  const turkishBytes = data(b, 'turkishBytes', Buffer.from([0xfd]));
  const germanPicture = wide(b, 'germanPicture', 'dddd, d. MMMM yyyy');
  const numericPicture = wide(b, 'numericPicture', 'yyyy-MM-dd');
  const z = wide(b, 'z', 'z');
  const umlaut = wide(b, 'umlaut', 'ä');
  call('GetUserDefaultLCID', [], 0x41f);
  call('SetThreadLocale', [0x411], 1);
  call('GetThreadLocale', [], 0x411);
  call('MultiByteToWideChar', [3, 8, japaneseBytes, 2, a.buffer, 32], 1, 'international-japanese');
  call('MultiByteToWideChar', [0, 8, turkishBytes, 1, a.buffer, 32], 1, 'international-turkish-byte');
  call('IsDBCSLeadByteEx', [932, 0x82], 1);
  call('LCMapStringW', [0x41f, 0x01000200, turkish, 0xffffffff, a.buffer, 64], 5, 'international-upper');
  call('LCMapStringW', [0x41f, 0x01000100, turkish, 0xffffffff, a.buffer, 64], 5, 'international-lower');
  call('GetLocaleInfoW', [0x40c, 0x39, a.buffer, 64], 'février'.length + 1, 'international-french');
  call('GetLocaleInfoW', [0x419, 0x38, a.buffer, 64], 'январь'.length + 1, 'international-russian');
  call('GetLocaleInfoW', [0x419, 0x10000038, a.buffer, 64], 'января'.length + 1, 'international-genitive');
  call('GetLocaleInfoW', [0x407, 0xe, a.buffer, 64], 2, 'international-decimal');
  call('GetLocaleInfoW', [0x407, 0xf, a.buffer, 64], 2, 'international-group');
  call('GetLocaleInfoW', [0x41f, 5, a.buffer, 64], 3, 'international-country');
  call('GetDateFormatW', [0x407, 0, a.date, germanPicture, a.buffer, 128], 'Donnerstag, 29. Februar 2024'.length + 1, 'international-german-date');
  call('GetDateFormatW', [0x41e, 0, a.date, numericPicture, a.buffer, 128], 11, 'international-thai-date');
  for (const [calendar, month] of [[9, 'août'], [10, 'آب'], [11, 'أغسطس'], [12, 'أوت']]) {
    call('GetCalendarInfoW', [0x401, calendar, 28, a.buffer, 64, 0], month.length + 1, `international-calendar-${calendar}`);
  }
  call('GetCalendarInfoW', [0x40d, 8, 33, a.buffer, 64, 0], 'אלול'.length + 1, 'international-hebrew');
  call('CompareStringW', [0x41d, 0, z, 0xffffffff, umlaut, 0xffffffff], 1);
  call('CompareStringW', [0x407, 0, z, 0xffffffff, umlaut, 0xffffffff], 3);
  call('GetLocaleInfoW', [0xf00411, 0xe, a.buffer, 64], 0, 'international-invalid-locale');
  const localeName = wide(b, 'localeName', 'tR-tr');
  const japaneseName = wide(b, 'japaneseName', 'ja-JP');
  const neutralName = wide(b, 'neutralName', 'en');
  const invalidName = wide(b, 'invalidName', 'unknown-locale');
  call('GetUserDefaultLocaleName', [a.buffer, 85], 6, 'international-profile-name');
  call('LocaleNameToLCID', [localeName, 0], 0x41f);
  call('LCIDToLocaleName', [0x10407, a.buffer, 85, 0], 13, 'international-sort-name');
  call('IsValidLocaleName', [invalidName], 0);
  call('GetLocaleInfoEx', [japaneseName, 0x20001004, a.buffer, 2], 2, 'international-name-codepage');
  call('LocaleNameToLCID', [neutralName, 0], 0x409);
  call('LocaleNameToLCID', [neutralName, 0x08000000], 9);
  call('LCIDToLocaleName', [0x7f, a.buffer, 85, 0], 1, 'international-invariant-name');
  call('EnumSystemLocalesEx', [b.callbacks[0].entry, 0x21, 0xabcdef, 0], 1, 'international-enumeration');

}
