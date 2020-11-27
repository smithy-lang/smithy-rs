#[allow(dead_code)]
mod uuid;

// This test is outside of uuid.rs to enable copying the entirety of uuid.rs into the SDK.
#[cfg(test)]
mod test {
    use crate::uuid::v4;
    use proptest::prelude::*;

    #[test]
    fn test_uuid() {
        assert_eq!(v4(0), "00000000-0000-4000-8000-000000000000");
        assert_eq!(v4(12341234), "2ff4cb00-0000-4000-8000-000000000000");
        assert_eq!(
            v4(u128::max_value()),
            "ffffffff-ffff-4fff-ffff-ffffffffffff"
        );
    }

    fn assert_valid(uuid: String) {
        assert_eq!(uuid.len(), 36);
        let bytes = uuid.as_bytes();
        let dashes: Vec<usize> = uuid
            .chars()
            .enumerate()
            .filter_map(|(idx, chr)| if chr == '-' { Some(idx) } else { None })
            .collect();
        assert_eq!(dashes, vec![8, 13, 18, 23]);
        // Check version
        assert_eq!(bytes[14] as char, '4');
        // Check variant
        assert!(bytes[19] as char >= '8');
    }

    proptest! {
        #[test]
        fn doesnt_crash_uuid(v in any::<u128>()) {
            let uuid = v4(v);
            assert_valid(uuid);
        }
    }
}
