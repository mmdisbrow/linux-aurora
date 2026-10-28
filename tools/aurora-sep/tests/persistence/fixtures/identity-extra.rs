// SPDX-License-Identifier: GPL-2.0-only OR MIT

fn disk_seed(s: &SepData, uuid: u8, label: u8) {
    let mut i = s.bio_index.lock();
    i.insert(bio::entry(uuid, label)).unwrap();
    s.with_host_store(|store| i.persist(store))
        .unwrap()
        .unwrap();
}
fn fail_store(s: &SepData, fail: bool) {
    s.host_store.borrow_mut().as_mut().unwrap().fail = fail;
}
fn list(s: &SepData, ids: &[u8]) {
    *s.listed.borrow_mut() = Some(ids.iter().map(|n| [*n; 16]).collect());
}
#[test]
fn retry_failed_enrollment_save_on_unchanged_reconcile() {
    let s = SepData::new();
    fail_store(&s, true);
    s.finish_enrolment(Ok([1; 16]));
    assert_eq!(s.poll().state, bio::State::Failed);
    assert_eq!(s.poll().status, 6);
    fail_store(&s, false);
    list(&s, &[1]);
    s.reconcile_identities();
    assert_eq!(bio::label(&s.reboot_index(), [1; 16]), Some([9; 128]));
    assert_eq!(
        s.poll().state,
        bio::State::Failed,
        "recovery must not republish enrollment success"
    );
}
#[test]
fn retry_failed_reconcile_save_on_unchanged_list() {
    let s = SepData::new();
    fail_store(&s, true);
    list(&s, &[1]);
    s.reconcile_identities();
    fail_store(&s, false);
    s.reconcile_identities();
    assert_eq!(bio::label(&s.reboot_index(), [1; 16]), Some([0; 128]));
}
#[test]
fn repeated_write_errors_keep_retry_pending() {
    let s = SepData::new();
    fail_store(&s, true);
    s.finish_enrolment(Ok([1; 16]));
    list(&s, &[1]);
    s.reconcile_identities();
    assert_eq!(WRITES.with(|n| n.get()), 2);
    fail_store(&s, false);
    s.reconcile_identities();
    assert_eq!(WRITES.with(|n| n.get()), 3);
    assert_eq!(bio::label(&s.reboot_index(), [1; 16]), Some([9; 128]));
    s.reconcile_identities();
    assert_eq!(WRITES.with(|n| n.get()), 3);
}
#[test]
fn duplicate_label_retry_preserves_new_metadata() {
    let s = SepData::new();
    disk_seed(&s, 1, 3);
    fail_store(&s, true);
    s.finish_enrolment(Ok([1; 16]));
    assert_eq!(bio::label(&s.reboot_index(), [1; 16]), Some([3; 128]));
    fail_store(&s, false);
    list(&s, &[1]);
    s.reconcile_identities();
    assert_eq!(bio::label(&s.reboot_index(), [1; 16]), Some([9; 128]));
}
#[test]
fn actual_forget_last_identity_can_flush_dirty_empty_index() {
    let s = SepData::new();
    disk_seed(&s, 1, 3);
    fail_store(&s, true);
    s.forget_identity(&[1; 16], c"confirmed deletion fixture");
    assert_eq!(s.bio_index.lock().total(), 0);
    assert_eq!(s.reboot_index().total(), 1);
    fail_store(&s, false);
    list(&s, &[]);
    s.reconcile_identities();
    assert_eq!(s.reboot_index().total(), 0);
}
#[test]
fn actual_forget_one_identity_retries_with_unchanged_survivor() {
    let s = SepData::new();
    disk_seed(&s, 1, 3);
    disk_seed(&s, 2, 4);
    fail_store(&s, true);
    s.forget_identity(&[1; 16], c"confirmed deletion fixture");
    fail_store(&s, false);
    list(&s, &[2]);
    s.reconcile_identities();
    let i = s.reboot_index();
    assert!(!i.contains_uuid(&[1; 16]));
    assert_eq!(bio::label(&i, [2; 16]), Some([4; 128]));
}
#[test]
fn partial_reconcile_addition_stays_dirty() {
    let s = SepData::new();
    list(&s, &[1, 2]);
    PUSH_BUDGET.with(|v| v.set(Some(1)));
    s.reconcile_identities();
    PUSH_BUDGET.with(|v| v.set(None));
    assert_eq!(s.bio_index.lock().total(), 1);
    list(&s, &[1]);
    s.reconcile_identities();
    assert_eq!(s.reboot_index().total(), 1);
}
#[test]
fn partial_reconcile_removal_and_addition_stay_dirty() {
    let s = SepData::new();
    disk_seed(&s, 1, 3);
    list(&s, &[2, 3]);
    PUSH_BUDGET.with(|v| v.set(Some(2)));
    s.reconcile_identities();
    PUSH_BUDGET.with(|v| v.set(None));
    assert_eq!(s.bio_index.lock().total(), 1);
    assert!(s.bio_index.lock().contains_uuid(&[2; 16]));
    list(&s, &[2]);
    s.reconcile_identities();
    let i = s.reboot_index();
    assert!(!i.contains_uuid(&[1; 16]));
    assert!(i.contains_uuid(&[2; 16]));
}
#[test]
fn unavailable_then_available_host_store_retries() {
    let s = SepData::new();
    *s.host_store.borrow_mut() = None;
    s.finish_enrolment(Ok([1; 16]));
    *s.host_store.borrow_mut() = Some(store::Store {
        raw: None,
        fail: false,
    });
    list(&s, &[1]);
    s.reconcile_identities();
    assert_eq!(bio::label(&s.reboot_index(), [1; 16]), Some([9; 128]));
    assert_eq!(s.poll().state, bio::State::Failed);
}
#[test]
fn empty_list_does_not_erase_nonempty_dirty_index() {
    let s = SepData::new();
    disk_seed(&s, 1, 3);
    s.bio_index.lock().insert(bio::entry(2, 4)).unwrap();
    list(&s, &[]);
    let writes = WRITES.with(|v| v.get());
    s.reconcile_identities();
    assert_eq!(s.bio_index.lock().total(), 2);
    assert_eq!(s.reboot_index().total(), 1);
    assert_eq!(WRITES.with(|v| v.get()), writes);
}
#[test]
fn new_empty_index_does_not_write_on_empty_reconcile() {
    let s = SepData::new();
    list(&s, &[]);
    s.reconcile_identities();
    assert_eq!(WRITES.with(|v| v.get()), 0);
}
#[test]
fn reconstructed_uuid_keeps_zero_label_without_inventing_metadata() {
    let s = SepData::new();
    list(&s, &[1]);
    s.reconcile_identities();
    assert_eq!(bio::label(&s.reboot_index(), [1; 16]), Some([0; 128]));
    let writes = WRITES.with(|v| v.get());
    s.reconcile_identities();
    assert_eq!(WRITES.with(|v| v.get()), writes);
}
#[test]
fn failed_explicit_save_of_clean_index_is_retried() {
    let s = SepData::new();
    disk_seed(&s, 1, 3);
    fail_store(&s, true);
    {
        let mut index = s.bio_index.lock();
        assert!(s
            .with_host_store(|store| index.persist(store))
            .unwrap()
            .is_err());
    }
    let writes = WRITES.with(|v| v.get());
    fail_store(&s, false);
    list(&s, &[1]);
    s.reconcile_identities();
    assert_eq!(WRITES.with(|v| v.get()), writes + 1);
    assert_eq!(bio::label(&s.reboot_index(), [1; 16]), Some([3; 128]));
}
