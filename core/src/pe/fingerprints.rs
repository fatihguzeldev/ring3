use sha2::{Digest, Sha256};

use super::{PeDeclaredEvidence, inspect_pe_declared_evidence};

const MAX_SHA256_BYTES: u64 = u64::MAX / 8;
const _: () = assert!(usize::BITS <= 64);

/// owned content observations; public fields can also be supplied by a caller.
/// equality does not authenticate a source or prove that hash collisions cannot occur.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeFingerprintedEvidence {
    pub byte_length: u64,
    /// sha-256 over every supplied byte, including uninspected and trailing bytes.
    /// this is not the pe authenticode hash.
    pub digest: [u8; 32],
    pub evidence: PeDeclaredEvidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeFingerprintError {
    InputTooLarge { length: u64, limit: u64 },
    Sha256LengthExceeded { length: u64, limit: u64 },
}

fn admit_length(length: u64, limit: u64) -> Result<(), PeFingerprintError> {
    if length > limit {
        return Err(PeFingerprintError::InputTooLarge { length, limit });
    }
    if length > MAX_SHA256_BYTES {
        return Err(PeFingerprintError::Sha256LengthExceeded {
            length,
            limit: MAX_SHA256_BYTES,
        });
    }
    Ok(())
}

/// hashes the whole input and collects declared evidence from the same bytes.
///
/// checks the caller's byte limit before the sha-256 message-length limit, then
/// hashes and inspects the same immutable slice. empty or malformed pe input still
/// receives a digest when admitted; independent reader errors remain in evidence.
/// no path, label or metadata subset participates in the digest. the input limit
/// does not bound process memory, perform acquisition or establish cache policy.
///
/// # errors
/// returns the caller-budget error first, then the sha-256 length error when the
/// message exceeds `u64::MAX / 8` bytes. neither refusal hashes or reads pe fields.
///
/// # panics
/// only if an internal successful-reader invariant is violated. the supported
/// pointer widths fit into u64; malformed bytes produce the usual reader errors.
///
/// # example
/// ```
/// use ring3_core::fingerprint_pe_declared_evidence;
/// let result = fingerprint_pe_declared_evidence(b"abc", 3).unwrap();
/// assert_eq!(result.byte_length, 3);
/// assert_eq!(&result.digest[..4], &[0xba, 0x78, 0x16, 0xbf]);
/// assert!(result.evidence.prefix.is_err());
/// ```
#[expect(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    reason = "project documentation headings are lower case"
)]
pub fn fingerprint_pe_declared_evidence(
    bytes: &[u8],
    max_bytes: u64,
) -> Result<PeFingerprintedEvidence, PeFingerprintError> {
    let byte_length = u64::try_from(bytes.len()).expect("usize fits u64 on the supported targets");
    admit_length(byte_length, max_bytes)?;
    let digest = Sha256::digest(bytes).into();
    let evidence = inspect_pe_declared_evidence(bytes);
    Ok(PeFingerprintedEvidence {
        byte_length,
        digest,
        evidence,
    })
}

#[cfg(test)]
mod tests {
    use super::{MAX_SHA256_BYTES, PeFingerprintError, admit_length};

    #[test]
    fn symbolic_lengths_preserve_both_limits_and_refusal_precedence() {
        use PeFingerprintError::{InputTooLarge, Sha256LengthExceeded};
        let max = MAX_SHA256_BYTES;
        for (length, limit, expected) in [
            (0, 0, Ok(())),
            (
                1,
                0,
                Err(InputTooLarge {
                    length: 1,
                    limit: 0,
                }),
            ),
            (max, max, Ok(())),
            (
                max + 1,
                max,
                Err(InputTooLarge {
                    length: max + 1,
                    limit: max,
                }),
            ),
            (
                max + 1,
                u64::MAX,
                Err(Sha256LengthExceeded {
                    length: max + 1,
                    limit: max,
                }),
            ),
            (
                u64::MAX,
                u64::MAX,
                Err(Sha256LengthExceeded {
                    length: u64::MAX,
                    limit: max,
                }),
            ),
            (
                u64::MAX,
                0,
                Err(InputTooLarge {
                    length: u64::MAX,
                    limit: 0,
                }),
            ),
            (0, u64::MAX, Ok(())),
        ] {
            assert_eq!(admit_length(length, limit), expected);
        }
    }
}
