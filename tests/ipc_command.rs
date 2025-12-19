use bincode;
use nexa::ipc::control::Command;

#[test]
fn command_roundtrip() {
    let cmd = Command::Play;

    let bytes = bincode::serialize(&cmd).unwrap();
    let decoded: Command = bincode::deserialize(&bytes).unwrap();

    assert_eq!(cmd, decoded);
}
