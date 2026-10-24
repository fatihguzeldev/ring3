use super::*;
use crate::cpu::{
    core::{
        emitter::executable::locals::Layout,
        program as decoded,
        region::{
            BlockId, Region,
            values::executable::{ExecutableProgram, ValueNode},
        },
    },
    x86::{
        Register32, X87Format, X87Operation,
        ir::{EffectiveAddress, Operation},
    },
};
use std::collections::BTreeSet;

fn executable(operations: &[Operation]) -> (RegionPlan, ExecutableProgram) {
    let block = decoded::Block {
        entry: 0x1000,
        instructions: operations
            .iter()
            .copied()
            .enumerate()
            .map(|(index, operation)| decoded::Instruction {
                pc: 0x1000 + index as u32,
                next: 0x1001 + index as u32,
                operation: Some(operation),
                gate: None,
            })
            .collect(),
    };
    let region = Region::from_blocks(&[block], 0x1000).unwrap();
    let values = RegionPlan::analyze(&region);
    let executable = ExecutableProgram::analyze(&region, &values);
    (values, executable)
}

fn load() -> Operation {
    Operation::X87 {
        operation: X87Operation::Load(X87Format::Float32),
        address: Some(EffectiveAddress {
            base: Some(Register32::Ebx),
            ..Default::default()
        }),
        opcode: 0x103,
        waiting: true,
    }
}

#[test]
fn lazy_record_sources_include_the_whole_top_dependency() {
    let (_, executable) = executable(&[load()]);
    let layout = Layout::build(&executable, 1, true, 179);
    let body = executable.block(BlockId(0));
    let available = BTreeSet::new();
    let context = SourceContext {
        executable: &executable,
        layout: &layout,
        available: &available,
        physical_gpr: Some(body.entry.gpr),
        physical_flags: Some(body.entry.flags),
        active_effect: None,
        captured_effect: None,
    };
    let FpPayload::Tagged {
        low,
        valid,
        certificate,
        ..
    } = body.entry.fp.payload[0]
    else {
        panic!("unknown entry");
    };
    let mut program = Program::new(128);
    let low = program.value(low, &context).unwrap();
    let SourceOp::Fp(Source::Input {
        field: FpInput::Low(0),
        top: Some(top),
    }) = program.get(low).op
    else {
        panic!("record input");
    };
    assert!(top < low);
    assert_eq!(program.get(low).op.dependencies(), vec![top]);
    assert!(matches!(
        program.get(top).op,
        SourceOp::Fp(Source::Input {
            field: FpInput::Top,
            top: None
        })
    ));
    let valid = program.value(valid, &context).unwrap();
    let certificate = program.value(certificate, &context).unwrap();
    assert!(program.get(certificate).op.dependencies().contains(&top));
    assert!(program.get(certificate).op.dependencies().contains(&valid));
    assert!(
        program.reverse_refs().all(|(id, source)| source
            .op
            .dependencies()
            .iter()
            .all(|&dep| dep < id))
    );
}

#[test]
fn dirty_and_valid_discriminators_do_not_select_untouched_ghost_payloads() {
    let (values, executable) = executable(&[load()]);
    let layout = Layout::build(&executable, 1, true, 179);
    let body = executable.block(BlockId(0));
    let available = BTreeSet::new();
    let context = SourceContext {
        executable: &executable,
        layout: &layout,
        available: &available,
        physical_gpr: Some(body.entry.gpr),
        physical_flags: Some(body.entry.flags),
        active_effect: None,
        captured_effect: None,
    };
    let mut program = Program::new(256);
    let projection = Projection::build(
        &mut program,
        &context,
        &body.entry.fp,
        body.entry.frame,
        &values,
    )
    .unwrap();
    let header: Vec<_> = projection.header.iter().map(|&(_, id)| id).collect();
    assert!(projection.discriminators(0).is_empty());
    assert_eq!(
        projection.selected(0, |_| panic!("no ghost discriminator")),
        header
    );
    let first = &projection.cells[0];
    let Payload::Tagged {
        valid,
        native,
        low,
        high,
    } = first.payload
    else {
        panic!("tagged input");
    };
    let dirty = first.aliases;
    assert_eq!(projection.discriminators(dirty), vec![valid]);
    let raw = projection.selected(dirty, |_| false);
    assert!(raw.contains(&low) && raw.contains(&high));
    assert!(!raw.contains(&native));
    let cached = projection.selected(dirty, |_| true);
    assert!(cached.contains(&native));
    assert!(!cached.contains(&low) && !cached.contains(&high));
    for cell in projection.cells.iter().skip(1) {
        for source in cell.payload.roots() {
            assert!(!raw.contains(&source) && !cached.contains(&source));
        }
    }
}

#[test]
fn a_rounded_or_loaded_value_is_captured_and_never_replayed_by_recovery() {
    let (values, executable) = executable(&[load()]);
    let layout = Layout::build(&executable, 1, true, 179);
    let body = executable.block(BlockId(0));
    let effect = body.effects().next().unwrap();
    let FpPayload::Native { value, .. } = effect.after.fp.payload[0] else {
        panic!("typed result");
    };
    let empty = BTreeSet::new();
    let mut context = SourceContext {
        executable: &executable,
        layout: &layout,
        available: &empty,
        physical_gpr: Some(effect.after.gpr),
        physical_flags: Some(effect.after.flags),
        active_effect: Some(effect.id),
        captured_effect: Some(effect.id),
    };
    let mut program = Program::new(256);
    assert_eq!(
        program.value(value, &context),
        Err(Error::UncapturedEffect(value))
    );
    assert!(program.refs().is_empty());
    let available: BTreeSet<_> = effect.definitions().into_iter().collect();
    context.available = &available;
    let projection = Projection::build(
        &mut program,
        &context,
        &effect.after.fp,
        effect.after.frame,
        &values,
    )
    .unwrap();
    let Payload::Native { value: result } = projection
        .cells
        .iter()
        .find(|cell| cell.aliases & 1 != 0)
        .unwrap()
        .payload
    else {
        panic!("native projection");
    };
    assert!(matches!(program.get(result).op, SourceOp::Available));
    assert_eq!(
        projection.certificates.count_ones(),
        5,
        "one full native24 certificate at publication"
    );
    assert_eq!(projection.dirty(0), effect.after.fp.written);
    let before = program.refs().len();
    assert_eq!(
        Projection::build(
            &mut program,
            &context,
            &effect.after.fp,
            effect.after.frame,
            &values
        )
        .unwrap(),
        projection
    );
    assert_eq!(
        program.refs().len(),
        before,
        "another guard at the same state reuses every source"
    );
}

#[test]
fn a_fresh_same_block_barrier_input_has_no_snapshot_certificate() {
    let (_, executable) = executable(&[Operation::InitializeX87]);
    let layout = Layout::build(&executable, 1, true, 179);
    let body = executable.block(BlockId(0));
    let effect = body.effects().next().unwrap();
    let FpPayload::Tagged { certificate, .. } = effect.after.fp.payload[0] else {
        panic!("fresh barrier input");
    };
    assert!(
        matches!(executable.value(certificate).node, ValueNode::Fp(FpValueNode::InputCertificate { point, .. }) if point.prefix == 1)
    );
    let available = BTreeSet::new();
    let context = SourceContext {
        executable: &executable,
        layout: &layout,
        available: &available,
        physical_gpr: Some(effect.after.gpr),
        physical_flags: Some(effect.after.flags),
        active_effect: Some(effect.id),
        captured_effect: Some(effect.id),
    };
    let mut program = Program::new(8);
    let certificate = program.value(certificate, &context).unwrap();
    assert!(matches!(
        program.get(certificate).op,
        SourceOp::Constant { bits: 0 }
    ));
    assert_eq!(program.refs().len(), 1);
}

#[test]
fn top_rotation_and_alias_selection_are_exact() {
    let header = [(87, RefId(20)); 12];
    let projection = Projection {
        frame: FrameStateId(0),
        top_delta: -1,
        written: 2,
        certificates: 0,
        cells: vec![Cell {
            cell: 0,
            aliases: 0b101,
            payload: Payload::Native { value: RefId(1) },
        }],
        header,
    };
    assert_eq!(projection.dirty(1), 0b10);
    assert_eq!(projection.dirty(0x80), 0b11);
    assert_eq!(
        projection
            .selected(4, |_| false)
            .iter()
            .filter(|&&id| id == RefId(1))
            .count(),
        1
    );
    assert!(
        projection
            .selected(2, |_| false)
            .iter()
            .all(|&id| id == RefId(20))
    );
}
