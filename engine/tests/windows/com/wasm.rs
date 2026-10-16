#[path = "../../support/wasm.rs"]
mod support_wasm;

use std::{fs, path::Path, time::SystemTime};

#[test]
fn authored_freestanding_server_executes_registry_com_factory_lifetime_and_unload() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = support_wasm::selected_engine(root);
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("select the exact engine SHA-256");
    let output = root.join("target/windows-com-wasm").join(format!(
        "run-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let result = support_wasm::run_node(
        root,
        "engine/tests/windows/com/fixtures/runtime/run.mjs",
        Path::new(&engine),
        &output,
        Some(&expected),
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "authored COM Wasm integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}

fn run_apartment_fixture(variant: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = support_wasm::selected_engine(root);
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("select the exact engine SHA-256");
    let output = root.join("target/windows-com-wasm").join(format!(
        "apartments-{variant}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let result = std::process::Command::new("node")
        .arg(root.join("engine/tests/windows/com/fixtures/runtime/apartments.mjs"))
        .arg(engine)
        .arg(&output)
        .arg(root)
        .env("RING3_ENGINE_SHA256", expected)
        .env(
            "RING3_COM_APARTMENTS_ORPHAN",
            if variant == "orphan" { "1" } else { "0" },
        )
        .env(
            "RING3_COM_APARTMENTS_RUNDOWN",
            if variant == "rundown" { "1" } else { "0" },
        )
        .output()
        .expect("run authored apartment fixture with node");
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "authored {variant} apartment Wasm integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}

#[test]
fn authored_standard_com_proxies_dispatch_in_sta_mta_and_neutral_apartments() {
    run_apartment_fixture("basic");
}

#[test]
fn authored_com_receiver_releases_an_actual_result_after_sender_thread_termination() {
    run_apartment_fixture("orphan");
}

#[test]
fn authored_final_couninitialize_runs_owner_release_before_unload_and_return() {
    run_apartment_fixture("rundown");
}

fn run_marshaling_fixture(profile: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = support_wasm::selected_engine(root);
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("select the exact engine SHA-256");
    let output = root.join("target/windows-com-wasm").join(format!(
        "marshaling-{profile}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let mut command = std::process::Command::new("node");
    if profile == "stream-copy" {
        let producer = std::env::var("RING3_COM_STREAM_COPY_FIXTURE_DIR")
            .expect("select the source-built application stream CopyTo fixture");
        command
            .env("RING3_COM_MARSHAL_FIXTURE_DIR", producer)
            .env("RING3_COM_REQUIRE_STREAM_COPY", "1");
    }
    let result = command
        .arg(root.join("engine/tests/windows/com/fixtures/runtime/marshaling.mjs"))
        .arg(engine)
        .arg(&output)
        .arg(root)
        .env("RING3_ENGINE_SHA256", expected)
        .env(
            "RING3_COM_MARSHAL_PROFILE",
            if profile == "stream-copy" {
                "custom"
            } else {
                profile
            },
        )
        .output()
        .expect("run authored marshaling fixture with node");
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "authored {profile} COM marshaling failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}

#[test]
fn authored_custom_imarshal_transfers_opaque_state_with_real_guest_stream_methods() {
    run_marshaling_fixture("custom");
}

#[test]
fn authored_registered_proxy_stub_uses_real_guest_rpc_channels_and_balanced_native_lifetimes() {
    run_marshaling_fixture("rpc");
}

#[test]
fn authored_stream_copyto_executes_application_write_with_partial_results_and_balanced_lifetime() {
    run_marshaling_fixture("stream-copy");
}
