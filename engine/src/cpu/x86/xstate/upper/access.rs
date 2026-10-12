use super::super::access::MemorySpan;

pub(crate) const SAVE_SPANS: [MemorySpan; 6] = [
    MemorySpan {
        selection: 1,
        offset: 0,
        width: 24,
    },
    MemorySpan {
        selection: 6,
        offset: 24,
        width: 8,
    },
    MemorySpan {
        selection: 1,
        offset: 32,
        width: 128,
    },
    MemorySpan {
        selection: 2,
        offset: 160,
        width: 128,
    },
    MemorySpan {
        selection: 0,
        offset: 512,
        width: 8,
    },
    MemorySpan {
        selection: 4,
        offset: 576,
        width: 128,
    },
];
pub(crate) const RESTORE_SPANS: [MemorySpan; 6] = [
    MemorySpan {
        selection: 0,
        offset: 512,
        width: 24,
    },
    MemorySpan {
        selection: 1,
        offset: 0,
        width: 24,
    },
    MemorySpan {
        selection: 6,
        offset: 24,
        width: 4,
    },
    MemorySpan {
        selection: 1,
        offset: 32,
        width: 128,
    },
    MemorySpan {
        selection: 2,
        offset: 160,
        width: 128,
    },
    MemorySpan {
        selection: 4,
        offset: 576,
        width: 128,
    },
];

/// adjacent selected fields share one atomic memory span; all masks need at most four.
pub(crate) fn save_ranges(request: u64) -> ([(usize, usize); 4], usize) {
    let mut ranges = [(0, 0); 4];
    let mut count = 0;
    for span in SAVE_SPANS {
        if span.selection != 0 && request & u64::from(span.selection) == 0 {
            continue;
        }
        let start = span.offset as usize;
        let end = start + span.width as usize;
        if count != 0 && ranges[count - 1].1 == start {
            ranges[count - 1].1 = end;
        } else {
            ranges[count] = (start, end);
            count += 1;
        }
    }
    (ranges, count)
}
