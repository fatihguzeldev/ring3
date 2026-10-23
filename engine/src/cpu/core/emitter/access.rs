use super::{ADDRESS, CTX, FULL_BLOCK, GP_FACTS_VALID, LOGICAL_ADDRESS, Output};
use crate::cpu::{
    core::memory::groups,
    x86::{
        Register32, SegmentRegister,
        ir::{AddressSize, EffectiveAddress},
    },
};
use wasm_encoder::BlockType;

struct Access {
    slot: u8,
    offset: u32,
    span: u32,
    rights: u32,
    logical_max: u32,
    alignment_mask: u32,
    alignment_residue: u32,
}

impl Output<'_> {
    // Success prepares the ordinary memory port's SLOT and POINTER. Admission
    // observes no guest data, raises no fault, and performs no write bookkeeping.
    pub(super) fn group_geometry(
        &mut self,
        address: EffectiveAddress,
        width: u32,
        write: bool,
    ) -> bool {
        let Some(access) = self.access(address, width, write) else {
            return false;
        };
        let bit = 1_i32 << access.slot;
        self.c()
            .local_get(GP_FACTS_VALID)
            .local_get(FULL_BLOCK)
            .i32_and()
            .if_(BlockType::Result(wasm_encoder::ValType::I32));
        self.c()
            .local_get(groups::TRIED)
            .i32_const(bit)
            .i32_and()
            .i32_eqz()
            .if_(BlockType::Empty);
        self.c()
            .local_get(groups::TRIED)
            .i32_const(bit)
            .i32_or()
            .local_set(groups::TRIED);
        for local in [CTX, LOGICAL_ADDRESS, ADDRESS] {
            self.c().local_get(local);
        }
        for value in [
            access.offset,
            access.span,
            access.rights,
            access.logical_max,
            access.alignment_mask,
            access.alignment_residue,
        ] {
            self.c().i32_const(value as i32);
        }
        let acquire = self.internal(11);
        self.c()
            .call(acquire)
            .i32_const(i32::from(access.slot))
            .i32_shl()
            .local_get(groups::VALID)
            .i32_or()
            .local_set(groups::VALID)
            .local_set(groups::anchor(access.slot))
            .local_set(groups::physical_slot(access.slot))
            .end();
        self.c()
            .local_get(groups::VALID)
            .i32_const(bit)
            .i32_and()
            .if_(BlockType::Result(wasm_encoder::ValType::I32))
            .local_get(groups::physical_slot(access.slot))
            .local_set(33)
            .local_get(groups::anchor(access.slot))
            .i32_const(access.offset as i32)
            .i32_add()
            .local_set(34)
            .local_get(groups::VALID)
            .i32_const(i32::from(access.slot))
            .i32_shr_u()
            .i32_const(groups::GEOMETRY | groups::PLAIN_WRITE)
            .i32_and()
            .else_()
            .i32_const(0)
            .end()
            .else_()
            .i32_const(0)
            .end();
        true
    }

    fn access(&self, address: EffectiveAddress, width: u32, write: bool) -> Option<Access> {
        // Quota tails and arbitrary interior ports retain the one precise access
        // path; the descriptor specialization belongs to ordinary full blocks.
        if !self.grouped_accesses {
            return None;
        }
        let block = self.current_block?;
        let member = self
            .plan
            .access_member(block, self.prefix, address, width, write)?;
        let group = self
            .plan
            .access_groups(block)
            .get(usize::from(member.group))?;
        if !group.alignment_compatible {
            return None;
        }
        Some(Access {
            slot: group.slot,
            offset: u32::try_from(member.delta - group.min_delta).ok()?,
            span: u32::try_from(group.max_end - group.min_delta).ok()?,
            rights: u32::from(group.permissions),
            logical_max: if address.address_size == AddressSize::Bits16 {
                0xffff
            } else {
                u32::MAX
            },
            alignment_mask: group.alignment_mask,
            alignment_residue: group.alignment_residue,
        })
    }

    pub(super) fn current_group_geometry(&mut self, width: u32, write: bool) -> bool {
        let Some(address) = self
            .stack_access
            .or(self.address_fact.map(|fact| fact.address))
        else {
            return false;
        };
        self.group_geometry(address, width, write)
    }

    pub(super) fn stack_address(&mut self, base: Register32, displacement: u32) {
        // the same real EA feeds group membership and ordinary precise access.
        // SS is flat on this port, matching the former raw stack arithmetic.
        let address = EffectiveAddress {
            base: Some(base),
            displacement,
            segment: Some(SegmentRegister::Ss),
            ..Default::default()
        };
        self.address_fact = None;
        self.stack_access = Some(address);
        self.c().local_get(base.index() as u32 + 2);
        if displacement != 0 {
            self.c().i32_const(displacement as i32).i32_add();
        }
        self.c().local_tee(ADDRESS).local_set(LOGICAL_ADDRESS);
    }
}
