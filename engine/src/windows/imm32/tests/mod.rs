use super::*;

#[test]
fn default_context_is_shared_by_thread_while_explicit_association_and_detachment_are_window_owned()
{
    let mut model = InputContexts32::new(8, 8, 32).unwrap();
    let first = model.window_context(10, 1).unwrap();
    assert_eq!(model.window_context(11, 1), Ok(first));
    let other = model.window_context(12, 2).unwrap();
    assert_ne!(first, other);
    let custom = model.create_context(1).unwrap();
    assert_eq!(model.associate(10, 1, custom), Ok(first));
    assert_eq!(model.window_context(10, 1), Ok(custom));
    assert_eq!(model.window_context(11, 1), Ok(first));
    assert_eq!(
        model.destroy_context(custom, 1),
        Err(ImmError32::Associated)
    );
    assert_eq!(model.associate(10, 1, 0), Ok(custom));
    assert_eq!(model.window_context(10, 1), Ok(0));
    model.destroy_context(custom, 1).unwrap();
    assert_eq!(model.associate(10, 1, first), Ok(0));
    assert_eq!(model.associate(10, 1, other), Err(ImmError32::WrongThread));
    assert_eq!(model.window_context(10, 1), Ok(first));
}

#[test]
fn context_admission_and_composition_failures_preserve_live_identity_and_state() {
    let mut model = InputContexts32::new(2, 1, 4).unwrap();
    let custom = model.create_context(1).unwrap();
    let default = model.window_context(10, 1).unwrap();
    assert_eq!(model.create_context(1), Err(ImmError32::Capacity));
    assert_eq!(model.associate(10, 1, custom), Ok(default));
    assert_eq!(model.associate(11, 1, custom), Err(ImmError32::Capacity));
    assert_eq!(model.window_context(11, 1), Ok(default));
    let composition = InputComposition32 {
        text: vec![65, 66],
        attributes: vec![0, 1],
        clauses: vec![0, 2],
        cursor: 1,
        ..Default::default()
    };
    model
        .set_composition(custom, 1, composition.clone())
        .unwrap();
    let invalid = InputComposition32 {
        cursor: 3,
        ..composition.clone()
    };
    assert_eq!(
        model.set_composition(custom, 1, invalid),
        Err(ImmError32::InvalidParameter)
    );
    assert_eq!(model.context(custom).unwrap().composition, composition);
    assert_eq!(model.associate(10, 1, 0), Ok(custom));
    model.destroy_context(custom, 1).unwrap();
    let replacement = model.create_context(1).unwrap();
    assert_ne!(replacement, custom);
    assert_eq!(model.context(custom), Err(ImmError32::InvalidHandle));
}

#[test]
fn window_and_thread_retirement_drop_only_their_owned_associations_and_contexts() {
    let mut model = InputContexts32::new(8, 8, 32).unwrap();
    let default = model.window_context(10, 1).unwrap();
    let custom = model.create_context(1).unwrap();
    model.associate(10, 1, custom).unwrap();
    let other = model.window_context(11, 2).unwrap();
    model.retire_window(10);
    assert_eq!(model.window_context(12, 1), Ok(default));
    model.destroy_context(custom, 1).unwrap();
    model.retire_thread(1);
    assert_eq!(model.context(default), Err(ImmError32::InvalidHandle));
    assert_eq!(model.window_context(11, 2), Ok(other));
}
