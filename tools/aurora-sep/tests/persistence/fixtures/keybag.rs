// SPDX-License-Identifier: GPL-2.0-only OR MIT
// All bag, secret and UUID bytes in this fixture are invented test values.
thread_local! { static BODY_ALLOC_FAIL: Cell<bool> = const { Cell::new(false) }; }
extern "C" {
    fn fixture_reset();
    fn fixture_done();
    fn fixture_fault(stage: i32, error: i32);
    fn fixture_write_fault(at: usize, error: i32, zero: i32);
    fn fixture_short(n: usize);
    fn fixture_count(n: i32) -> i32;
    fn fixture_get(p: *mut u8, n: usize) -> usize;
    fn fixture_set(p: *const u8, n: usize);
}
struct Fixture;
impl Fixture {
    fn new() -> Self {
        BODY_ALLOC_FAIL.with(|v| v.set(false));
        unsafe { fixture_reset() };
        Self
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        BODY_ALLOC_FAIL.with(|v| v.set(false));
        unsafe { fixture_done() };
    }
}
fn seed() -> keybag_identity::SnapshotIdentity {
    keybag::write_generated_uuid(keybag::Slot::Identity, &[11, 22, 33], &[7; 16], &[9; 32])
        .unwrap();
    keybag_identity::SnapshotIdentity::from_loaded(
        [7; 16],
        keybag_identity::UuidProvenance::AsGenerated,
        [8; 16],
        true,
    )
    .unwrap()
}
fn current() -> keybag::StoredKeyBag {
    match keybag::read(keybag::Slot::Identity).unwrap() {
        keybag::State::Present(bag) => bag,
        _ => panic!("committed bag missing"),
    }
}
fn check(wrapped: &[u8]) {
    let bag = current();
    assert_eq!(bag.wrapped(), wrapped);
    assert_eq!(bag.uuid(), &[7; 16]);
    assert_eq!(bag.secret(), &[9; 32]);
    assert!(bag.uuid_provenance() == keybag_identity::UuidProvenance::AsGenerated);
}
fn update(proof: &keybag_identity::SnapshotIdentity) -> Result<()> {
    keybag::replace_wrapped(keybag::Slot::Identity, &[44, 55], &[8; 16], proof)
}
fn fault(stage: i32, error: i32) {
    unsafe { fixture_fault(stage, error) };
}

#[test]
fn successful_snapshot_roundtrips() {
    let _f = Fixture::new();
    let proof = seed();
    update(&proof).unwrap();
    check(&[44, 55]);
}
#[test]
fn readback_uuid_provenance_is_preserved() {
    let _f = Fixture::new();
    keybag::write_bag_uuid(keybag::Slot::Identity, &[1, 2], &[7; 16], &[9; 32]).unwrap();
    let proof = keybag_identity::SnapshotIdentity::from_loaded(
        [7; 16],
        keybag_identity::UuidProvenance::ReadBackFromBag,
        [7; 16],
        false,
    )
    .unwrap();
    keybag::replace_wrapped(keybag::Slot::Identity, &[3], &[7; 16], &proof).unwrap();
    let bag = current();
    assert_eq!(bag.wrapped(), &[3]);
    assert!(bag.uuid_provenance() == keybag_identity::UuidProvenance::ReadBackFromBag);
}
#[test]
fn maximum_record_roundtrips() {
    let _f = Fixture::new();
    keybag::write_generated_uuid(keybag::Slot::Identity, &[1; 4096], &[7; 16], &[9; 4096]).unwrap();
    let bag = current();
    assert_eq!(bag.wrapped(), &[1; 4096]);
    assert_eq!(bag.secret(), &[9; 4096]);
}
#[test]
fn snapshot_first_write_failure_preserves_previous_generation() {
    let _f = Fixture::new();
    let proof = seed();
    unsafe { fixture_write_fault(0, 5, 0) };
    assert_eq!(update(&proof), Err(EIO));
    check(&[11, 22, 33]);
}
#[test]
fn partial_header_failure_preserves_previous_generation() {
    let _f = Fixture::new();
    let proof = seed();
    unsafe { fixture_write_fault(7, 5, 0) };
    assert_eq!(update(&proof), Err(EIO));
    check(&[11, 22, 33]);
}
#[test]
fn partial_secret_failure_preserves_previous_generation() {
    let _f = Fixture::new();
    let proof = seed();
    unsafe { fixture_write_fault(0x34 + 2 + 5, 5, 0) };
    assert_eq!(update(&proof), Err(EIO));
    check(&[11, 22, 33]);
}
#[test]
fn zero_write_is_error_and_preserves_previous_generation() {
    let _f = Fixture::new();
    let proof = seed();
    unsafe { fixture_write_fault(0x34, 0, 1) };
    assert_eq!(update(&proof), Err(EIO));
    check(&[11, 22, 33]);
}
#[test]
fn short_writes_complete_and_do_not_leave_trailing_bytes() {
    let _f = Fixture::new();
    let proof = seed();
    unsafe { fixture_short(1) };
    update(&proof).unwrap();
    check(&[44, 55]);
}
#[test]
fn pre_rename_failures_preserve_previous_generation() {
    for stage in [3, 4, 5, 6, 7, 8, 9] {
        let _f = Fixture::new();
        let proof = seed();
        fault(stage, 5);
        assert_eq!(update(&proof), Err(EIO), "stage {stage}");
        check(&[11, 22, 33]);
    }
}
#[test]
fn allocation_failure_preserves_previous_generation() {
    let _f = Fixture::new();
    let proof = seed();
    fault(1, 12);
    assert_eq!(update(&proof), Err(ENOMEM));
    check(&[11, 22, 33]);
}
#[test]
fn body_allocation_failure_precedes_any_file_mutation() {
    let _f = Fixture::new();
    let proof = seed();
    BODY_ALLOC_FAIL.with(|v| v.set(true));
    assert_eq!(update(&proof), Err(ENOMEM));
    BODY_ALLOC_FAIL.with(|v| v.set(false));
    check(&[11, 22, 33]);
    assert_eq!(unsafe { fixture_count(1) }, 1);
}
#[test]
fn post_rename_sync_failure_returns_error_with_complete_new_record() {
    let _f = Fixture::new();
    let proof = seed();
    fault(10, 5);
    assert_eq!(update(&proof), Err(EIO));
    check(&[44, 55]);
}
#[test]
fn retry_after_uncertain_directory_sync_keeps_identity() {
    let _f = Fixture::new();
    let proof = seed();
    fault(10, 5);
    assert_eq!(update(&proof), Err(EIO));
    fault(10, 0);
    keybag::replace_wrapped(keybag::Slot::Identity, &[66], &[8; 16], &proof).unwrap();
    check(&[66]);
}
#[test]
fn failed_initial_creation_does_not_publish_a_partial_file() {
    let _f = Fixture::new();
    unsafe { fixture_write_fault(3, 5, 0) };
    assert_eq!(
        keybag::write_generated_uuid(keybag::Slot::Identity, &[1], &[7; 16], &[9]),
        Err(EIO)
    );
    assert!(matches!(
        keybag::read(keybag::Slot::Identity),
        Ok(keybag::State::Absent(_))
    ));
}
#[test]
fn intent_requires_complete_publication_and_blocks_creation() {
    let _f = Fixture::new();
    keybag::write_intent(keybag::Slot::Identity).unwrap();
    assert!(matches!(keybag::read(keybag::Slot::Identity), Err(EEXIST)));
}
#[test]
fn intent_directory_sync_failure_stays_blocked() {
    let _f = Fixture::new();
    fault(10, 5);
    assert_eq!(keybag::write_intent(keybag::Slot::Identity), Err(EIO));
    assert!(matches!(keybag::read(keybag::Slot::Identity), Err(EEXIST)));
}
#[test]
fn explicit_refusal_is_retryable_after_successful_publication() {
    let _f = Fixture::new();
    keybag::write_intent(keybag::Slot::Identity).unwrap();
    keybag::mark_refused(keybag::Slot::Identity).unwrap();
    assert!(matches!(
        keybag::read(keybag::Slot::Identity),
        Ok(keybag::State::Absent(_))
    ));
}
#[test]
fn failed_refusal_keeps_intent_blocking_recreation() {
    let _f = Fixture::new();
    keybag::write_intent(keybag::Slot::Identity).unwrap();
    unsafe { fixture_write_fault(3, 5, 0) };
    assert_eq!(keybag::mark_refused(keybag::Slot::Identity), Err(EIO));
    assert!(matches!(keybag::read(keybag::Slot::Identity), Err(EEXIST)));
}
#[test]
fn wrong_identity_does_not_mutate_file() {
    let _f = Fixture::new();
    let proof = seed();
    assert_eq!(
        keybag::replace_wrapped(keybag::Slot::Identity, &[1], &[2; 16], &proof),
        Err(EPERM)
    );
    check(&[11, 22, 33]);
}
#[test]
fn invalid_fresh_blob_does_not_mutate_file() {
    let _f = Fixture::new();
    let proof = seed();
    for fresh in [vec![], vec![1; 4097]] {
        assert_eq!(
            keybag::replace_wrapped(keybag::Slot::Identity, &fresh, &[8; 16], &proof),
            Err(EINVAL)
        );
        check(&[11, 22, 33]);
    }
}

fn record_bytes() -> Vec<u8> {
    let mut bytes = vec![0; 0x34 + 8192];
    let size = unsafe { fixture_get(bytes.as_mut_ptr(), bytes.len()) };
    bytes.truncate(size);
    bytes
}

fn put_record(bytes: &[u8]) {
    unsafe { fixture_set(bytes.as_ptr(), bytes.len()) };
}

#[test]
fn damaged_refusal_never_authorizes_creation() {
    let _f = Fixture::new();
    keybag::mark_refused(keybag::Slot::Identity).unwrap();
    let original = record_bytes();
    // Every bit of the persistent record, including its reserved bytes,
    // must be checked before an Absent proof is granted.
    for offset in 0..original.len() {
        for bit in 0..8 {
            let mut damaged = original.clone();
            damaged[offset] ^= 1 << bit;
            put_record(&damaged);
            assert!(keybag::read(keybag::Slot::Identity).is_err(),
                    "accepted corrupted refusal byte {offset}, bit {bit}");
        }
    }
}

#[test]
fn committed_state_bit_flip_cannot_become_absent() {
    let _f = Fixture::new();
    keybag::write_bag_uuid(keybag::Slot::Identity, &[1, 2], &[7; 16], &[9; 32]).unwrap();
    let mut bytes = record_bytes();
    bytes[0x14] ^= 1; // Read-back committed state 3 becomes refusal state 2.
    put_record(&bytes);
    assert!(keybag::read(keybag::Slot::Identity).is_err());
}

#[test]
fn refusal_with_trailing_bytes_is_not_absent() {
    let _f = Fixture::new();
    keybag::mark_refused(keybag::Slot::Identity).unwrap();
    let mut bytes = record_bytes();
    bytes.push(0);
    put_record(&bytes);
    assert!(keybag::read(keybag::Slot::Identity).is_err());
}
