use nexa::ipc::control::Command;

#[test]
fn command_roundtrip() {
    let cmd = Command::Play;

    let bytes = postcard::to_stdvec(&cmd).expect("Failed to serialize Command");
    let decoded: Command = postcard::from_bytes(&bytes).expect("Failed to deserialize Command");

    assert_eq!(cmd, decoded);
}
