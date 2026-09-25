#[path = "../tests/support/fixture.rs"]
mod fixture;

use super::process::Session;
use serde_json::{Value, json};

fn configured() -> Session {
    let mut session = Session::default();
    let executable = fixture::executable();
    session
        .command(
            0,
            0,
            json!({"files":[
                {"path":"C:\\sample.exe","size":executable.len(),"role":"executable"},
                {"path":"C:\\a.bin","size":4,"role":"data"}
            ]})
            .to_string()
            .into_bytes(),
        )
        .unwrap();
    session.command(1, 0, executable).unwrap();
    session
}

fn run(session: &mut Session, budget: u32) -> Value {
    session
        .command(3, budget, br#"{"elapsedMs":100}"#.to_vec())
        .unwrap()
}

#[test]
fn guest_file_request_rejects_wrong_token_and_size_then_resumes_exactly() {
    let mut results = Vec::new();
    for budget in [1, 5, 100_000] {
        let mut session = configured();
        session.command(2, 0, Vec::new()).unwrap();
        let request = loop {
            let result = run(&mut session, budget);
            if result["state"] == "file" {
                break result;
            }
        };
        assert_eq!(request["pending"]["path"], "C:\\a.bin");
        let id = request["pending"]["id"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        for bytes in [
            (id + 1).to_le_bytes().into_iter().chain(*b"data").collect(),
            id.to_le_bytes().to_vec(),
        ] {
            assert!(session.command(4, 0, bytes).is_err());
            assert_eq!(session.command(6, 0, Vec::new()).unwrap(), request);
        }
        session
            .command(4, 0, id.to_le_bytes().into_iter().chain(*b"data").collect())
            .unwrap();
        let result = loop {
            let result = run(&mut session, budget);
            if result["state"] != "running" {
                break result;
            }
        };
        assert_eq!(result["reason"], "Some(Exited(42))");
        results.push(result);
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn failed_control_operations_preserve_a_loaded_process() {
    let mut session = configured();
    let snapshot = session.command(2, 0, Vec::new()).unwrap();
    for (operation, argument, input) in [
        (0, 0, b"{}".to_vec()),
        (1, 0, Vec::new()),
        (2, 0, Vec::new()),
        (3, 0, br#"{"elapsedMs":0}"#.to_vec()),
        (3, 100_001, br#"{"elapsedMs":0}"#.to_vec()),
        (3, 1, br#"{"elapsedMs":-1}"#.to_vec()),
        (5, 0, b"{}".to_vec()),
        (100, 0, Vec::new()),
    ] {
        assert!(session.command(operation, argument, input).is_err());
        assert_eq!(session.command(6, 0, Vec::new()).unwrap(), snapshot);
    }
}

#[test]
fn invalid_reconfiguration_and_contents_do_not_replace_valid_inputs() {
    let mut session = configured();
    assert!(session.command(0, 0, b"{}".to_vec()).is_err());
    assert!(session.command(1, 0, vec![0]).is_err());
    assert!(session.command(1, 1, b"data".to_vec()).is_err());
    assert!(session.command(1, u32::MAX, Vec::new()).is_err());
    assert_eq!(
        session.command(2, 0, Vec::new()).unwrap()["state"],
        "running"
    );
    assert_eq!(super::ring3_input(u32::MAX), 0);
}
