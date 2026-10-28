// SPDX-License-Identifier: GPL-2.0-only OR MIT
extern "C" {
    fn fixture_reset();
    fn fixture_done();
    fn fixture_fault(stage: i32, error: i32);
    fn fixture_write_fault(at: usize, error: i32, zero: i32);
    fn fixture_short(n: usize);
    fn fixture_set(p: *const u8, n: usize);
    fn fixture_get(p: *mut u8, n: usize) -> usize;
    fn fixture_count(n: i32) -> i32;
    fn fixture_race_publication();
}
struct Fixture;
impl Fixture {
    fn new() -> Self {
        unsafe { fixture_reset() };
        Self
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { fixture_done() };
    }
}
fn run(sep: &SepData) {
    sep.sks_machine_refkey(sks::KeyBagHandle(1), &[2; 32]);
}
fn stored() -> Vec<u8> {
    let mut bytes = vec![0; (1 << 20) + 32];
    let len = unsafe { fixture_get(bytes.as_mut_ptr(), bytes.len()) };
    bytes.truncate(len);
    bytes
}
fn seed(bytes: &[u8]) {
    unsafe { fixture_set(bytes.as_ptr(), bytes.len()) };
}
fn cached(sep: &SepData) -> bool {
    sep.machine_refkey.lock().is_some()
}

#[test]
fn new_refkey_is_cached_only_after_complete_publication() {
    let _f = Fixture::new();
    let sep = SepData::new();
    run(&sep);
    assert!(cached(&sep));
    assert_eq!(sep.creates.get(), 1);
    assert_eq!(stored(), [1, 2, 3]);
    assert_eq!(unsafe { fixture_count(1) }, 1);
    assert_eq!(unsafe { fixture_count(0) }, 1);
    assert_eq!(unsafe { fixture_count(2) }, 1);
}
#[test]
fn existing_refkey_is_loaded_without_creation_or_replacement() {
    let _f = Fixture::new();
    seed(&[4, 5, 6]);
    let sep = SepData::new();
    run(&sep);
    assert!(cached(&sep));
    assert_eq!(sep.creates.get(), 0);
    assert_eq!(stored(), [4, 5, 6]);
    assert_eq!(unsafe { fixture_count(0) }, 0);
}
#[test]
fn invalid_existing_refkey_is_refused_without_replacement() {
    let _f = Fixture::new();
    seed(&[4, 5, 6]);
    let sep = SepData::new();
    sep.valid_blob.set(false);
    run(&sep);
    assert!(!cached(&sep));
    assert_eq!(sep.creates.get(), 0);
    assert_eq!(stored(), [4, 5, 6]);
}
#[test]
fn denied_read_does_not_admit_creation() {
    let _f = Fixture::new();
    unsafe { fixture_fault(16, 13) };
    let sep = SepData::new();
    run(&sep);
    assert!(!cached(&sep));
    assert_eq!(sep.creates.get(), 0);
    assert!(stored().is_empty());
}
#[test]
fn partial_write_never_publishes_or_caches_a_refkey() {
    let _f = Fixture::new();
    unsafe { fixture_write_fault(1, 5, 0) };
    let sep = SepData::new();
    run(&sep);
    assert!(!cached(&sep));
    assert_eq!(sep.creates.get(), 1);
    assert!(stored().is_empty());
}
#[test]
fn pre_rename_failure_does_not_cache_or_publish() {
    for stage in [3, 4, 5, 6, 7, 8, 9] {
        let _f = Fixture::new();
        unsafe { fixture_fault(stage, 5) };
        let sep = SepData::new();
        run(&sep);
        assert!(!cached(&sep), "stage {stage}");
        assert!(stored().is_empty());
    }
}
#[test]
fn directory_sync_failure_does_not_cache_visible_refkey() {
    let _f = Fixture::new();
    unsafe { fixture_fault(10, 5) };
    let sep = SepData::new();
    run(&sep);
    assert!(!cached(&sep));
    assert_eq!(stored(), [1, 2, 3]);
}
#[test]
fn later_retry_loads_complete_refkey_without_creating_another() {
    let _f = Fixture::new();
    unsafe { fixture_fault(10, 5) };
    let sep = SepData::new();
    run(&sep);
    assert!(!cached(&sep));
    unsafe { fixture_fault(10, 0) };
    run(&sep);
    assert!(cached(&sep));
    assert_eq!(sep.creates.get(), 1);
    assert_eq!(stored(), [1, 2, 3]);
}

#[test]
fn later_retry_does_not_cache_while_directory_sync_still_fails() {
    let _f = Fixture::new();
    unsafe { fixture_fault(10, 5) };
    let sep = SepData::new();
    run(&sep);
    run(&sep);
    assert!(!cached(&sep));
    assert_eq!(sep.creates.get(), 1);
    assert_eq!(stored(), [1, 2, 3]);
}
#[test]
fn competing_publication_is_not_replaced_or_cached() {
    let _f = Fixture::new();
    unsafe { fixture_race_publication() };
    let sep = SepData::new();
    run(&sep);
    assert!(!cached(&sep));
    assert_eq!(sep.creates.get(), 1);
    assert_eq!(stored(), b"competing");
}
#[test]
fn exclusive_create_preserves_existing_destination() {
    let _f = Fixture::new();
    seed(b"existing");
    assert_eq!(
        shim::create_file(c"/var/lib/aurora-sep-refkey.bin", b"new"),
        Err(EEXIST)
    );
    assert_eq!(stored(), b"existing");
}
#[test]
fn allocation_failure_does_not_cache_or_publish() {
    let _f = Fixture::new();
    unsafe { fixture_fault(1, 12) };
    let sep = SepData::new();
    run(&sep);
    assert!(!cached(&sep));
    assert!(stored().is_empty());
}
#[test]
fn short_writes_publish_complete_refkey() {
    let _f = Fixture::new();
    unsafe { fixture_short(1) };
    let sep = SepData::new();
    run(&sep);
    assert!(cached(&sep));
    assert_eq!(stored(), [1, 2, 3]);
}
#[test]
fn zero_write_does_not_publish_or_cache() {
    let _f = Fixture::new();
    unsafe { fixture_write_fault(0, 0, 1) };
    let sep = SepData::new();
    run(&sep);
    assert!(!cached(&sep));
    assert!(stored().is_empty());
}
