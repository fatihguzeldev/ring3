use wasm_encoder::{BlockType, Ieee64, InstructionSink, Lane, MemArg};

// The region emitter owns its enclosing labels before encoding bytes. Helpers
// reached through raw() are balanced fragments with no region return; keeping
// that escape explicit makes new opcodes fail to compile until forwarded here.
pub(in crate::cpu::core) struct Sink<'a> {
    code: InstructionSink<'a>,
    depth: &'a mut u32,
    redirect_returns: bool,
}

macro_rules! forward {
    ($($name:ident($($arg:ident: $ty:ty),*);)*) => {$ (
        #[inline]
        pub(in crate::cpu::core) fn $name(&mut self, $($arg: $ty),*) -> &mut Self {
            self.code.$name($($arg),*);
            self
        }
    )*};
}

impl<'a> Sink<'a> {
    pub(super) fn new(
        code: InstructionSink<'a>,
        depth: &'a mut u32,
        redirect_returns: bool,
    ) -> Self {
        Self {
            code,
            depth,
            redirect_returns,
        }
    }

    pub(super) fn raw(&mut self) -> &mut InstructionSink<'a> {
        &mut self.code
    }

    fn target(&self, label: u32) {
        assert!(
            label < *self.depth - u32::from(self.redirect_returns),
            "region branches target explicit controls"
        );
    }

    pub(in crate::cpu::core) fn block(&mut self, kind: BlockType) -> &mut Self {
        *self.depth += 1;
        self.code.block(kind);
        self
    }

    pub(in crate::cpu::core) fn loop_(&mut self, kind: BlockType) -> &mut Self {
        *self.depth += 1;
        self.code.loop_(kind);
        self
    }

    pub(in crate::cpu::core) fn if_(&mut self, kind: BlockType) -> &mut Self {
        *self.depth += 1;
        self.code.if_(kind);
        self
    }

    pub(in crate::cpu::core) fn else_(&mut self) -> &mut Self {
        assert!(*self.depth != 0);
        self.code.else_();
        self
    }

    pub(in crate::cpu::core) fn end(&mut self) -> &mut Self {
        *self.depth = self
            .depth
            .checked_sub(1)
            .expect("balanced emitted controls");
        self.code.end();
        self
    }

    pub(in crate::cpu::core) fn br(&mut self, label: u32) -> &mut Self {
        self.target(label);
        self.code.br(label);
        self
    }

    pub(in crate::cpu::core) fn br_if(&mut self, label: u32) -> &mut Self {
        self.target(label);
        self.code.br_if(label);
        self
    }

    pub(in crate::cpu::core) fn br_table<V>(&mut self, labels: V, default: u32) -> &mut Self
    where
        V: IntoIterator<Item = u32>,
        V::IntoIter: ExactSizeIterator,
    {
        self.target(default);
        let depth = *self.depth - u32::from(self.redirect_returns);
        self.code.br_table(
            labels.into_iter().inspect(|&label| assert!(label < depth)),
            default,
        );
        self
    }

    pub(in crate::cpu::core) fn return_(&mut self) -> &mut Self {
        if self.redirect_returns {
            self.code
                .local_set(super::exits::REASON)
                .br(self.depth.checked_sub(1).expect("region exit wrapper"));
        } else {
            // The sole neutral return precedes imported architectural state.
            assert_eq!(
                *self.depth, 2,
                "infrastructure guard inside the exit wrapper"
            );
            self.code.return_();
        }
        self
    }

    forward! {
        call(f: u32);
        f32_demote_f64();
        f32_reinterpret_i32();
        f32_store(m: MemArg);
        f32x4_add();
        f64_abs();
        f64_add();
        f64_const(x: Ieee64);
        f64_div();
        f64_eq();
        f64_gt();
        f64_lt();
        f64_mul();
        f64_ne();
        f64_neg();
        f64_promote_f32();
        f64_reinterpret_i64();
        f64_store(m: MemArg);
        f64_sub();
        f64x2_add();
        f64x2_ne();
        f64x2_promote_low_f32x4();
        global_get(g: u32);
        global_set(g: u32);
        i32_add();
        i32_and();
        i32_clz();
        i32_const(x: i32);
        i32_ctz();
        i32_eq();
        i32_eqz();
        i32_ge_u();
        i32_gt_s();
        i32_gt_u();
        i32_le_u();
        i32_load(m: MemArg);
        i32_load16_u(m: MemArg);
        i32_lt_s();
        i32_lt_u();
        i32_mul();
        i32_ne();
        i32_or();
        i32_popcnt();
        i32_reinterpret_f32();
        i32_rem_u();
        i32_rotl();
        i32_rotr();
        i32_shl();
        i32_shr_s();
        i32_shr_u();
        i32_store(m: MemArg);
        i32_store16(m: MemArg);
        i32_sub();
        i32_wrap_i64();
        i32_xor();
        i32x4_add();
        i32x4_all_true();
        i32x4_eq();
        i32x4_le_u();
        i32x4_shr_u();
        i32x4_splat();
        i32x4_sub();
        i64_add();
        i64_and();
        i64_const(x: i64);
        i64_div_s();
        i64_div_u();
        i64_eq();
        i64_eqz();
        i64_extend_i32_s();
        i64_extend_i32_u();
        i64_gt_s();
        i64_gt_u();
        i64_load(m: MemArg);
        i64_lt_s();
        i64_mul();
        i64_ne();
        i64_or();
        i64_reinterpret_f64();
        i64_rem_s();
        i64_rem_u();
        i64_shl();
        i64_shr_u();
        i64_store(m: MemArg);
        i64_sub();
        i64_xor();
        i8x16_shuffle(lanes: [Lane; 16]);
        local_get(l: u32);
        local_set(l: u32);
        local_tee(l: u32);
        select();
        unreachable();
        v128_and();
        v128_any_true();
        v128_load(memarg: MemArg);
        v128_or();
        v128_store(memarg: MemArg);
    }
}
