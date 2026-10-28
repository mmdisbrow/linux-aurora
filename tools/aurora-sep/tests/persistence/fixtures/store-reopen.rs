// SPDX-License-Identifier: GPL-2.0-only OR MIT

fn retry_cases() -> Vec<(i32, usize, i32)> {
    vec![
        (1, 88, 5),
        (2, 32775, 5),
        (6, 0, 5),
        (7, 0, 5),
        (8, 0, 5),
        (8, 0, -5),
    ]
}
#[test]
fn crossreview_second_failure_reopen_different_key() {
    let mut cases = 0;
    for phase in [1, 2] {
        for (stage, at, error) in retry_cases() {
            let _f = Fixture::new();
            let mut s = seeded();
            fault(1, 2, 88, 5);
            assert_eq!(s.write(&A, b"first-failed"), Err(EIO));
            fault(stage, phase, at, error);
            assert_eq!(s.write(&B, b"second-failed"), Err(EIO));
            assert_eq!(count(2), 0);
            drop(s);
            crash();
            let mut r = store::Store::open().unwrap();
            let published_intent = phase == 2 || (stage == 8 && error < 0);
            let published_commit = phase == 2 && stage == 8 && error < 0;
            assert_eq!(
                value(&mut r, &A),
                if published_intent {
                    &b"first-failed"[..]
                } else {
                    &b"old-index"[..]
                },
                "phase={phase} stage={stage} error={error}"
            );
            assert_eq!(
                value(&mut r, &B),
                if published_commit {
                    &b"second-failed"[..]
                } else {
                    &b"old-lockout"[..]
                },
                "phase={phase} stage={stage} error={error}"
            );
            cases += 1;
        }
    }
    println!("independent different-key immediate reopen cases: {cases}");
}
#[test]
fn crossreview_second_failure_reopen_same_key() {
    let mut cases = 0;
    for phase in [1, 2] {
        for (stage, at, error) in retry_cases() {
            let _f = Fixture::new();
            let mut s = seeded();
            fault(1, 2, 88, 5);
            assert_eq!(s.write(&A, b"first-failed"), Err(EIO));
            fault(stage, phase, at, error);
            assert_eq!(s.write(&A, b"second-failed"), Err(EIO));
            assert_eq!(count(2), 0);
            drop(s);
            crash();
            let mut r = store::Store::open().unwrap();
            let published_intent = phase == 2 || (stage == 8 && error < 0);
            let published_commit = phase == 2 && stage == 8 && error < 0;
            let expected = if published_commit {
                &b"second-failed"[..]
            } else if published_intent {
                &b"first-failed"[..]
            } else {
                &b"old-index"[..]
            };
            assert_eq!(
                value(&mut r, &A),
                expected,
                "phase={phase} stage={stage} error={error}"
            );
            assert_eq!(value(&mut r, &B), b"old-lockout");
            cases += 1;
        }
    }
    println!("independent same-key immediate reopen cases: {cases}");
}
