use std::collections::BTreeMap;

use super::{Directory, DispatchError, GuestMemory, guest, paths, thread};
use crate::execution::Access;

#[derive(Clone, Copy)]
struct Entry {
    source: u16,
    start: u16,
    end: u16,
}

struct Search {
    entries: Vec<Entry>,
    position: usize,
}

pub(super) struct Searches {
    active: BTreeMap<u32, Search>,
    next: u32,
}

impl Default for Searches {
    fn default() -> Self {
        Self {
            active: BTreeMap::new(),
            next: 0x7300_0004,
        }
    }
}

impl Directory {
    pub(super) fn find_first(
        &mut self,
        source: u32,
        output: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        let input = paths::read(memory, source)?;
        let (parent, pattern) = match filter(&self.terminated[..self.terminated.len() - 1], &input)
        {
            Ok(filter) => filter,
            Err(paths::PathError::Unsupported) => return Err(DispatchError::Unsupported),
            Err(paths::PathError::Windows(error)) => return failure(memory, error, u32::MAX),
        };
        if !self.exists(&parent) {
            return failure(memory, 3, u32::MAX);
        }
        let entries = self.matches(&parent, pattern)?;
        let Some(&first) = entries.first() else {
            return failure(memory, 2, u32::MAX);
        };
        if self.searches.active.len() == 64 || self.searches.next > 0x73ff_fffc {
            return failure(memory, 8, u32::MAX);
        }
        write(memory, output, &self.record(first))?;
        let handle = self.searches.next;
        self.searches.active.insert(
            handle,
            Search {
                entries,
                position: 1,
            },
        );
        self.searches.next += 4;
        Ok(handle)
    }

    pub(super) fn find_next(
        &mut self,
        handle: u32,
        output: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        let Some(search) = self.searches.active.get(&handle) else {
            return failure(memory, 6, 0);
        };
        let Some(&entry) = search.entries.get(search.position) else {
            return failure(memory, 18, 0);
        };
        write(memory, output, &self.record(entry))?;
        self.searches
            .active
            .get_mut(&handle)
            .expect("live search")
            .position += 1;
        Ok(1)
    }

    pub(super) fn find_close(
        &mut self,
        handle: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if self.searches.active.remove(&handle).is_some() {
            Ok(1)
        } else {
            failure(memory, 6, 0)
        }
    }

    fn path(&self, source: usize) -> &[u8] {
        if source < self.declarations.len() {
            &self.declarations[source]
        } else {
            &self.files[source - self.declarations.len()].path
        }
    }

    fn name(&self, entry: Entry) -> &[u8] {
        &self.path(usize::from(entry.source))[usize::from(entry.start)..usize::from(entry.end)]
    }

    fn matches(&self, parent: &[u8], pattern: &[u8]) -> Result<Vec<Entry>, DispatchError> {
        let start = parent.len() + usize::from(parent.len() > 3);
        let mut entries = Vec::with_capacity(self.declarations.len() + self.files.len());
        for (source, path) in self.all_paths().enumerate() {
            if !path
                .get(..parent.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(parent))
                || (parent.len() > 3 && path.get(parent.len()) != Some(&b'\\'))
            {
                continue;
            }
            let Some(tail) = path.get(start..).filter(|tail| !tail.is_empty()) else {
                continue;
            };
            let name = tail.split(|&b| b == b'\\').next().expect("nonempty tail");
            if source >= self.declarations.len()
                && start + name.len() == path.len()
                && self.files[source - self.declarations.len()].removed
            {
                continue;
            }
            if !matches_name(pattern, name) {
                continue;
            }
            if name.len() > 259 {
                return Err(DispatchError::Unsupported);
            }
            entries.push(Entry {
                source: u16::try_from(source).expect("bounded catalog"),
                start: u16::try_from(start).expect("bounded path"),
                end: u16::try_from(start + name.len()).expect("bounded path"),
            });
        }
        entries.sort_by(|a, b| {
            self.name(*a)
                .iter()
                .map(u8::to_ascii_lowercase)
                .cmp(self.name(*b).iter().map(u8::to_ascii_lowercase))
        });
        entries.dedup_by(|a, b| self.name(*a).eq_ignore_ascii_case(self.name(*b)));
        Ok(entries)
    }

    fn record(&self, entry: Entry) -> [u8; 320] {
        let source = usize::from(entry.source);
        let directory =
            source < self.declarations.len() || usize::from(entry.end) < self.path(source).len();
        let mut record = [0; 320];
        record[0] = if directory { 0x10 } else { 0x80 };
        if !directory {
            let size = self.files[source - self.declarations.len()]
                .size
                .to_le_bytes();
            record[28..32].copy_from_slice(&size[4..]);
            record[32..36].copy_from_slice(&size[..4]);
        }
        let name = self.name(entry);
        record[44..44 + name.len()].copy_from_slice(name);
        record
    }
}

fn filter<'a>(current: &[u8], input: &'a [u8]) -> Result<(Vec<u8>, &'a [u8]), paths::PathError> {
    if input.is_empty() || input.last() == Some(&b'\\') {
        return Err(paths::PathError::Windows(123));
    }
    if input.get(1) == Some(&b':') && input.get(2) != Some(&b'\\') {
        return Err(paths::PathError::Unsupported);
    }
    let (parent, pattern) = input
        .iter()
        .rposition(|&b| b == b'\\')
        .map_or((&b"."[..], input), |separator| {
            (&input[..=separator], &input[separator + 1..])
        });
    // DOS *.* matches every directory entry, including names without a period.
    let pattern = if pattern == b"*.*" {
        &b"*"[..]
    } else {
        pattern
    };
    if parent.contains(&b'*') || parent.contains(&b'?') {
        return Err(paths::PathError::Unsupported);
    }
    if pattern != b"*" {
        let suffix = pattern.strip_prefix(b"*");
        let literal = suffix.unwrap_or(pattern);
        if literal.is_empty()
            || literal.contains(&b'*')
            || literal.contains(&b'?')
            || matches!(literal, b"." | b"..")
        {
            return Err(paths::PathError::Unsupported);
        }
        if suffix.is_some() {
            paths::validate_suffix(literal)?;
        } else {
            paths::validate_component(literal)?;
        }
    }
    Ok((paths::resolve(current, parent)?, pattern))
}

fn matches_name(pattern: &[u8], name: &[u8]) -> bool {
    pattern == b"*"
        || pattern.eq_ignore_ascii_case(name)
        || (pattern.starts_with(b"*")
            && name.len() >= pattern.len() - 1
            && name[name.len() - (pattern.len() - 1)..].eq_ignore_ascii_case(&pattern[1..]))
}

fn write(memory: &mut GuestMemory, output: u32, record: &[u8; 320]) -> Result<(), DispatchError> {
    guest::check(memory, output, record.len(), Access::Write)?;
    memory.write(u64::from(output), record)?;
    Ok(())
}

fn failure(memory: &mut GuestMemory, error: u32, value: u32) -> Result<u32, DispatchError> {
    thread::set_last_error(memory, error)?;
    Ok(value)
}
