import assert from 'node:assert/strict';

export function checkInternational(call, a, read, text) {
  const expected = {
    'international-profile-name': 'tr-TR',
    'international-sort-name': 'de-DE_phoneb',
    'international-invariant-name': '',
    'international-upper': 'İIIİ',
    'international-lower': 'iııi',
    'international-french': 'février',
    'international-russian': 'январь',
    'international-genitive': 'января',
    'international-decimal': ',',
    'international-group': '.',
    'international-country': '90',
    'international-german-date': 'Donnerstag, 29. Februar 2024',
    'international-thai-date': '2567-02-29',
    'international-calendar-9': 'août',
    'international-calendar-10': 'آب',
    'international-calendar-11': 'أغسطس',
    'international-calendar-12': 'أوت',
    'international-hebrew': 'אלול',
    'international-invalid-locale': 'אלול',
    'international-date-ltr': '\u200e2001/\u200e02/\u200e03',
    'international-date-rtl': '\u200f2001/\u200f02/\u200f03',
    'international-date-hebrew-numeric': "תשס\"א/ה'/י'",
    'international-date-hebrew-auto': "\u200fתשס\"א/\u200fה'/\u200fי'",
    'international-date-embedded-mark': '\u200eA\u200e',
    'international-urdu-native-name': 'اردو (پاکستان)',
    'international-urdu-date-auto': '\u200e2001/\u200e02/\u200e03',
  };
  if (call.hook === 'international-urdu-calendar') {
    assert.equal(read(a.buffer), 2);
    return true;
  }
  if(call.hook==='international-name-codepage'){
    assert.equal(read(a.buffer),932);return true;
  }
  if(call.hook==='international-enumeration'){
    assert.equal(read(a.localesCount),1);
    assert.equal(read(a.localesArguments+4),0x21);
    assert.equal(read(a.localesArguments+8),0xabcdef);
    assert.equal(text(a.localeCallbackName,true),'ar-SA');
    return true;
  }
  if (Object.hasOwn(expected, call.hook)) {
    assert.equal(text(a.buffer, true), expected[call.hook], call.hook);
    return true;
  }
  if (call.hook === 'international-japanese' || call.hook === 'international-turkish-byte') {
    assert.equal(read(a.buffer) & 0xffff, call.hook === 'international-japanese' ? 0x3042 : 0x131);
    return true;
  }
  return false;
}
