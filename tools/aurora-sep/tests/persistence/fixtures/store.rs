// SPDX-License-Identifier: GPL-2.0-only OR MIT
extern "C" {
    fn fixture_reset();
    fn fixture_done();
    fn fixture_arm(stage: i32, which: i32, at: usize, error: i32);
    fn fixture_clear();
    fn fixture_sync_error(n: i32);
    fn fixture_data_error(n: i32);
    fn fixture_short_reads(n: usize);
    fn fixture_count(n: i32) -> i32;
    fn fixture_crash();
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
fn fault(stage: i32, which: i32, at: usize, error: i32) {
    unsafe { fixture_arm(stage, which, at, error) }
}
fn clear() {
    unsafe { fixture_clear() }
}
fn count(n: i32) -> i32 {
    unsafe { fixture_count(n) }
}
fn crash() {
    unsafe { fixture_crash() }
}
const A: store::Key = store::Key {
    kind: 0xf1,
    uuid: [0; 16],
};
const B: store::Key = store::Key {
    kind: 0xf5,
    uuid: [0; 16],
};
fn seeded() -> store::Store {
    let mut s = store::Store::open().unwrap();
    s.write(&A, b"old-index").unwrap();
    s.write(&B, b"old-lockout").unwrap();
    s
}
fn value(s: &mut store::Store, k: &store::Key) -> Vec<u8> {
    s.read(k).unwrap().unwrap().to_vec()
}
#[test]
fn initial_creation_and_ordinary_writes_survive_reopen() {
    let _f = Fixture::new();
    let s = seeded();
    drop(s);
    crash();
    let mut reopened = store::Store::open().unwrap();
    assert_eq!(value(&mut reopened, &A), b"old-index");
    assert_eq!(value(&mut reopened, &B), b"old-lockout");
    assert_eq!(count(2), 0);
}
#[test]
fn every_partial_header_boundary_preserves_previous_record() {
    let mut cases = 0;
    for phase in [1, 2] {
        for prefix in [
            0, 1, 16, 28, 36, 37, 38, 39, 40, 64, 65, 66, 82, 83, 84, 87, 88, 89, 111, 112, 113,
            114, 130, 131, 132, 136, 4096, 32767,
        ] {
            let _f = Fixture::new();
            let mut s = seeded();
            fault(1, phase, prefix, 5);
            assert_eq!(s.write(&A, b"new-index"), Err(EIO));
            drop(s);
            crash();
            let mut r = store::Store::open().unwrap();
            assert_eq!(
                value(&mut r, &A),
                b"old-index",
                "phase={phase} prefix={prefix}"
            );
            assert_eq!(value(&mut r, &B), b"old-lockout");
            cases += 1;
        }
    }
    println!("production prefix/reopen cases: {cases}");
}
#[test]
fn tail_and_metadata_precommit_faults_preserve_all_committed_records() {
    let mut cases = 0;
    for phase in [1, 2] {
        for (stage, at, error) in [
            (2, 32768, 5),
            (2, 32775, 28),
            (2, 65535, 5),
            (2, 72 * 32768 - 1, 5),
            (3, 32768, 5),
            (3, 65536, 5),
            (4, 32768, 5),
            (4, 65536, 5),
            (5, 0, 12),
            (6, 0, 5),
            (7, 0, 5),
        ] {
            let _f = Fixture::new();
            let mut s = seeded();
            fault(stage, phase, at, error);
            assert!(s.write(&A, b"new-index").is_err());
            drop(s);
            crash();
            let mut r = store::Store::open().unwrap();
            assert_eq!(value(&mut r, &A), b"old-index");
            assert_eq!(value(&mut r, &B), b"old-lockout");
            cases += 1;
        }
    }
    println!("production precommit failure cases: {cases}");
}
#[test]
fn short_source_reads_and_full_tail_are_copied_exactly() {
    let _f = Fixture::new();
    let mut s = seeded();
    let payload: Vec<u8> = (0..32768).map(|i| (i % 251) as u8).collect();
    s.write(&B, &payload).unwrap();
    unsafe { fixture_short_reads(137) };
    s.write(&A, b"new-index").unwrap();
    assert!(count(5) > 1000);
    drop(s);
    crash();
    let mut r = store::Store::open().unwrap();
    assert_eq!(value(&mut r, &A), b"new-index");
    assert_eq!(value(&mut r, &B), payload);
}
#[test]
fn failed_dirsync_still_moves_handle_for_different_key_write() {
    for phase in [1, 2] {
        for persistence in [5, -5] {
            let _f = Fixture::new();
            let mut s = seeded();
            fault(8, phase, 0, persistence);
            assert_eq!(s.write(&A, b"new-index"), Err(EIO));
            clear();
            s.write(&B, b"new-lockout").unwrap();
            assert_eq!(count(2), 0, "no writes may hit an old unlinked inode");
            drop(s);
            crash();
            let mut r = store::Store::open().unwrap();
            assert_eq!(value(&mut r, &B), b"new-lockout");
            assert_eq!(
                value(&mut r, &A),
                if phase == 1 {
                    &b"old-index"[..]
                } else {
                    &b"new-index"[..]
                }
            );
        }
    }
}
#[test]
fn repeated_commit_failure_then_failed_retry_preserves_committed_values() {
    for second_stage in [1, 2, 6, 7, 8] {
        let _f = Fixture::new();
        let mut s = seeded();
        fault(1, 2, 88, 5);
        assert_eq!(s.write(&A, b"first-failed"), Err(EIO));
        fault(
            second_stage,
            2,
            if second_stage == 2 { 32775 } else { 88 },
            5,
        );
        assert_eq!(s.write(&B, b"second-failed"), Err(EIO));
        clear();
        s.write(&B, b"finally-stored").unwrap();
        assert_eq!(count(2), 0);
        drop(s);
        crash();
        let mut r = store::Store::open().unwrap();
        assert_eq!(value(&mut r, &A), b"first-failed");
        assert_eq!(value(&mut r, &B), b"finally-stored");
    }
}
#[test]
fn raw_data_and_outer_sync_failures_retry_on_current_inode() {
    for sync in 0..=3 {
        let _f = Fixture::new();
        let mut s = seeded();
        if sync == 0 {
            unsafe { fixture_data_error(1) }
        } else {
            unsafe { fixture_sync_error(sync) }
        }
        assert_eq!(s.write(&A, b"new-index"), Err(EIO));
        clear();
        s.write(&A, b"retry-index").unwrap();
        assert_eq!(count(2), 0);
        drop(s);
        crash();
        let mut r = store::Store::open().unwrap();
        assert_eq!(value(&mut r, &A), b"retry-index");
        assert_eq!(value(&mut r, &B), b"old-lockout");
    }
}
#[test]
fn stale_canonical_inode_is_refused_before_any_rename() {
    for stage in [9, 10] {
        let _f = Fixture::new();
        let mut s = seeded();
        fault(stage, 1, 0, 5);
        assert_eq!(s.write(&A, b"new-index"), Err(Error::from_errno(-116)));
        assert_eq!(count(0), 0);
        assert_eq!(count(2), 0);
        clear();
        assert_eq!(s.write(&A, b"retry-index"), Err(Error::from_errno(-116)));
        assert_eq!(count(0), 0);
    }
}
#[test]
fn source_size_change_is_refused_before_any_rename() {
    for stage in [11, 12] {
        let _f = Fixture::new();
        let mut s = seeded();
        fault(stage, 1, 65536, 5);
        assert_eq!(s.write(&A, b"new-index"), Err(Error::from_errno(-116)));
        assert_eq!(count(0), 0);
        assert_eq!(count(2), 0);
    }
}
#[test]
fn old_descriptor_remains_valid_until_closed() {
    let _f = Fixture::new();
    let mut writer = seeded();
    let mut observer = store::Store::open().unwrap();
    writer.write(&A, b"new-index").unwrap();
    assert_eq!(value(&mut observer, &A), b"old-index");
    assert_eq!(value(&mut writer, &A), b"new-index");
    let mut fresh = store::Store::open().unwrap();
    assert_eq!(value(&mut fresh, &A), b"new-index");
    assert_eq!(count(2), 0);
}

#[test]
fn source_size_changes_during_stream_are_refused() {
    for stage in [13, 14] {
        let _f = Fixture::new();
        let mut s = seeded();
        fault(stage, 1, 65536, 5);
        assert!(s.write(&A, b"new-index").is_err());
        assert_eq!(count(0), 0);
        assert_eq!(count(2), 0);
    }
}
#[test]
fn header_callback_copies_short_final_chunks_and_boundary_bytes() {
    for tail in [1, 32767, 32768, 32769, 65553] {
        let _f = Fixture::new();
        let path = c"/var/lib/aurora-sep-host-state.bin";
        let (mut file, created) = shim::StoreFile::open_host(path).unwrap();
        assert!(created);
        let image: Vec<u8> = (0..32768 + tail).map(|i| (i % 251) as u8).collect();
        file.write_all(0, &image).unwrap();
        file.sync().unwrap();
        let header = vec![0xa5; 32768];
        file.replace_header(path, &header, image.len() as u64)
            .unwrap();
        let mut actual = vec![0; image.len()];
        file.read_exact(0, &mut actual).unwrap();
        assert_eq!(&actual[..32768], &header);
        assert_eq!(&actual[32768..], &image[32768..]);
        drop(file);
        crash();
        let file = shim::StoreFile::open_readonly(path).unwrap();
        file.read_exact(0, &mut actual).unwrap();
        assert_eq!(&actual[32768..], &image[32768..]);
        assert_eq!(count(2), 0);
    }
}
