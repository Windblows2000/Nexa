use bincode;
use nexa::ipc::snapshot::Snapshot;

#[test]
fn snapshot_roundtrip() {
    let snapshot = Snapshot::default();

    let bytes = bincode::serialize(&snapshot).unwrap();
    let decoded: Snapshot = bincode::deserialize(&bytes).unwrap();

    assert_eq!(snapshot, decoded);
}
