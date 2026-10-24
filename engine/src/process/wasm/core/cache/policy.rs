//! bounded optimization evidence. These records never grant execution authority.
use super::*;

pub(super) const POLICY_ROWS: usize = 4096;
const WAYS: usize = 4;
const CAPTURE_ALLOWANCE: usize = 16 << 10;
pub(super) const POLICY_BYTES: usize = 2 << 20;
const INITIAL_CREDIT: i64 = 4 << 20;
const MAX_CREDIT: i64 = 8 << 20;
const CREDIT_WORK: u32 = 32;
pub(super) const ATTEMPT_FLOOR: usize = 16 << 10;

#[derive(Clone)]
pub(super) enum SourceKey {
    Fragment(ObservedPlanFingerprint),
    Window(WindowFingerprint),
}

impl SourceKey {
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Fragment(a), Self::Fragment(b)) => a == b,
            (Self::Window(a), Self::Window(b)) => a.same_graph(b),
            _ => false,
        }
    }
    fn hash(&self) -> u64 {
        match self {
            Self::Fragment(key) => key.policy_hash(),
            Self::Window(key) => key.policy_hash(),
        }
    }
    fn charge(&self) -> usize {
        match self {
            Self::Fragment(key) => key.resident_charge(),
            Self::Window(key) => key.resident_charge(),
        }
    }
}

struct Record {
    key: SourceKey,
    charge: usize,
    capture: u64,
    observations: u8,
    last_charge: usize,
    stamp: u64,
}

pub(super) struct AutoPermit {
    key: SourceKey,
    bucket: usize,
    reserved: usize,
}

pub(super) struct PolicyBook {
    rows: Box<[Option<Record>]>,
    key_charge: usize,
    stamp: u64,
    credit: i64,
    remainder: u32,
    pub(super) attempts: u64,
    pub(super) deferred: u64,
    pub(super) completed: u64,
}

impl Default for PolicyBook {
    fn default() -> Self {
        Self {
            rows: (0..POLICY_ROWS).map(|_| None).collect(),
            key_charge: 0,
            stamp: 0,
            credit: INITIAL_CREDIT,
            remainder: 0,
            attempts: 0,
            deferred: 0,
            completed: 0,
        }
    }
}

impl PolicyBook {
    // the whole allowance is reserved in the executable ledger. A full cache
    // must not prevent recording the evidence needed for the next working set.
    pub(super) fn charge(&self) -> usize {
        POLICY_BYTES
    }
    fn allocated(&self) -> usize {
        self.rows.len() * size_of::<Option<Record>>() + self.key_charge + CAPTURE_ALLOWANCE
    }
    fn bucket(hash: u64) -> usize {
        ((hash ^ (hash >> 32)) as usize & (POLICY_ROWS / WAYS - 1)) * WAYS
    }
    fn find(&self, bucket: usize, key: &SourceKey) -> Option<usize> {
        (bucket..bucket + WAYS).find(|&i| self.rows[i].as_ref().is_some_and(|r| r.key.same(key)))
    }
    fn record(&mut self, bucket: usize, key: SourceKey) -> Option<usize> {
        if let Some(i) = self.find(bucket, &key) {
            return Some(i);
        }
        let charge = key.charge();
        if self.rows.len() * size_of::<Option<Record>>() + charge + CAPTURE_ALLOWANCE > POLICY_BYTES
        {
            return None;
        }
        let i = (bucket..bucket + WAYS)
            .find(|&i| self.rows[i].is_none())
            .unwrap_or_else(|| {
                (bucket..bucket + WAYS)
                    .min_by_key(|&i| self.rows[i].as_ref().unwrap().stamp)
                    .unwrap()
            });
        if let Some(old) = self.rows[i].take() {
            self.key_charge -= old.charge;
        }
        while self.allocated().saturating_add(charge) > POLICY_BYTES {
            let victim = self
                .rows
                .iter()
                .enumerate()
                .filter_map(|(j, r)| r.as_ref().map(|r| (j, r.stamp)))
                .min_by_key(|&(_, stamp)| stamp)
                .map(|(j, _)| j)?;
            self.key_charge -= self.rows[victim].take().unwrap().charge;
        }
        self.key_charge += charge;
        self.rows[i] = Some(Record {
            key,
            charge,
            capture: 0,
            observations: 0,
            last_charge: 0,
            stamp: self.stamp,
        });
        Some(i)
    }
    fn observe(&mut self, key: SourceKey, capture: u64, repeated: bool) -> bool {
        self.stamp = self.stamp.saturating_add(1);
        let bucket = Self::bucket(key.hash());
        let Some(i) = self.record(bucket, key) else {
            return repeated;
        };
        let record = self.rows[i].as_mut().unwrap();
        record.stamp = self.stamp;
        if repeated {
            record.observations = 2;
        } else if capture != 0 && record.capture != capture {
            record.capture = capture;
            record.observations = record.observations.saturating_add(1).min(2);
        }
        record.observations >= 2
    }
    pub(super) fn can_reserve(&self, key: &SourceKey, geometry: usize) -> bool {
        let known = self
            .find(Self::bucket(key.hash()), key)
            .and_then(|i| self.rows[i].as_ref())
            .map_or(0, |r| r.last_charge);
        self.credit >= 0
            && self.credit as u64 >= ATTEMPT_FLOOR.saturating_add(geometry).max(known) as u64
    }
    fn reserve(&mut self, key: SourceKey, geometry: usize) -> Option<AutoPermit> {
        let bucket = Self::bucket(key.hash());
        let known = self
            .find(bucket, &key)
            .and_then(|i| self.rows[i].as_ref())
            .map_or(0, |r| r.last_charge);
        let reserved = ATTEMPT_FLOOR.saturating_add(geometry).max(known);
        if self.credit < 0 || (self.credit as u64) < reserved as u64 {
            self.deferred = self.deferred.saturating_add(1);
            return None;
        }
        self.credit -= reserved as i64;
        self.attempts = self.attempts.saturating_add(1);
        Some(AutoPermit {
            key,
            bucket,
            reserved,
        })
    }
    pub(super) fn finish(&mut self, permit: AutoPermit, actual: usize) {
        let extra = actual.saturating_sub(permit.reserved);
        self.credit = self
            .credit
            .saturating_sub(extra.min(i64::MAX as usize) as i64);
        self.stamp = self.stamp.saturating_add(1);
        if let Some(i) = self.record(permit.bucket, permit.key) {
            let record = self.rows[i].as_mut().unwrap();
            record.last_charge = record.last_charge.max(actual);
            record.stamp = self.stamp;
        }
    }
    pub(super) fn earn(&mut self, work: u32) {
        self.completed = self.completed.saturating_add(work as u64);
        let total = work as u64 + self.remainder as u64;
        self.remainder = (total % CREDIT_WORK as u64) as u32;
        self.credit = self
            .credit
            .saturating_add((total / CREDIT_WORK as u64) as i64)
            .min(MAX_CREDIT);
    }
}

impl CoreCache32 {
    pub(in crate::process) fn can_capture_automatic(&self) -> bool {
        self.policy.credit >= ATTEMPT_FLOOR as i64
    }
    pub(in crate::process) fn earn_completed_work(&mut self, work: u32) {
        self.policy.earn(work);
    }
    pub(in crate::process) fn observe_window_candidate(
        &mut self,
        plan: &WindowPlan,
        capture_serial: u64,
        repeated: bool,
    ) -> bool {
        self.policy.observe(
            SourceKey::Window(plan.fingerprint().clone()),
            capture_serial,
            repeated,
        )
    }
    pub(super) fn reserve_fragment(&mut self, plan: &ObservedPlan) -> Option<AutoPermit> {
        let geometry = plan.instruction_pcs().count().saturating_mul(32);
        self.policy
            .reserve(SourceKey::Fragment(plan.fingerprint().clone()), geometry)
    }
    pub(super) fn reserve_window(&mut self, plan: &WindowPlan) -> Option<AutoPermit> {
        let geometry = plan.instruction_pcs().count().saturating_mul(32);
        self.policy
            .reserve(SourceKey::Window(plan.fingerprint().clone()), geometry)
    }

    pub(super) fn queue_ends(&self, protected: bool) -> (u32, u32) {
        if protected {
            (
                self.route_header.protected_oldest,
                self.route_header.protected_newest,
            )
        } else {
            (self.route_header.oldest, self.route_header.newest)
        }
    }
    fn set_queue_ends(&mut self, protected: bool, oldest: u32, newest: u32) {
        if protected {
            self.route_header.protected_oldest = oldest;
            self.route_header.protected_newest = newest;
        } else {
            self.route_header.oldest = oldest;
            self.route_header.newest = newest;
        }
    }
    pub(super) fn protected(&self, index: usize) -> bool {
        self.route_owners[index].flags & route::PROTECTED != 0
    }
    pub(super) fn detach_queue(&mut self, index: usize) {
        let row = self.route_owners[index];
        let protected = row.flags & route::PROTECTED != 0;
        let (mut oldest, mut newest) = self.queue_ends(protected);
        if let Some(i) = Self::route_index(row.older) {
            self.route_owners[i].newer = row.newer;
        } else {
            oldest = row.newer;
        }
        if let Some(i) = Self::route_index(row.newer) {
            self.route_owners[i].older = row.older;
        } else {
            newest = row.older;
        }
        self.set_queue_ends(protected, oldest, newest);
    }
    pub(super) fn append_queue(&mut self, index: usize, protected: bool) {
        let (mut oldest, newest) = self.queue_ends(protected);
        self.route_owners[index].older = newest;
        self.route_owners[index].newer = 0;
        if let Some(i) = Self::route_index(newest) {
            self.route_owners[i].newer = index as u32 + 1;
        } else {
            oldest = index as u32 + 1;
        }
        self.set_queue_ends(protected, oldest, index as u32 + 1);
    }
    fn move_tier(&mut self, index: usize, protected: bool) {
        let charge = self.regions[index].as_ref().unwrap().charge;
        if self.protected(index) != protected {
            self.detach_queue(index);
            if protected {
                self.route_owners[index].flags |= route::PROTECTED;
                self.protected_charge += charge;
                self.protected_count += 1;
            } else {
                self.route_owners[index].flags &= !route::PROTECTED;
                self.protected_charge -= charge;
                self.protected_count -= 1;
            }
            self.append_queue(index, protected);
        } else {
            self.touch(index);
        }
        self.regions[index].as_mut().unwrap().policy_baseline =
            self.route_owners[index].completed_work;
    }
    fn useful_since_review(&self, index: usize) -> bool {
        let entry = self.regions[index].as_ref().unwrap();
        entry.automatic
            && self.route_owners[index]
                .completed_work
                .saturating_sub(entry.policy_baseline)
                >= (entry.code.instruction_count() as u64)
                    .saturating_mul(4)
                    .max(256)
    }
    fn protected_target(&self) -> usize {
        MAX_CODE_BYTES.saturating_sub(self.directory_charge()) / 4 * 3
    }
    pub(super) fn review_protected(&mut self) {
        for _ in 0..self.protected_count.min(8) {
            let Some(index) = Self::route_index(self.route_header.protected_oldest) else {
                break;
            };
            let renew = self.useful_since_review(index);
            self.move_tier(index, renew);
        }
        while self.protected_charge > self.protected_target() {
            let Some(index) = Self::route_index(self.route_header.protected_oldest) else {
                break;
            };
            self.move_tier(index, false);
        }
    }
    pub(super) fn policy_victim(&mut self) -> Option<usize> {
        loop {
            if let Some(index) = self.oldest() {
                if self.useful_since_review(index) {
                    self.move_tier(index, true);
                    while self.protected_charge > self.protected_target() {
                        let old = Self::route_index(self.route_header.protected_oldest).unwrap();
                        self.move_tier(old, false);
                    }
                    continue;
                }
                return Some(index);
            }
            let index = Self::route_index(self.route_header.protected_oldest)?;
            self.move_tier(index, false);
        }
    }
}

#[cfg(test)]
#[path = "policy/tests.rs"]
mod tests;
