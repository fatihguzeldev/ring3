use super::*;

impl CoreCache32 {
    pub(in crate::process) fn continuation_header(&self) -> &route::Header {
        &self.route_header
    }
    pub(in crate::process) fn continuation_header_mut(&mut self) -> &mut route::Header {
        &mut self.route_header
    }

    pub(super) fn route_index(index: u32) -> Option<usize> {
        index.checked_sub(1).map(|i| i as usize)
    }
    pub(super) fn oldest(&self) -> Option<usize> {
        Self::route_index(self.route_header.oldest)
    }
    pub(super) fn newest(&self) -> Option<usize> {
        Self::route_index(self.route_header.newest)
    }

    pub(super) fn route_capacity(count: usize) -> usize {
        if count == 0 {
            0
        } else {
            count.saturating_mul(2).max(16).next_power_of_two()
        }
    }

    pub(super) fn route_position(&self, pc: u32) -> Result<usize, usize> {
        self.route_position_key(PreferredKey {
            origin: None,
            target: pc,
        })
    }

    pub(super) fn route_position_key(&self, key: PreferredKey) -> Result<usize, usize> {
        if self.route_rows.is_empty() {
            return Err(0);
        }
        let mask = self.route_rows.len() - 1;
        let start = route::context_hash(key.target, key.origin) as usize & mask;
        let mut deleted = None;
        for offset in 0..=mask {
            let index = (start + offset) & mask;
            let row = &self.route_rows[index];
            match row.state {
                route::PRESENT
                    if row.pc == key.target
                        && row.has_origin == u32::from(key.origin.is_some())
                        && row.origin == key.origin.unwrap_or(0) =>
                {
                    return Ok(index);
                }
                route::EMPTY => return Err(deleted.unwrap_or(index)),
                route::DELETED if deleted.is_none() => deleted = Some(index),
                _ => {}
            }
        }
        Err(deleted.expect("continuation table retains spare buckets"))
    }

    pub(super) fn rebuild_routes(&mut self, capacity: usize) {
        assert_eq!(
            self.route_header.active, 0,
            "continuation pointers are stable during a lease"
        );
        let old = std::mem::replace(&mut self.route_rows, vec![route::Row::default(); capacity]);
        self.route_count = 0;
        for row in old.into_iter().filter(|row| row.state == route::PRESENT) {
            let index = self
                .route_position_key(PreferredKey {
                    target: row.pc,
                    origin: (row.has_origin != 0).then_some(row.origin),
                })
                .unwrap_err();
            self.route_rows[index] = row;
            self.route_count += 1;
        }
        self.refresh_route_pointers();
    }

    pub(super) fn reserve_routes(&mut self, additional: usize) {
        let required = Self::route_capacity(self.route_count.saturating_add(additional));
        if required > self.route_rows.len() {
            self.rebuild_routes(required);
        }
    }

    pub(super) fn refresh_route_pointers(&mut self) {
        self.route_header.rows = if self.route_rows.is_empty() {
            0
        } else {
            self.route_rows.as_ptr() as usize as u32
        };
        self.route_header.row_mask = self.route_rows.len().saturating_sub(1) as u32;
        self.route_header.owners = self.route_owners.as_ptr() as usize as u32;
        self.route_header.owner_len = self.route_owners.len() as u32;
        self.route_header.link_owners = self.link_owners.as_ptr() as usize as u32;
        self.route_header.slot_regions = self.slot_regions.as_ptr() as usize as u32;
        self.route_header.slot_generations = self.slot_generations.as_ptr() as usize as u32;
        self.route_header.slot_len = self.link_owners.len() as u32;
    }

    pub(super) fn refresh_route_pc(&mut self, pc: u32) {
        self.refresh_route_context(PreferredKey {
            origin: None,
            target: pc,
        });
    }

    pub(super) fn refresh_route_context(&mut self, key: PreferredKey) {
        let pc = key.target;
        let preference = self.contexts.get(key).filter(|owner| {
            self.region(*owner)
                .is_some_and(|entry| entry.code.hot_span(pc).is_some())
        });
        let keys = if key.origin.is_some() {
            [preference, None, None]
        } else {
            [
                preference
                    .or_else(|| self.window_entries.get(&pc).copied())
                    .filter(|owner| {
                        self.region(*owner)
                            .is_some_and(|entry| entry.code.hot_span(pc).is_some())
                    }),
                self.trace_entries
                    .get(&pc)
                    .copied()
                    .filter(|owner| {
                        self.region(*owner)
                            .is_some_and(|entry| entry.code.hot_span(pc).is_some())
                    })
                    .or_else(|| {
                        self.executable_entries.get(&pc).copied().filter(|owner| {
                            self.region(*owner)
                                .is_some_and(|entry| entry.code.hot_span(pc).is_some())
                        })
                    }),
                self.entries.get(&pc).copied().filter(|owner| {
                    self.region(*owner)
                        .is_some_and(|entry| entry.code.hot_span(pc).is_some())
                }),
            ]
        };
        let mut candidates = [route::Candidate::default(); 3];
        for (position, owner) in keys.into_iter().enumerate() {
            let Some(owner) = owner else {
                continue;
            };
            let Some(entry) = self.region(owner) else {
                continue;
            };
            let Some(span) = entry.code.hot_span(pc) else {
                continue;
            };
            candidates[position] = route::Candidate {
                identity: owner.identity,
                index: owner.index as u32 + 1,
                port: span.port,
                work: span.instructions,
                flags: if entry.code.is_execution_window() {
                    route::WINDOW_ROOT
                } else {
                    0
                },
            };
        }
        if candidates.iter().all(|candidate| candidate.identity == 0) {
            if let Ok(index) = self.route_position_key(key) {
                self.route_rows[index] = route::Row {
                    state: route::DELETED,
                    ..Default::default()
                };
                self.route_count -= 1;
            }
            return;
        }
        let index = match self.route_position_key(key) {
            Ok(index) => index,
            Err(index) => {
                self.route_count += 1;

                index
            }
        };
        self.route_rows[index] = route::Row {
            pc,
            state: route::PRESENT,
            origin: key.origin.unwrap_or(0),
            has_origin: u32::from(key.origin.is_some()),
            candidates,
        };
    }

    // called only by an authenticated native transition, or by the equivalent
    // generated lease path. The actual live retired count is never read from a
    // potentially stale Context snapshot while generated locals own it.
    pub(in crate::process) fn account_owner_retired(&mut self, actual: u32) {
        let h = &mut self.route_header;
        let completed = actual.wrapping_sub(h.retired_clock);
        h.retired_clock = actual;
        if h.active != 1
            || h.current_nonce != h.nonce
            || h.nonce == 0
            || h.flags & (route::COLD | route::TRANSIENT) != 0
        {
            return;
        }
        let Some(index) = Self::route_index(h.current_index) else {
            return;
        };
        let Some(owner) = self.route_owners.get_mut(index) else {
            return;
        };
        if owner.identity != h.current_id
            || owner.generation != h.current_generation
            || owner.slot != h.current_slot
        {
            return;
        }
        owner.completed_work = owner.completed_work.saturating_add(completed as u64);
    }

    pub(in crate::process) fn begin_continuation(
        &mut self,
        memory: &AddressSpace,
        context: u32,
        trace_arena: u32,
        selected: Option<&RegionRef>,
        pc: u32,
    ) -> bool {
        if self.route_header.active != 0 {
            return false;
        }
        let Some(nonce) = self.route_header.nonce.checked_add(1) else {
            return false;
        };
        self.refresh_route_pointers();
        self.route_header.nonce = nonce;
        self.route_header.active = 1;
        self.route_header.retired_clock = 0;
        self.route_header.context = context;
        self.route_header.memory = memory.identity();
        self.route_header.trace_arena = trace_arena;
        if let Some(selected) = selected {
            if !self.bind_continuation(memory, selected, pc) {
                self.end_continuation();
                return false;
            }
        } else {
            self.clear_continuation();
        }
        true
    }

    pub(in crate::process) fn bind_continuation(
        &mut self,
        memory: &AddressSpace,
        code: &RegionRef,
        pc: u32,
    ) -> bool {
        if self.route_header.active != 1
            || self.route_header.memory != memory.identity()
            || !self.precise_source_current(memory, code)
        {
            return false;
        }
        let (index, slot, generation, flags) = if code.is_transient() {
            (0, 0, 0, route::TRANSIENT)
        } else {
            let entry = self.retained(code).unwrap();
            let slot = entry.slot;
            let generation = self.slot_generations[slot as usize];
            let record = &mut self.route_owners[code.key.index];
            record.validation = memory.code_validation_version();
            (
                code.key.index as u32 + 1,
                slot,
                generation,
                if slot == 0 {
                    route::SELECTED_UNBOUND
                } else {
                    0
                },
            )
        };
        self.route_header.reserved = self.route_header.current_entry;
        self.route_header.current_index = index;
        self.route_header.current_slot = slot;
        self.route_header.current_id = code.identity();
        self.route_header.current_generation = generation;
        self.route_header.current_entry = pc;
        self.route_header.current_port = code.hot_span(pc).map_or(u32::MAX, |span| span.port);
        self.route_header.current_nonce = self.route_header.nonce;
        self.route_header.flags = flags;
        true
    }

    pub(in crate::process) fn clear_continuation(&mut self) {
        self.route_header.current_index = 0;
        self.route_header.current_slot = 0;
        self.route_header.current_id = 0;
        self.route_header.current_generation = 0;
        self.route_header.current_entry = 0;
        self.route_header.current_port = u32::MAX;
        self.route_header.current_nonce = self.route_header.nonce;
        self.route_header.flags = route::COLD;
    }

    pub(in crate::process) fn end_continuation(&mut self) {
        self.route_header.active = 0;
        self.route_header.current_nonce = 0;
    }

    pub(in crate::process) fn continuation_source(
        &self,
        memory: &AddressSpace,
        context: u32,
        slot: u32,
        generation: u64,
        selected: Option<&RegionRef>,
        allow_stale: bool,
        require_entry: bool,
    ) -> Option<RegionRef> {
        let h = &*self.route_header;
        if h.active != 1
            || h.context != context
            || h.memory != memory.identity()
            || h.nonce == 0
            || h.current_nonce != h.nonce
            || h.current_slot != slot
            || h.current_generation != generation
            || h.flags & !(route::TRANSIENT | route::SELECTED_UNBOUND) != 0
        {
            return None;
        }
        let code = if h.flags == route::TRANSIENT {
            selected
                .filter(|code| {
                    code.is_transient()
                        && slot == 0
                        && generation == 0
                        && code.identity() == h.current_id
                })?
                .clone()
        } else {
            let index = Self::route_index(h.current_index)?;
            let key = RegionKey {
                index,
                identity: h.current_id,
            };
            let entry = self.region(key)?;
            let record = self.route_owners.get(index)?;
            if record.identity != key.identity {
                return None;
            }
            if slot == 0 {
                if h.flags != route::SELECTED_UNBOUND
                    || entry.slot != 0
                    || selected.is_none_or(|selected| {
                        selected.key != key || !Rc::ptr_eq(&selected.code, &entry.code)
                    })
                {
                    return None;
                }
            } else if h.flags != 0
                || !self.reciprocal_slot(key, entry)
                || record.slot != slot
                || record.generation != generation
                || self.slot_generations.get(slot as usize) != Some(&generation)
            {
                return None;
            }
            RegionRef {
                key,
                code: entry.code.clone(),
            }
        };
        if require_entry && !code.supports_entry(h.current_entry)
            || !allow_stale && !code.is_current(memory)
        {
            return None;
        }
        Some(code)
    }
}
