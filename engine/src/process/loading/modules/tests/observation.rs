mod wire;

use super::paths::dll;
use crate::{
    abi::{
        arena::EXIT_OFFSET,
        x86::{decode_state, encode_exit_v3, encode_state_current},
    },
    cpu::{ExecutionExit, ExitReason},
    process::{EngineInstance, ModuleInitializer32, ModuleOperation32},
};
use ring3_observability::{CategorySelection, Config, Level, Observer, categories};
use wire::{Value, events};

fn observer() -> Observer {
    Observer::new(Config {
        level: Level::Basic,
        categories: CategorySelection::Only(vec![categories::ENGINE_LOADER_MODULES]),
        ..Config::default()
    })
    .unwrap()
}

fn started(observer: &Observer) -> EngineInstance {
    let mut engine = EngineInstance::new(256, 7).unwrap();
    engine.set_observer(observer.clone()).unwrap();
    engine
        .register_guest_module(
            "main.exe",
            &crate::process::windows::process::test_image_for_startup(),
        )
        .unwrap();
    assert_eq!(
        engine
            .start_module_process("main.exe", None, Default::default())
            .unwrap(),
        ModuleOperation32::Loaded { handle: 0x400000 }
    );
    engine
}

fn return_initializer(
    engine: &mut EngineInstance,
    initializer: ModuleInitializer32,
    value: u32,
) -> ModuleOperation32 {
    let mut state = decode_state(&engine.arena()[..56]).unwrap();
    state.eip = initializer.return_pc;
    state.registers[4] = initializer.return_esp;
    state.registers[0] = value;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Gate {
                id: initializer.return_id,
            },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + 40],
    )
    .unwrap();
    engine.finish_module_initializer(initializer.token).unwrap()
}

#[test]
fn committed_mapping_observes_actual_metadata_once_and_distinguishes_reload() {
    let observer = observer();
    let mut engine = started(&observer);
    let trace = observer.drain();
    let mapped = events(&trace, "module.mapped");
    assert_eq!(mapped.len(), 1);
    assert_eq!(mapped[0]["module"], Value::Text("main.exe".into()));
    assert_eq!(mapped[0]["base"], Value::Number(0x400000));
    let main = engine.loaded_windows_modules().next().unwrap();
    assert_eq!(mapped[0]["module_id"], Value::Number(main.id().get()));
    assert_eq!(
        mapped[0]["image_size"],
        Value::Number(u64::from(main.image_size()))
    );
    assert_eq!(mapped[0]["is_system"], Value::Number(0));

    engine
        .register_guest_module("plugin.dll", &dll(0x600000))
        .unwrap();
    assert_eq!(
        engine.load_library_module("plugin.dll", None).unwrap(),
        ModuleOperation32::Loaded { handle: 0x600000 }
    );
    let trace = observer.drain();
    let mapped = events(&trace, "module.mapped");
    assert_eq!(mapped.len(), 1);
    let first_id = mapped[0]["module_id"].number();
    assert_eq!(mapped[0]["module"], Value::Text("plugin.dll".into()));
    assert_eq!(mapped[0]["base"], Value::Number(0x600000));
    let plugin = engine
        .loaded_windows_modules()
        .find(|module| module.base() == 0x600000)
        .unwrap();
    assert_eq!(
        mapped[0]["image_size"],
        Value::Number(u64::from(plugin.image_size()))
    );

    assert_eq!(
        engine.load_library_module("plugin.dll", None).unwrap(),
        ModuleOperation32::Loaded { handle: 0x600000 }
    );
    engine.free_library_module(0x600000).unwrap();
    let trace = observer.drain();
    assert!(events(&trace, "module.mapped").is_empty());
    assert!(events(&trace, "module.unmapped").is_empty());
    engine.free_library_module(0x600000).unwrap();
    let trace = observer.drain();
    let unmapped = events(&trace, "module.unmapped");
    assert_eq!(unmapped.len(), 1);
    assert_eq!(unmapped[0]["module_id"], Value::Number(first_id));
    assert_eq!(engine.module_handle(Some("plugin.dll")).unwrap(), None);

    engine.load_library_module("plugin.dll", None).unwrap();
    let trace = observer.drain();
    let mapped = events(&trace, "module.mapped");
    assert_eq!(mapped.len(), 1);
    assert!(mapped[0]["module_id"].number() > first_id);
    assert_eq!(mapped[0]["base"], Value::Number(0x600000));
    assert_eq!(observer.stats().dropped_events, 0);
}

#[test]
fn failed_staging_emits_no_mapping_and_initializer_rollback_closes_the_real_mapping() {
    let observer = observer();
    let mut engine = started(&observer);
    observer.drain();
    engine
        .register_guest_module("conflict.dll", &dll(0x400000))
        .unwrap();
    assert!(engine.load_library_module("conflict.dll", None).is_err());
    let trace = observer.drain();
    assert!(events(&trace, "module.mapped").is_empty());
    assert!(events(&trace, "module.unmapped").is_empty());

    let mut bytes = dll(0x600000);
    bytes[0xa8..0xac].copy_from_slice(&0x1000u32.to_le_bytes());
    engine.register_guest_module("failed.dll", &bytes).unwrap();
    let mut operation = engine.load_library_module("failed.dll", None).unwrap();
    let trace = observer.drain();
    let mapped = events(&trace, "module.mapped");
    assert_eq!(mapped.len(), 1);
    assert_eq!(mapped[0]["module"], Value::Text("failed.dll".into()));
    let id = mapped[0]["module_id"].number();
    assert!(matches!(operation, ModuleOperation32::Pending(_)));
    for index in 0..4 {
        let ModuleOperation32::Pending(initializer) = operation else {
            break;
        };
        operation = return_initializer(&mut engine, initializer, u32::from(index != 0));
    }
    assert_eq!(operation, ModuleOperation32::InitializationFailed);
    assert_eq!(engine.module_handle(Some("failed.dll")).unwrap(), None);
    let trace = observer.drain();
    let unmapped = events(&trace, "module.unmapped");
    assert_eq!(unmapped.len(), 1);
    assert_eq!(unmapped[0]["module_id"], Value::Number(id));
    assert!(events(&trace, "module.mapped").is_empty());
    assert_eq!(observer.stats().dropped_events, 0);
}

#[test]
fn mapping_uses_its_instance_observer_and_respects_category_selection() {
    let outer = observer();
    let _capture = outer.enter();
    for selected in [
        Observer::disabled(),
        Observer::new(Config {
            level: Level::Basic,
            categories: CategorySelection::Only(vec![categories::ENGINE_CPU]),
            ..Config::default()
        })
        .unwrap(),
    ] {
        let before = outer.stats().emitted_events;
        let mut engine = started(&selected);
        engine
            .register_guest_module("plugin.dll", &dll(0x600000))
            .unwrap();
        engine.load_library_module("plugin.dll", None).unwrap();
        engine.free_library_module(0x600000).unwrap();
        let trace = selected.drain();
        assert!(events(&trace, "module.mapped").is_empty());
        assert!(events(&trace, "module.unmapped").is_empty());
        assert_eq!(outer.stats().emitted_events, before);
    }
}
