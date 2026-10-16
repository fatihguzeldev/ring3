use crate::windows::nls::NlsError32;
use icu_decimal::{
    DecimalFormatter, DecimalFormatterPreferences, input::Decimal, provider::DecimalSymbolsV1,
};
use icu_experimental::dimension::currency::{CurrencyCode, formatter::CurrencyFormatter};
use icu_provider::prelude::*;
use writeable::Writeable;

#[derive(Debug)]
pub(super) struct DecimalData32 {
    pub decimal: String,
    pub group: String,
    pub grouping: String,
    pub digits: String,
    pub negative: String,
    pub currency: String,
    pub currency_order: u32,
}
impl DecimalData32 {
    pub(super) fn new(locale: &icu_locale::Locale, currency: &str) -> Result<Self, NlsError32> {
        let preferences: DecimalFormatterPreferences = locale.clone().into();
        let locale_key = DecimalSymbolsV1::make_locale(preferences.locale_preferences);
        let symbols: DataResponse<DecimalSymbolsV1> = icu_decimal::provider::Baked
            .load(DataRequest {
                id: DataIdentifierBorrowed::for_locale(&locale_key),
                ..Default::default()
            })
            .map_err(|_| NlsError32::Unsupported)?;
        let symbols = symbols.payload;
        let symbols = symbols.get();
        let formatter = DecimalFormatter::try_new(preferences, Default::default())
            .map_err(|_| NlsError32::Unsupported)?;
        let digits = (0..10)
            .map(|value| {
                formatter
                    .format(&Decimal::from(value))
                    .write_to_string()
                    .into_owned()
            })
            .collect::<String>();
        let grouping = if symbols.grouping_sizes.primary == 0 {
            "0".into()
        } else if symbols.grouping_sizes.primary == symbols.grouping_sizes.secondary {
            format!("{};0", symbols.grouping_sizes.primary)
        } else {
            format!(
                "{};{};0",
                symbols.grouping_sizes.primary, symbols.grouping_sizes.secondary
            )
        };
        let currency_formatter =
            CurrencyFormatter::try_new(locale.clone().into(), Default::default())
                .map_err(|_| NlsError32::Unsupported)?;
        let currency_code =
            CurrencyCode(currency.parse().map_err(|_| NlsError32::InvalidParameter)?);
        let zero = Decimal::from(0);
        let number = formatter.format(&zero).write_to_string().into_owned();
        let money = currency_formatter
            .format_fixed_decimal(&zero, currency_code)
            .write_to_string()
            .into_owned();
        let number_index = money.find(&number).ok_or(NlsError32::Unsupported)?;
        let prefix = &money[..number_index];
        let suffix = &money[number_index + number.len()..];
        let currency_order = if prefix.is_empty() {
            if suffix.starts_with(char::is_whitespace) {
                3
            } else {
                1
            }
        } else if prefix.ends_with(char::is_whitespace) {
            2
        } else {
            0
        };
        let currency = if prefix.is_empty() {
            suffix.trim()
        } else {
            prefix.trim()
        }
        .to_string();
        Ok(Self {
            decimal: symbols.decimal_separator().into(),
            group: symbols.grouping_separator().into(),
            grouping,
            digits,
            negative: format!(
                "{}{}",
                symbols.minus_sign_affixes().0,
                symbols.minus_sign_affixes().1
            ),
            currency,
            currency_order,
        })
    }
}

pub(super) fn currency_name(
    locale: &icu_locale::Locale,
    currency: &str,
) -> Result<String, NlsError32> {
    use icu_experimental::dimension::currency::long_formatter::LongCurrencyFormatter;
    let code = CurrencyCode(currency.parse().map_err(|_| NlsError32::InvalidParameter)?);
    let formatter = LongCurrencyFormatter::try_new(locale.clone().into(), &code)
        .map_err(|_| NlsError32::Unsupported)?;
    let decimal = DecimalFormatter::try_new(locale.clone().into(), Default::default())
        .map_err(|_| NlsError32::Unsupported)?;
    let one = Decimal::from(1);
    let number = decimal.format(&one).write_to_string().into_owned();
    let text = formatter.format_fixed_decimal(&one, code).to_string();
    let (before, after) = text.split_once(&number).ok_or(NlsError32::Unsupported)?;
    Ok(format!("{before}{after}").trim().to_string())
}
