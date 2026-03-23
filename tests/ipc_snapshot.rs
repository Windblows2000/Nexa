use nexa::ipc::snapshot::Snapshot;

#[test]
fn snapshot_roundtrip() {
    let snapshot = Snapshot::default();

    let bytes = postcard::to_stdvec(&snapshot).expect("Failed to serialize Snapshot");
    let decoded: Snapshot = postcard::from_bytes(&bytes).expect("Failed to deserialize Snapshot");

    assert_eq!(snapshot, decoded);
}
