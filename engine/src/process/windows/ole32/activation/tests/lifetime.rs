use super::*;

fn server(id: u64, path: &str) -> Server32 {
    Server32 {
        id,
        path: path.into(),
        handle: 0x500000,
        threading: ServerThreadingModel32::Both,
        threads: vec![1],
        mta_lease: false,
        neutral_lease: false,
        unused_deadline: None,
        activity: 1,
    }
}

#[test]
fn each_acquired_module_reference_retires_only_its_exact_server_identity() {
    let mut runtime = ActivationRuntime32::default();
    runtime.servers.push(server(1, "server"));
    runtime.servers.push(server(2, "server.dll"));
    let first = runtime.servers[0].lease();
    let second = runtime.servers[1].lease();
    runtime.release_server(first);
    assert_eq!(runtime.servers.len(), 1);
    assert_eq!(runtime.servers[0].id, 2);
    runtime.release_server(first);
    assert_eq!(runtime.servers[0].id, 2);
    runtime.release_server(ServerUse32 {
        handle: 0x600000,
        ..second
    });
    assert_eq!(runtime.servers[0].id, 2);
    runtime.release_server(second);
    assert!(runtime.servers.is_empty());
}

#[test]
fn can_unload_allows_reentrant_activation_but_actual_unload_keeps_it_parked() {
    let (mut engine, _) = pending(Ole32Api32::CoCreateInstance);
    let memory = engine.memory.as_mut().unwrap();
    let scratch =
        GuestCallbackScratch32::allocate(memory, ActivationRuntime32::scratch_reservation(), &[0])
            .unwrap();
    let runtime = &mut engine.ole32.as_mut().unwrap().activation;
    runtime.servers.push(server(1, "native.dll"));
    let lease = runtime.servers[0].lease();
    runtime.pending[0].phase = Phase32::Load;
    let frame = runtime.pending[0].frame;
    runtime.pending.push(PendingActivation32 {
        continuation: 2,
        thread: 2,
        owner: PendingOwner::Replacement(2),
        token: 2,
        frame,
        api: Ole32Api32::CoFreeUnusedLibrariesEx,
        out: 0,
        outer: 0,
        path: String::new(),
        scratch,
        phase: Phase32::CanUnload { lease, delay: 0 },
        threading: ServerThreadingModel32::Both,
        server_threads: Vec::new(),
        server: None,
    });
    assert!(runtime.load_ready(0));
    assert!(runtime.unload_reserved(lease.handle, 0));
    runtime.pending[1].phase = Phase32::Unloading { lease, delay: 0 };
    assert!(!runtime.load_ready(0));
    runtime.pending[1].phase = Phase32::UnloadNext { after: 0, delay: 0 };
    assert!(runtime.load_ready(0));
    runtime.pending[0].server = Some(1);
    assert!(runtime.server_held(lease.handle, 1));
    runtime.pending[0].server = None;
    assert!(!runtime.server_held(lease.handle, 1));
    runtime.pending[1].phase = Phase32::Loading;
    runtime.pending[1].path = "NATIVE.DLL".into();
    assert!(!runtime.load_ready(0));
}

#[test]
fn an_s_ok_from_before_reentrant_use_cannot_create_an_unload_candidate() {
    let (mut engine, scratch) = pending(Ole32Api32::CoFreeUnusedLibrariesEx);
    let runtime = &mut engine.ole32.as_mut().unwrap().activation;
    runtime.servers.push(server(1, "native.dll"));
    let lease = runtime.servers[0].lease();
    runtime.pending[0].phase = Phase32::CanUnload { lease, delay: 0 };
    runtime.servers[0].activity += 1;
    let Ole32CallbackOutcome32::Complete(result) = engine.resume_ole32_callback(1, 0).unwrap()
    else {
        panic!("old candidate is skipped")
    };
    assert_eq!(result, 0);
    let runtime = &engine.ole32.as_ref().unwrap().activation;
    assert_eq!(runtime.servers.len(), 1);
    assert_eq!(runtime.servers[0].activity, 2);
    assert_eq!(runtime.servers[0].unused_deadline, None);
    assert!(runtime.pending.is_empty());
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(scratch))
            .is_err()
    );
}

#[test]
fn shared_server_lease_outlives_its_creator_but_not_the_last_user_apartment() {
    let mut apartments = crate::windows::ole32::Apartments32::new(4).unwrap();
    apartments.initialize(1, 0, 0).unwrap();
    apartments.initialize(2, 0, 0).unwrap();
    let mut shared = server(1, "mta.dll");
    shared.mta_lease = true;
    assert!(!shared.closes_with(&apartments, 1));
    apartments.uninitialize(1);
    shared.threads.clear();
    assert!(shared.closes_with(&apartments, 2));
    // A canonical worker services calls but does not count as a remaining user.
    apartments.initialize(3, 0, 0).unwrap();
    apartments.mark_host(3);
    assert!(shared.closes_with(&apartments, 2));
    apartments.initialize(4, 0, 2).unwrap();
    assert!(shared.closes_with(&apartments, 2));
    shared.neutral_lease = true;
    assert!(!shared.closes_with(&apartments, 2));
    apartments.uninitialize(4);
    assert!(shared.closes_with(&apartments, 2));
}
