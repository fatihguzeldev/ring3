use ring3_observability::{
    CATALOG_VERSION, CATEGORIES, Category, CategorySelection, Config, ConfigError, Level, Observer,
    categories,
};

#[test]
fn catalog_discovery_and_typed_constants_share_the_same_known_domain() {
    assert_eq!(CATALOG_VERSION, 1);
    for descriptor in CATEGORIES {
        let category = Category::from_id(descriptor.id).unwrap();
        assert_eq!(category.descriptor().id, descriptor.id);
        assert!(!descriptor.label.is_empty());
        assert!(!descriptor.description.is_empty());
        if let Some(parent) = descriptor.parent {
            assert!(Category::from_id(parent).is_some());
        }
    }
    assert_eq!(
        Category::from_id("engine.cpu.decode"),
        Some(categories::ENGINE_CPU_DECODE)
    );
    assert!(Category::from_id("engine.cpus").is_none());
}

#[test]
fn all_none_and_subtree_selections_have_distinct_wire_meanings() {
    assert_eq!(CategorySelection::from_wire(""), Ok(CategorySelection::All));
    let none = CategorySelection::from_wire("-").unwrap();
    assert_eq!(none, CategorySelection::Only(Vec::new()));
    assert!(!none.allows(categories::ENGINE_CPU));
    let selected = CategorySelection::from_wire("engine.cpu\nhost.files").unwrap();
    assert!(selected.allows(categories::ENGINE_CPU));
    assert!(selected.allows(categories::ENGINE_CPU_DECODE));
    assert!(selected.allows(categories::HOST_FILES));
    assert!(!selected.allows(categories::ENGINE_LOADER));
    assert!(!selected.allows(categories::HOST_GRAPHICS));
}

#[test]
fn unknown_internal_or_malformed_wire_selections_fail_closed() {
    for input in [
        "engine.cpus",
        "engine.cpu\n",
        "\nengine.cpu",
        " engine.cpu",
        "-\nengine.cpu",
        "observability",
        "engine.cpu\nengine.cpu",
    ] {
        assert_eq!(
            CategorySelection::from_wire(input),
            Err(ConfigError::Categories),
            "input {input:?}"
        );
    }
    assert!(
        CategorySelection::from_wire(&vec!["engine"; CATEGORIES.len() + 1].join("\n")).is_err()
    );
    assert!(
        Observer::new(Config {
            categories: CategorySelection::Only(vec![
                categories::ENGINE_CPU,
                categories::ENGINE_CPU
            ]),
            ..Config::default()
        })
        .is_err()
    );
}

#[test]
fn only_empty_keeps_control_lifecycle_but_emits_no_selected_component_events() {
    let observer = Observer::new(Config {
        level: Level::Detailed,
        categories: CategorySelection::Only(Vec::new()),
        ..Config::default()
    })
    .unwrap();
    assert!(CategorySelection::Only(Vec::new()).allows(categories::OBSERVABILITY));
    let before = observer.stats().emitted_events;
    observer.instant(Level::Basic, categories::ENGINE_LOADER, "load", &[]);
    observer.counter(Level::Basic, categories::ENGINE_CPU_CACHE, "hits", 1.0);
    drop(observer.scope(
        Level::Detailed,
        categories::ENGINE_CPU_DECODE,
        "decode",
        &[],
    ));
    assert_eq!(observer.stats().emitted_events, before);
    let trace = observer.finish();
    for name in ["capture.started", "capture.finished"] {
        assert!(
            trace
                .windows(name.len())
                .any(|bytes| bytes == name.as_bytes())
        );
    }
    assert_eq!(observer.stats().emitted_events, before + 1);
}
