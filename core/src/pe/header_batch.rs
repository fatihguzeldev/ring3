use super::{PeHeaderError, PeHeaderPrefix, parse_pe_header_prefix};

/// caller-selected limits for already materialized inputs; not a memory cap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeHeaderBatchLimits {
    pub max_files: u64,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeHeaderBatchError {
    FileCountExceeded {
        count: u64,
        limit: u64,
    },
    FileSizeExceeded {
        index: usize,
        size: u64,
        limit: u64,
    },
    TotalSizeOverflow {
        index: usize,
        total: u64,
        size: u64,
    },
    TotalSizeExceeded {
        index: usize,
        total: u64,
        limit: u64,
    },
}

/// owned outcomes in input order; a prefix error does not stop later inputs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeHeaderBatch {
    /// logical bytes across all input occurrences, including repeated references.
    pub total_bytes: u64,
    pub headers: Vec<Result<PeHeaderPrefix, PeHeaderError>>,
}

/// checks every input budget before allocating results or reading any prefix.
///
/// zero limits are valid. input count is checked first, then each input's size,
/// aggregate overflow and aggregate limit, in input order. aliases count again.
/// admitted inputs each produce one owned prefix or error, without stopping on
/// a malformed prefix. recognition does not validate a loadable image.
///
/// # errors
/// returns the first budget failure with its exact operands. limits account for
/// already materialized logical input bytes, not allocation or process memory.
///
/// # example
/// ```
/// use ring3_core::{PeHeaderBatchLimits, parse_pe_header_prefix_batch};
/// let limits = PeHeaderBatchLimits {
///     max_files: 2, max_file_bytes: 4, max_total_bytes: 4,
/// };
/// let batch = parse_pe_header_prefix_batch(&[b"", b"bad"], limits)?;
/// assert_eq!(batch.total_bytes, 3);
/// assert_eq!(batch.headers.len(), 2);
/// assert!(batch.headers.iter().all(Result::is_err));
/// # Ok::<(), ring3_core::PeHeaderBatchError>(())
/// ```
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_header_prefix_batch(
    inputs: &[&[u8]],
    limits: PeHeaderBatchLimits,
) -> Result<PeHeaderBatch, PeHeaderBatchError> {
    parse_with(inputs, limits, parse_pe_header_prefix)
}

fn preflight(
    sizes: impl ExactSizeIterator<Item = u64>,
    limits: PeHeaderBatchLimits,
) -> Result<u64, PeHeaderBatchError> {
    let count = sizes.len() as u64;
    if count > limits.max_files {
        return Err(PeHeaderBatchError::FileCountExceeded {
            count,
            limit: limits.max_files,
        });
    }
    let mut total = 0_u64;
    for (index, size) in sizes.enumerate() {
        if size > limits.max_file_bytes {
            return Err(PeHeaderBatchError::FileSizeExceeded {
                index,
                size,
                limit: limits.max_file_bytes,
            });
        }
        total = total
            .checked_add(size)
            .ok_or(PeHeaderBatchError::TotalSizeOverflow { index, total, size })?;
        if total > limits.max_total_bytes {
            return Err(PeHeaderBatchError::TotalSizeExceeded {
                index,
                total,
                limit: limits.max_total_bytes,
            });
        }
    }
    Ok(total)
}

fn parse_with(
    inputs: &[&[u8]],
    limits: PeHeaderBatchLimits,
    mut parse: impl FnMut(&[u8]) -> Result<PeHeaderPrefix, PeHeaderError>,
) -> Result<PeHeaderBatch, PeHeaderBatchError> {
    let total_bytes = preflight(inputs.iter().map(|bytes| bytes.len() as u64), limits)?;
    let headers = inputs.iter().map(|bytes| parse(bytes)).collect();
    Ok(PeHeaderBatch {
        total_bytes,
        headers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_failure_never_reads_lengths() {
        let mut visits = 0;
        let sizes = [u64::MAX, 1].into_iter().inspect(|_| visits += 1);
        let limits = PeHeaderBatchLimits {
            max_files: 1,
            max_file_bytes: 0,
            max_total_bytes: 0,
        };
        assert_eq!(
            preflight(sizes, limits),
            Err(PeHeaderBatchError::FileCountExceeded { count: 2, limit: 1 })
        );
        assert_eq!(visits, 0);
    }

    #[test]
    fn every_rejected_batch_stays_out_of_the_parser() {
        let inputs: &[&[u8]] = &[b"", b"abcd"];
        for limits in [
            PeHeaderBatchLimits {
                max_files: 1,
                max_file_bytes: 4,
                max_total_bytes: 4,
            },
            PeHeaderBatchLimits {
                max_files: 2,
                max_file_bytes: 3,
                max_total_bytes: 4,
            },
            PeHeaderBatchLimits {
                max_files: 2,
                max_file_bytes: 4,
                max_total_bytes: 3,
            },
        ] {
            assert!(
                parse_with(inputs, limits, |_| panic!(
                    "parser entered a rejected batch"
                ))
                .is_err()
            );
        }
    }

    #[test]
    fn admitted_inputs_are_parsed_once_in_order_even_after_an_error() {
        let inputs: &[&[u8]] = &[b"", b"a", b"bc"];
        let mut seen = Vec::new();
        let limits = PeHeaderBatchLimits {
            max_files: 3,
            max_file_bytes: 2,
            max_total_bytes: 3,
        };
        let result = parse_with(inputs, limits, |bytes| {
            seen.push(bytes.to_vec());
            parse_pe_header_prefix(bytes)
        })
        .unwrap();
        assert_eq!(seen, inputs);
        assert_eq!(result.headers.len(), 3);
        assert!(result.headers.iter().all(Result::is_err));
    }

    fn check_lengths(
        sizes: &[u64],
        limits: [u64; 3],
        expected: Result<u64, PeHeaderBatchError>,
        expected_visits: usize,
    ) {
        let mut visits = 0;
        let actual = preflight(
            sizes.iter().copied().inspect(|_| visits += 1),
            PeHeaderBatchLimits {
                max_files: limits[0],
                max_file_bytes: limits[1],
                max_total_bytes: limits[2],
            },
        );
        assert_eq!(actual, expected);
        assert_eq!(visits, expected_visits);
    }

    #[test]
    fn synthetic_lengths_allow_zero_and_exact_u64_totals() {
        check_lengths(&[], [0, 0, 0], Ok(0), 0);
        check_lengths(&[0], [1, 0, 0], Ok(0), 1);
        check_lengths(&[0, 0], [2, 0, 0], Ok(0), 2);
        check_lengths(
            &[u64::MAX],
            [0, 0, 0],
            Err(PeHeaderBatchError::FileCountExceeded { count: 1, limit: 0 }),
            0,
        );
        check_lengths(&[u64::MAX], [1, u64::MAX, u64::MAX], Ok(u64::MAX), 1);
        check_lengths(&[u64::MAX, 0], [2, u64::MAX, u64::MAX], Ok(u64::MAX), 2);
        check_lengths(&[0, u64::MAX], [2, u64::MAX, u64::MAX], Ok(u64::MAX), 2);
    }

    #[test]
    fn synthetic_lengths_preserve_overflow_and_first_failure() {
        check_lengths(
            &[u64::MAX, 1],
            [2, u64::MAX, u64::MAX],
            Err(PeHeaderBatchError::TotalSizeOverflow {
                index: 1,
                total: u64::MAX,
                size: 1,
            }),
            2,
        );
        check_lengths(&[u64::MAX - 1, 1], [2, u64::MAX, u64::MAX], Ok(u64::MAX), 2);
        check_lengths(
            &[u64::MAX - 1, 2],
            [2, u64::MAX, u64::MAX],
            Err(PeHeaderBatchError::TotalSizeOverflow {
                index: 1,
                total: u64::MAX - 1,
                size: 2,
            }),
            2,
        );
        check_lengths(
            &[1, u64::MAX],
            [2, u64::MAX, u64::MAX],
            Err(PeHeaderBatchError::TotalSizeOverflow {
                index: 1,
                total: 1,
                size: u64::MAX,
            }),
            2,
        );
        check_lengths(
            &[1, u64::MAX],
            [2, u64::MAX - 1, u64::MAX],
            Err(PeHeaderBatchError::FileSizeExceeded {
                index: 1,
                size: u64::MAX,
                limit: u64::MAX - 1,
            }),
            2,
        );
        check_lengths(
            &[u64::MAX - 1, 1],
            [2, u64::MAX, u64::MAX - 1],
            Err(PeHeaderBatchError::TotalSizeExceeded {
                index: 1,
                total: u64::MAX,
                limit: u64::MAX - 1,
            }),
            2,
        );
        check_lengths(
            &[4_294_967_296, 4_294_967_296],
            [2, 4_294_967_296, 8_589_934_592],
            Ok(8_589_934_592),
            2,
        );
        check_lengths(
            &[4_294_967_296, 4_294_967_296],
            [2, 4_294_967_296, 8_589_934_591],
            Err(PeHeaderBatchError::TotalSizeExceeded {
                index: 1,
                total: 8_589_934_592,
                limit: 8_589_934_591,
            }),
            2,
        );
        check_lengths(
            &[4_294_967_296],
            [1, 4_294_967_295, u64::MAX],
            Err(PeHeaderBatchError::FileSizeExceeded {
                index: 0,
                size: 4_294_967_296,
                limit: 4_294_967_295,
            }),
            1,
        );
        check_lengths(
            &[2, u64::MAX],
            [2, 2, 1],
            Err(PeHeaderBatchError::TotalSizeExceeded {
                index: 0,
                total: 2,
                limit: 1,
            }),
            1,
        );
        check_lengths(
            &[u64::MAX, 1],
            [1, 0, 0],
            Err(PeHeaderBatchError::FileCountExceeded { count: 2, limit: 1 }),
            0,
        );
    }
}
