// These OLEAUT32 words are independent of CLDR yes/no names. The native
// installed-locale probe records the fallback for languages without a resource.
pub(crate) fn localized_boolean(locale: u32, truth: bool) -> &'static str {
    let (false_word, true_word) = match locale & 0x3ff {
        0x05 => ("Nepravda", "Pravda"),
        0x06 => ("Falsk", "Sand"),
        0x07 => ("Falsch", "Wahr"),
        0x08 => ("Ψευδές", "Αληθές"),
        0x0a => ("Falso", "Verdadero"),
        0x0b => ("Epätosi", "Tosi"),
        0x0c => ("Faux", "Vrai"),
        0x0e => ("Hamis", "Igaz"),
        0x10 => ("Falso", "Vero"),
        0x13 => ("Onwaar", "Waar"),
        0x14 => ("Usann", "Sann"),
        0x15 => ("Fałsz", "Prawda"),
        0x16 => ("Falso", "Verdadeiro"),
        0x19 => ("Ложь", "Истина"),
        0x1d => ("Falskt", "Sant"),
        0x1f => ("Yanlış", "Doğru"),
        0x24 => ("Neresnično", "Resnično"),
        _ => ("False", "True"),
    };
    if truth { true_word } else { false_word }
}
