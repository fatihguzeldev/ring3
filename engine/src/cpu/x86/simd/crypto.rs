use super::CryptoKind;
use crate::math::{self, aes};

pub(super) fn execute(
    kind: CryptoKind,
    destination: [u8; 16],
    source: [u8; 16],
    immediate: Option<u8>,
) -> [u8; 16] {
    use CryptoKind as C;
    match kind {
        C::EncryptRound | C::EncryptLast => aes::round(
            destination,
            source,
            aes::Direction::Encrypt,
            kind == C::EncryptLast,
        ),
        C::DecryptRound | C::DecryptLast => aes::round(
            destination,
            source,
            aes::Direction::Decrypt,
            kind == C::DecryptLast,
        ),
        C::InverseMixColumns => aes::inverse_mix_columns(source),
        C::KeyAssist => aes::key_assist(source, immediate.unwrap()),
        C::CarrylessMultiply => {
            let immediate = immediate.unwrap();
            let left = usize::from(immediate & 1) * 8;
            let right = usize::from(immediate >> 4 & 1) * 8;
            math::carryless_product(
                u64::from_le_bytes(destination[left..left + 8].try_into().unwrap()),
                u64::from_le_bytes(source[right..right + 8].try_into().unwrap()),
            )
            .to_le_bytes()
        }
    }
}
