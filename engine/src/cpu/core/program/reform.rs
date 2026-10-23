use super::*;

const MAX_REFORM_ENTRIES: usize = 1024;

impl CompiledCode {
    #[cfg(test)]
    pub(crate) fn with_entry(
        &self,
        memory: &AddressSpace,
        requested_pc: GuestAddress,
        options: CompileOptions,
        gates: &[(GuestAddress, u32)],
    ) -> Result<Option<Self>, CompileError> {
        validate_options(options)?;
        if self
            .entries
            .binary_search_by_key(&requested_pc.0, |entry| entry.0)
            .is_ok()
        {
            return Ok(None);
        }
        Self::reform_union(memory, &[self], requested_pc, options, gates)
    }

    pub(crate) fn reform_union(
        memory: &AddressSpace,
        owners: &[&Self],
        entry: GuestAddress,
        options: CompileOptions,
        gates: &[(GuestAddress, u32)],
    ) -> Result<Option<Self>, CompileError> {
        validate_options(options)?;
        if owners.is_empty() || owners.iter().any(|owner| !owner.is_current(memory)) {
            return Ok(None);
        }
        let mut pcs = BTreeSet::new();
        let mut entries = BTreeSet::from([entry.0]);
        let mut roots = BTreeSet::from([entry.0]);
        for owner in owners {
            for &pc in &owner.instruction_pcs {
                pcs.insert(pc);
                if pcs.len() > options.instructions {
                    return Ok(None);
                }
            }
            entries.extend(owner.entries.iter().map(|entry| entry.0));
            entries.extend(owner.roots.iter().map(|root| root.0));
            roots.extend(owner.roots.iter().map(|root| root.0));
        }
        // The caller's block budget bounds discovery of new code. Entry splits
        // introduce no instruction starts, so they use the already owned set's
        // size, subject to the compiler's unchanged hard entry ceiling.
        let entry_budget = pcs.len().min(MAX_REFORM_ENTRIES);
        if entries.len() > entry_budget
            || !pcs.contains(&entry.0)
            || !entries.iter().all(|entry| pcs.contains(entry))
        {
            return Ok(None);
        }

        // Decode only the already owned instruction starts. Direct targets and
        // fallthroughs are never followed here, even when they are readable.
        let mut nodes = BTreeMap::new();
        let mut sources = Vec::with_capacity(pcs.len());
        for &pc in &pcs {
            let (instruction, source) = decode_instruction(memory, pc, gates)?;
            nodes.insert(pc, instruction);
            sources.push(source);
        }
        let blocks = partition_blocks(nodes, &entries);
        if blocks.len() > entry_budget {
            return Ok(None);
        }
        let roots = roots.into_iter().map(GuestAddress).collect();
        let Some(candidate) = encode_blocks(
            memory,
            entry,
            CompileOptions {
                blocks: entry_budget,
                ..options
            },
            blocks,
            sources,
            roots,
            EmissionLimit::Decline,
        )?
        else {
            return Ok(None);
        };
        if !candidate.instruction_pcs.iter().copied().eq(pcs)
            || !entries.iter().all(|entry| {
                candidate
                    .entries
                    .binary_search_by_key(entry, |entry| entry.0)
                    .is_ok()
            })
        {
            return Ok(None);
        }
        Ok(Some(candidate))
    }
}

#[cfg(test)]
#[path = "reform_tests.rs"]
mod tests;
