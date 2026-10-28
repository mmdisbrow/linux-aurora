// SPDX-License-Identifier: GPL-2.0-only OR MIT

}
fn encoded(count: u32, records: &[(u8, u8)]) -> Vec<u8> {
    let mut out = 2u32.to_le_bytes().to_vec();
    out.extend_from_slice(&count.to_le_bytes());
    for (uuid, label) in records {
        out.extend_from_slice(&[*uuid; 16]);
        out.extend_from_slice(&[*label; 128]);
    }
    out
}
fn load(raw: Option<Vec<u8>>) -> Result<bio::IdentityIndex> {
    READ_ERROR.with(|v| v.set(false));
    bio::IdentityIndex::load(&mut store::Store {
        raw: raw.map(KVec),
        fail: false,
    })
}
fn refused(raw: Vec<u8>) {
    let before = raw.clone();
    let mut store = store::Store {
        raw: Some(KVec(raw)),
        fail: false,
    };
    READ_ERROR.with(|v| v.set(false));
    let result = bio::IdentityIndex::load(&mut store);
    assert!(
        matches!(result, Err(EINVAL)),
        "malformed present v2 record was accepted"
    );
    assert_eq!(store.raw.unwrap().0, before);
}
#[test]
fn missing_record_is_clean_empty() {
    let i = load(None).unwrap();
    assert_eq!(i.total(), 0);
    assert!(!i.is_dirty());
}
#[test]
fn present_empty_record_is_not_missing() {
    refused(vec![]);
}
#[test]
fn all_truncated_header_lengths_are_refused() {
    for n in 1..8 {
        let mut b = encoded(0, &[]);
        b.truncate(n);
        refused(b);
    }
}
#[test]
fn valid_empty_v2_is_clean_empty() {
    let i = load(Some(encoded(0, &[]))).unwrap();
    assert_eq!(i.total(), 0);
    assert!(!i.is_dirty());
}
#[test]
fn unsupported_version_is_refused() {
    for version in [0u32, 1, 3, u32::MAX] {
        let mut b = encoded(0, &[]);
        b[..4].copy_from_slice(&version.to_le_bytes());
        refused(b);
    }
}
#[test]
fn declared_record_without_body_is_refused() {
    refused(encoded(1, &[]));
}
#[test]
fn every_partial_first_record_is_refused() {
    let b = encoded(1, &[(1, 9)]);
    for n in 9..b.len() {
        refused(b[..n].to_vec());
    }
}
#[test]
fn every_partial_second_record_is_refused() {
    let b = encoded(2, &[(1, 9), (2, 8)]);
    for n in 152..b.len() {
        refused(b[..n].to_vec());
    }
}
#[test]
fn count_above_capacity_is_not_capped() {
    let records: Vec<_> = (1..=33).map(|n| (n, 9)).collect();
    refused(encoded(33, &records));
}
#[test]
fn huge_count_is_refused_without_overflow() {
    refused(encoded(u32::MAX, &[]));
}
#[test]
fn trailing_zero_padding_is_not_a_writer_format() {
    for n in [1, 8, 144, 4096] {
        let mut b = encoded(1, &[(1, 9)]);
        b.extend(vec![0; n]);
        refused(b);
    }
}
#[test]
fn trailing_complete_record_with_wrong_count_is_refused() {
    refused(encoded(1, &[(1, 9), (2, 8)]));
}
#[test]
fn zero_count_with_trailing_record_is_refused() {
    refused(encoded(0, &[(1, 9)]));
}
#[test]
fn duplicate_uuid_same_label_is_refused() {
    refused(encoded(2, &[(1, 9), (1, 9)]));
}
#[test]
fn duplicate_uuid_conflicting_label_is_refused() {
    refused(encoded(2, &[(1, 9), (1, 8)]));
}
#[test]
fn zero_uuid_is_refused() {
    refused(encoded(1, &[(0, 9)]));
}
#[test]
fn valid_nonterminated_binary_label_is_preserved() {
    let i = load(Some(encoded(1, &[(1, 255)]))).unwrap();
    assert_eq!(bio::label(&i, [1; 16]), Some([255; 128]));
    assert!(!i.is_dirty());
}
#[test]
fn valid_zero_unknown_label_is_preserved() {
    let i = load(Some(encoded(1, &[(1, 0)]))).unwrap();
    assert_eq!(bio::label(&i, [1; 16]), Some([0; 128]));
}
#[test]
fn full_capacity_round_trip_matches_writer_exactly() {
    let records: Vec<_> = (1..=32).map(|n| (n, n + 1)).collect();
    let bytes = encoded(32, &records);
    let mut i = load(Some(bytes.clone())).unwrap();
    assert_eq!(i.total(), 32);
    let mut store = store::Store {
        raw: None,
        fail: false,
    };
    i.persist(&mut store).unwrap();
    assert_eq!(store.raw.unwrap().0, bytes);
    assert!(!i.is_dirty());
}
#[test]
fn store_read_error_is_not_missing() {
    READ_ERROR.with(|v| v.set(true));
    let mut store = store::Store {
        raw: None,
        fail: false,
    };
    assert!(matches!(bio::IdentityIndex::load(&mut store), Err(5)));
    READ_ERROR.with(|v| v.set(false));
}
#[test]
fn historical_writer_fixture_has_identical_v2_bytes() {
    let bytes = encoded(3, &[(1, 3), (2, 4), (3, 5)]);
    let mut i = load(Some(bytes.clone())).unwrap();
    let mut store = store::Store {
        raw: None,
        fail: false,
    };
    i.persist(&mut store).unwrap();
    assert_eq!(store.raw.unwrap().0, bytes);
}
