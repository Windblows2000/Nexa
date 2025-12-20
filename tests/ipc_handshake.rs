use nexa::ipc::handshake::Handshake;
use nexa::ipc::version::PROTOCOL_VERSION;

#[test]
fn handshake_roundtrip() {
    let hello = Handshake {
        protocol_version: PROTOCOL_VERSION,
        client_name: "test".into(),
        requested_features: vec![],
    };

    let bytes = bincode::serialize(&hello).unwrap();
    let decoded: Handshake = bincode::deserialize(&bytes).unwrap();

    assert_eq!(hello, decoded);
}
