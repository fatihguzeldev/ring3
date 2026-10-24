//! Immutable sources for the shared, cold recovery executor.
//!
//! A local number alone is not a source: the allocator may reuse it at another
//! prefix, and an allocated lazy value may not have been initialized. Each
//! reference therefore records both its immutable value identity and the exact
//! source mode available at the requesting site. Dependencies are numbered
//! before their users, allowing bounded reverse/forward sweeps without a stack.

use crate::cpu::core::{
    emitter::executable::locals::Layout,
    flags::FlagKind,
    region::values::{
        ValueType,
        executable::{
            Binary32, ExecEffectId, ExecValueId, ExecutableProgram, Expr32, FlagsOrigin,
            FlagsRecipe, FlagsState, Recovery, ValueNode,
        },
    },
};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::cpu::core::emitter) struct RefId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core::emitter) struct Color {
    pub local: u32,
    pub ty: ValueType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core::emitter) enum ValueKey {
    Executable(ExecValueId),
    Flags { origin: FlagsOrigin, field: u8 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core::emitter) enum SourceOp {
    /// The requesting site's availability proof establishes this exact value
    /// in its color already. It must still participate in color validation.
    Available,
    /// An architectural input is still the same whole value at this site.
    /// The forward sweep captures it before projection changes any destination.
    PhysicalInput {
        local: u32,
    },
    Constant {
        bits: u64,
    },
    Binary32 {
        kind: Binary32,
        left: RefId,
        right: RefId,
    },
    ShiftLeft32 {
        value: RefId,
        amount: u8,
    },
    Fp(super::fp::Source),
}

impl SourceOp {
    pub fn dependencies(&self) -> Vec<RefId> {
        match self {
            Self::Binary32 { left, right, .. } => vec![*left, *right],
            Self::ShiftLeft32 { value, .. } => vec![*value],
            Self::Fp(source) => source.dependencies(),
            Self::Available | Self::PhysicalInput { .. } | Self::Constant { .. } => Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core::emitter) struct SourceRef {
    pub key: ValueKey,
    pub color: Color,
    pub op: SourceOp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core::emitter) struct KernelMasks {
    pub gpr: u8,
    pub flags: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core::emitter) struct ArithmeticFlags {
    pub kind: FlagKind,
    pub width: u32,
    pub left: RefId,
    pub right: RefId,
    pub result: RefId,
}

/// Origin order is FLAGS, KIND, WIDTH, LEFT, RIGHT, VALUE, CARRY_IN. The optional
/// recipe overrides the latter six fields, preserving the original base FLAGS.
/// Its inputs are evaluated before *any* GPR or flags destination is changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core::emitter) struct FlagsProjection {
    pub origin: [RefId; 7],
    pub arithmetic: Option<ArithmeticFlags>,
}

impl FlagsProjection {
    pub fn roots(&self) -> Vec<RefId> {
        let mut roots = self.origin.to_vec();
        if let Some(recipe) = &self.arithmetic {
            roots.extend([recipe.left, recipe.right, recipe.result]);
        }
        roots
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core::emitter) struct ScalarProjection {
    pub gpr: [Option<RefId>; 8],
    pub flags: Option<FlagsProjection>,
}

impl ScalarProjection {
    pub fn roots(&self) -> Vec<RefId> {
        self.gpr
            .iter()
            .flatten()
            .copied()
            .chain(self.flags.iter().flat_map(FlagsProjection::roots))
            .collect()
    }
}

/// All fields describe one immutable emission-site snapshot. In particular,
/// `available` must not be changed while building its scalar/FP descriptors.
pub(in crate::cpu::core::emitter) struct SourceContext<'a> {
    pub executable: &'a ExecutableProgram,
    pub layout: &'a Layout,
    pub available: &'a BTreeSet<ExecValueId>,
    pub physical_gpr: Option<[ExecValueId; 8]>,
    pub physical_flags: Option<FlagsState>,
    pub active_effect: Option<ExecEffectId>,
    pub captured_effect: Option<ExecEffectId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core::emitter) enum Error {
    ReferenceLimit,
    MissingLocal(ExecValueId),
    InvalidInput(ExecValueId),
    UncapturedEffect(ExecValueId),
    MissingFlags(FlagsOrigin),
    UncapturedFlags(FlagsOrigin),
    InvalidReference(RefId),
    ForwardDependency {
        reference: RefId,
        dependency: RefId,
    },
    TypeMismatch,
    ColorConflict {
        color: Color,
        first: RefId,
        second: RefId,
    },
}

/// The caller supplies a bound derived from the already bounded executable
/// program. Exact keys are used for interning; no hash/fingerprint equivalence
/// substitutes for the source, color or value identity. Pure literal sources
/// have no defining effect, so identical bits/type/color share one reference.
#[derive(Clone)]
pub(in crate::cpu::core::emitter) struct Program {
    refs: Vec<SourceRef>,
    interned: BTreeMap<Vec<u64>, RefId>,
    colors: BTreeMap<u32, ValueType>,
    limit: usize,
}

impl Program {
    pub fn new(limit: usize) -> Self {
        Self {
            refs: Vec::new(),
            interned: BTreeMap::new(),
            colors: BTreeMap::new(),
            // NeedTag uses +(id + 1) for pending and its negative for ready;
            // zero remains the unrequested state. Keep both signs exact i32.
            limit: limit.min(i32::MAX as usize),
        }
    }

    pub fn refs(&self) -> &[SourceRef] {
        &self.refs
    }

    pub fn reverse_refs(&self) -> impl Iterator<Item = (RefId, &SourceRef)> {
        self.refs
            .iter()
            .enumerate()
            .rev()
            .map(|(index, reference)| (RefId(index as u32), reference))
    }

    pub fn get(&self, id: RefId) -> &SourceRef {
        &self.refs[id.0 as usize]
    }

    /// Includes stable FLAGS origin tuples as well as executable values. These
    /// colors define the complete selector and per-color NeedTag namespace.
    pub fn colors(&self) -> impl Iterator<Item = Color> + '_ {
        self.colors.iter().map(|(&local, &ty)| Color { local, ty })
    }

    pub fn color(&self, id: ExecValueId, context: &SourceContext<'_>) -> Result<Color, Error> {
        let local = context
            .layout
            .values
            .get(id.0)
            .copied()
            .flatten()
            .ok_or(Error::MissingLocal(id))?;
        Ok(Color {
            local,
            ty: context.executable.value(id).ty,
        })
    }

    pub fn reference(&mut self, key: ValueKey, color: Color, op: SourceOp) -> Result<RefId, Error> {
        self.intern(SourceRef { key, color, op })
    }

    pub fn intern(&mut self, source: SourceRef) -> Result<RefId, Error> {
        let key = source_key(&source);
        if let Some(&id) = self.interned.get(&key) {
            return Ok(id);
        }
        if self.refs.len() == self.limit {
            return Err(Error::ReferenceLimit);
        }
        if self
            .colors
            .get(&source.color.local)
            .is_some_and(|&ty| ty != source.color.ty)
        {
            return Err(Error::TypeMismatch);
        }
        let id = RefId(self.refs.len() as u32);
        for dependency in source.op.dependencies() {
            let Some(input) = self.refs.get(dependency.0 as usize) else {
                return Err(Error::InvalidReference(dependency));
            };
            if dependency >= id {
                return Err(Error::ForwardDependency {
                    reference: id,
                    dependency,
                });
            }
            if matches!(
                source.op,
                SourceOp::Binary32 { .. } | SourceOp::ShiftLeft32 { .. }
            ) && input.color.ty != ValueType::I32
            {
                return Err(Error::TypeMismatch);
            }
        }
        if matches!(
            source.op,
            SourceOp::Binary32 { .. } | SourceOp::ShiftLeft32 { .. }
        ) && source.color.ty != ValueType::I32
        {
            return Err(Error::TypeMismatch);
        }
        self.colors.insert(source.color.local, source.color.ty);
        self.refs.push(source);
        self.interned.insert(key, id);
        Ok(id)
    }

    /// Resolve only pure expressions or already completed effect results.
    /// No load, arithmetic rounding point, observer or helper can be replayed.
    pub fn value(&mut self, id: ExecValueId, context: &SourceContext<'_>) -> Result<RefId, Error> {
        let color = self.color(id, context)?;
        let key = ValueKey::Executable(id);
        if context.available.contains(&id) {
            return self.reference(key, color, SourceOp::Available);
        }
        let op = match context.executable.value(id).node.clone() {
            ValueNode::Scalar(Expr32::Input { register, .. }) => {
                if context.physical_gpr.map(|gpr| gpr[usize::from(register)]) != Some(id) {
                    return Err(Error::InvalidInput(id));
                }
                SourceOp::PhysicalInput {
                    local: u32::from(register) + 2,
                }
            }
            ValueNode::Scalar(Expr32::Constant(value)) => SourceOp::Constant {
                bits: u64::from(value),
            },
            ValueNode::Scalar(Expr32::Binary { kind, left, right }) => {
                let left = self.value(left, context)?;
                let right = self.value(right, context)?;
                SourceOp::Binary32 { kind, left, right }
            }
            ValueNode::Scalar(Expr32::ShiftLeft { value, amount }) => {
                let value = self.value(value, context)?;
                SourceOp::ShiftLeft32 { value, amount }
            }
            ValueNode::Scalar(Expr32::EffectResult { effect, register }) => {
                if context.active_effect != Some(effect) {
                    return Err(Error::UncapturedEffect(id));
                }
                SourceOp::PhysicalInput {
                    local: u32::from(register) + 2,
                }
            }
            ValueNode::Fp(node) => return super::fp::source(self, id, &node, context),
        };
        self.reference(key, color, op)
    }

    fn flag_origin(
        &mut self,
        origin: FlagsOrigin,
        context: &SourceContext<'_>,
    ) -> Result<[RefId; 7], Error> {
        if let FlagsOrigin::Effect(effect) = origin {
            if context.active_effect == Some(effect) && context.captured_effect != Some(effect) {
                return Err(Error::UncapturedFlags(origin));
            }
        }
        let base = match origin {
            FlagsOrigin::Entry(block) => context.layout.flags_entry.get(block.0),
            FlagsOrigin::Effect(effect) => context.layout.flags_effect.get(effect.0),
        }
        .copied()
        .flatten()
        .ok_or(Error::MissingFlags(origin))?;
        let mut fields = [RefId(0); 7];
        for (index, field) in fields.iter_mut().enumerate() {
            *field = self.reference(
                ValueKey::Flags {
                    origin,
                    field: index as u8,
                },
                Color {
                    local: base + index as u32,
                    ty: ValueType::I32,
                },
                SourceOp::Available,
            )?;
        }
        Ok(fields)
    }

    pub fn scalar(
        &mut self,
        recovery: &Recovery,
        kernel: KernelMasks,
        context: &SourceContext<'_>,
    ) -> Result<ScalarProjection, Error> {
        let physical = context.physical_gpr.unwrap_or(recovery.gpr);
        let mut gpr = [None; 8];
        for register in 0..8 {
            if kernel.gpr & (1 << register) == 0 && physical[register] != recovery.gpr[register] {
                gpr[register] = Some(self.value(recovery.gpr[register], context)?);
            }
        }
        let flags = if kernel.flags || context.physical_flags == Some(recovery.flags) {
            None
        } else {
            let origin = self.flag_origin(recovery.flags.origin, context)?;
            let arithmetic = match recovery.flags.recipe {
                FlagsRecipe::Current => None,
                FlagsRecipe::Arithmetic {
                    kind,
                    width,
                    left,
                    right,
                    result,
                } => Some(ArithmeticFlags {
                    kind,
                    width,
                    left: self.value(left, context)?,
                    right: self.value(right, context)?,
                    result: self.value(result, context)?,
                }),
            };
            Some(FlagsProjection { origin, arithmetic })
        };
        let projection = ScalarProjection { gpr, flags };
        self.validate_roots(&projection.roots())?;
        Ok(projection)
    }

    /// Validate each descriptor's selectable closure, not the union of unrelated
    /// sites. Available leaves count too: another lazy value may not overwrite
    /// their color while the request still needs them. Passing a conservative
    /// union of all FP choices is valid (and stronger) when their liveness
    /// intervals already overlap, as in the current executable allocator.
    pub fn validate_roots(&self, roots: &[RefId]) -> Result<Vec<RefId>, Error> {
        let mut needed = BTreeSet::new();
        let mut pending = roots.to_vec();
        let mut colors = BTreeMap::new();
        while let Some(id) = pending.pop() {
            if !needed.insert(id) {
                continue;
            }
            let source = self
                .refs
                .get(id.0 as usize)
                .ok_or(Error::InvalidReference(id))?;
            if let Some(first) = colors.insert(source.color.local, id) {
                if first != id {
                    return Err(Error::ColorConflict {
                        color: source.color,
                        first,
                        second: id,
                    });
                }
            }
            pending.extend(source.op.dependencies());
        }
        Ok(needed.into_iter().collect())
    }
}

fn type_key(ty: ValueType) -> u64 {
    match ty {
        ValueType::I32 => 0,
        ValueType::I64 => 1,
        ValueType::F64 => 2,
    }
}

fn source_key(source: &SourceRef) -> Vec<u64> {
    if let SourceOp::Constant { bits } = source.op {
        // This source only writes these exact bits to this typed color. Unlike
        // Available/PhysicalInput/effect sources, its executable analysis ID
        // contributes no initialization or whole-value authority.
        return vec![
            2,
            u64::from(source.color.local),
            type_key(source.color.ty),
            bits,
        ];
    }
    let mut key = match source.key {
        ValueKey::Executable(id) => vec![0, id.0 as u64],
        ValueKey::Flags { origin, field } => match origin {
            FlagsOrigin::Entry(block) => vec![1, 0, block.0 as u64, u64::from(field)],
            FlagsOrigin::Effect(effect) => vec![1, 1, effect.0 as u64, u64::from(field)],
        },
    };
    key.extend([u64::from(source.color.local), type_key(source.color.ty)]);
    match &source.op {
        SourceOp::Available => key.push(0),
        SourceOp::PhysicalInput { local } => key.extend([1, u64::from(*local)]),
        SourceOp::Constant { bits } => key.extend([2, *bits]),
        SourceOp::Binary32 { kind, left, right } => key.extend([
            3,
            match kind {
                Binary32::Add => 0,
                Binary32::Sub => 1,
                Binary32::And => 2,
                Binary32::Or => 3,
                Binary32::Xor => 4,
            },
            u64::from(left.0),
            u64::from(right.0),
        ]),
        SourceOp::ShiftLeft32 { value, amount } => {
            key.extend([4, u64::from(value.0), u64::from(*amount)])
        }
        SourceOp::Fp(source) => {
            key.push(5);
            key.extend(source.key());
        }
    }
    key
}
