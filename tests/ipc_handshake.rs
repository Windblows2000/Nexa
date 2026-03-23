use nexa::ipc::handshake::Handshake;
use nexa::ipc::version::PROTOCOL_VERSION;

#[test]
fn handshake_roundtrip() {
    let hello = Handshake {
        protocol_version: PROTOCOL_VERSION,
        client_name: "test".into(),
        requested_features: vec![],
    };

    let bytes = postcard::to_stdvec(&hello).expect("Failed to serialize Handshake");
    let decoded: Handshake = postcard::from_bytes(&bytes).expect("Failed to deserialize Handshake");

    assert_eq!(hello, decoded);
}
