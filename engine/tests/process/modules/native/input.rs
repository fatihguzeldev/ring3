use super::fixtures::{EXE_BASE, image};
use ring3_engine::process::{EngineInstance, ModuleOperation32, ModuleRuntimeConfig32};

#[test]
fn staged_module_source_requires_exact_order_and_failure_retains_owned_input() {
    let mut engine = EngineInstance::new(64, 7).unwrap();
    let mut bytes = image(EXE_BASE, false, true);
    let initial = engine.arena().to_vec();
    engine
        .begin_module_input(bytes.len() as u32, "MAIN.EXE")
        .unwrap();
    assert!(engine.append_module_input(1, &bytes[..512]).is_err());
    engine.append_module_input(0, &bytes[..512]).unwrap();
    assert!(engine.append_module_input(0, &bytes[512..]).is_err());
    assert!(engine.commit_module_input().is_err());
    engine.append_module_input(512, &bytes[512..]).unwrap();
    bytes.fill(0xa5);
    engine.commit_module_input().unwrap();
    assert_eq!(engine.arena(), initial);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    assert!(engine.commit_module_input().is_err());
    assert_eq!(
        engine
            .start_module_process("main.exe", None, ModuleRuntimeConfig32::default())
            .unwrap(),
        ModuleOperation32::Loaded { handle: EXE_BASE }
    );
}

#[test]
fn malformed_commit_retains_input_until_explicit_abort_and_valid_retry() {
    let mut engine = EngineInstance::new(64, 7).unwrap();
    engine.begin_module_input(4, "bad.dll").unwrap();
    engine.append_module_input(0, b"bad!").unwrap();
    let before = engine.arena().to_vec();
    let error = engine.commit_module_input().unwrap_err();
    assert_eq!(engine.commit_module_input(), Err(error));
    assert!(engine.begin_module_input(4, "other.dll").is_err());
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    engine.abort_module_input().unwrap();
    let bytes = image(EXE_BASE, false, true);
    engine
        .begin_module_input(bytes.len() as u32, "main.exe")
        .unwrap();
    engine.append_module_input(0, &bytes).unwrap();
    engine.commit_module_input().unwrap();
}

#[test]
fn source_budget_is_configured_before_registration_and_applies_to_staging() {
    use ring3_engine::loader::ModuleLimits32;
    let mut engine = EngineInstance::new(64, 7).unwrap();
    let bytes = image(EXE_BASE, false, true);
    let limits = ModuleLimits32 {
        source_bytes: bytes.len(),
        ..ModuleLimits32::default()
    };
    engine.configure_module_limits(limits).unwrap();
    engine.register_guest_module("main.exe", &bytes).unwrap();
    let before = engine.arena().to_vec();
    assert!(engine.begin_module_input(1, "more.dll").is_err());
    assert!(
        engine
            .configure_module_limits(ModuleLimits32::default())
            .is_err()
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(
        engine
            .start_module_process("main.exe", None, ModuleRuntimeConfig32::default())
            .unwrap(),
        ModuleOperation32::Loaded { handle: EXE_BASE }
    );
}
