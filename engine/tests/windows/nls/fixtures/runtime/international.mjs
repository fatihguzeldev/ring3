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
  };
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
