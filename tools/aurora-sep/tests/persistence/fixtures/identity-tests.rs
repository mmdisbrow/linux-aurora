// SPDX-License-Identifier: GPL-2.0-only OR MIT

}
#[test]
fn durable_success_and_label_survive_reload() {
    let s = SepData::new();
    s.finish_enrolment(Ok([1; 16]));
    assert_eq!(s.poll().state, bio::State::Done);
    assert_eq!(bio::label(&s.reboot_index(), [1; 16]), Some([9; 128]));
    assert_eq!(WRITES.with(|v| v.get()), 1);
    assert_eq!(s.wakes.get(), 1);
}
#[test]
fn write_error_is_unfiled_not_success() {
    let s = SepData::new();
    s.host_store.borrow_mut().as_mut().unwrap().fail = true;
    s.finish_enrolment(Ok([1; 16]));
    let poll = s.poll();
    assert_eq!(poll.state, bio::State::Failed);
    assert_eq!(poll.status, ENROL_STATUS_UNFILED);
}
#[test]
fn unavailable_store_is_unfiled_not_success() {
    let s = SepData::new();
    *s.host_store.borrow_mut() = None;
    s.finish_enrolment(Ok([1; 16]));
    assert_eq!(s.poll().state, bio::State::Failed);
    assert_eq!(s.poll().status, ENROL_STATUS_UNFILED);
}
#[test]
fn no_poll_success_before_index_write() {
    let s = SepData::new();
    POLL_GAP.with(|v| v.set(true));
    s.finish_enrolment(Ok([1; 16]));
    let gap = GAP_RESULT.with(|v| v.get());
    assert_ne!(
        gap,
        Some((bio::State::Done as u32, 0)),
        "poll observed Done before persistence began"
    );
    assert_eq!(s.poll().state, bio::State::Done);
}
#[test]
fn cancellation_after_write_wins_over_publication() {
    let s = SepData::new();
    CANCEL_AFTER_WRITE.with(|v| v.set(true));
    s.finish_enrolment(Ok([1; 16]));
    let poll = s.poll();
    assert_eq!(poll.state, bio::State::Failed);
    assert_eq!(poll.status, 0);
    assert_eq!(s.wakes.get(), 0);
    assert_eq!(s.reboot_index().total(), 1);
}
#[test]
fn close_after_write_cannot_publish() {
    let s = SepData::new();
    CLOSE_AFTER_WRITE.with(|v| v.set(true));
    s.finish_enrolment(Ok([1; 16]));
    let _ = s.poll();
    assert_eq!(s.wakes.get(), 0);
    assert_eq!(s.reboot_index().total(), 1);
}
#[test]
fn cancelled_before_finish_never_writes() {
    let s = SepData::new();
    {
        let mut session = s.bio_session.lock();
        let mut ctx = bio::Context {
            session: &mut session,
        };
        bio::cancel(&mut ctx).unwrap();
    }
    s.finish_enrolment(Ok([1; 16]));
    assert_eq!(WRITES.with(|v| v.get()), 0);
    assert_eq!(s.wakes.get(), 0);
}
#[test]
fn stale_epoch_never_writes() {
    let s = SepData::new();
    s.capture_epoch.store(6, Relaxed);
    s.finish_enrolment(Ok([1; 16]));
    assert_eq!(WRITES.with(|v| v.get()), 0);
    assert_eq!(s.wakes.get(), 0);
}
#[test]
fn completed_operation_never_republishes() {
    let s = SepData::new();
    s.finish_enrolment(Err(3));
    s.finish_enrolment(Ok([1; 16]));
    assert_eq!(s.poll().status, 3);
    assert_eq!(WRITES.with(|v| v.get()), 0);
    assert_eq!(s.wakes.get(), 1);
}
#[test]
fn failure_retains_in_memory_identity_without_claiming_success() {
    let s = SepData::new();
    s.host_store.borrow_mut().as_mut().unwrap().fail = true;
    s.finish_enrolment(Ok([1; 16]));
    assert!(s.bio_index.lock().contains_uuid(&[1; 16]));
    assert_eq!(s.reboot_index().total(), 0);
    assert_eq!(s.poll().state, bio::State::Failed);
}
#[test]
fn duplicate_identity_updates_label_and_persists() {
    let s = SepData::new();
    s.bio_index.lock().insert(bio::entry(1, 3)).unwrap();
    s.finish_enrolment(Ok([1; 16]));
    assert_eq!(s.bio_index.lock().total(), 1);
    assert_eq!(bio::label(&s.reboot_index(), [1; 16]), Some([9; 128]));
    assert_eq!(s.poll().state, bio::State::Done);
}
#[test]
fn full_index_reports_failure() {
    let s = SepData::new();
    for n in 1..=32 {
        s.bio_index.lock().insert(bio::entry(n, 3)).unwrap();
    }
    s.finish_enrolment(Ok([99; 16]));
    assert_eq!(s.poll().state, bio::State::Failed);
}
#[test]
fn reconcile_addition_is_persisted() {
    let s = SepData::new();
    *s.listed.borrow_mut() = Some(vec![[1; 16]]);
    s.reconcile_identities();
    assert_eq!(s.bio_index.lock().total(), 1);
    assert_eq!(s.reboot_index().total(), 1);
    assert_eq!(bio::label(&s.reboot_index(), [1; 16]), Some([0; 128]));
}
#[test]
fn reconcile_additions_and_deletions_are_persisted() {
    let s = SepData::new();
    s.bio_index.lock().insert(bio::entry(1, 3)).unwrap();
    s.bio_index.lock().insert(bio::entry(2, 4)).unwrap();
    *s.listed.borrow_mut() = Some(vec![[2; 16], [3; 16]]);
    s.reconcile_identities();
    let loaded = s.reboot_index();
    assert!(!loaded.contains_uuid(&[1; 16]));
    assert_eq!(bio::label(&loaded, [2; 16]), Some([4; 128]));
    assert_eq!(bio::label(&loaded, [3; 16]), Some([0; 128]));
}
#[test]
fn reconcile_unchanged_has_no_extra_write() {
    let s = SepData::new();
    disk_seed(&s, 1, 3);
    WRITES.with(|v| v.set(0));
    *s.listed.borrow_mut() = Some(vec![[1; 16]]);
    s.reconcile_identities();
    assert_eq!(WRITES.with(|v| v.get()), 0);
}
#[test]
fn reconcile_missing_list_preserves_index() {
    let s = SepData::new();
    s.bio_index.lock().insert(bio::entry(1, 3)).unwrap();
    s.reconcile_identities();
    assert_eq!(s.bio_index.lock().total(), 1);
    assert_eq!(WRITES.with(|v| v.get()), 0);
}
#[test]
fn reconcile_write_error_retains_memory_but_not_disk() {
    let s = SepData::new();
    s.host_store.borrow_mut().as_mut().unwrap().fail = true;
    *s.listed.borrow_mut() = Some(vec![[1; 16]]);
    s.reconcile_identities();
    assert_eq!(s.bio_index.lock().total(), 1);
    assert_eq!(s.reboot_index().total(), 0);
}
#[test]
fn original_error_status_is_preserved() {
    let s = SepData::new();
    s.finish_enrolment(Err(4));
    assert_eq!(s.poll().state, bio::State::Failed);
    assert_eq!(s.poll().status, 4);
    assert_eq!(WRITES.with(|v| v.get()), 0);
}
