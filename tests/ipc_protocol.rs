use nexa::ipc::version::PROTOCOL_VERSION;

#[test]
fn protocol_version_is_nonzero() {
    assert!(PROTOCOL_VERSION > 0);
}
