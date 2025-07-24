/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#[cfg(test)]
mod tests {
    use super::super::cacheable::{Cacheable, ValidationError, WireCacheable};
    use bytes::Bytes;
    use std::error::Error;

    // A simple test struct to use with Cacheable
    #[derive(Debug, Clone, PartialEq)]
    struct TestData {
        id: String,
        value: i32,
    }

    // Mock implementation of WireCacheable for TestData
    impl WireCacheable for TestData {
        fn to_bytes(&self) -> Bytes {
            // Simple serialization for testing
            let serialized = format!("{}:{}", self.id, self.value);
            Bytes::from(serialized)
        }

        fn validate(bytes: &[u8]) -> Result<(), ValidationError> {
            // Simple validation for testing
            match std::str::from_utf8(bytes) {
                Ok(s) => {
                    if s.contains(':') {
                        Ok(())
                    } else {
                        Err(ValidationError::InvalidData {
                            shape: "TestData",
                            source: Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "Invalid format: missing colon",
                            )),
                        })
                    }
                }
                Err(e) => Err(ValidationError::InvalidData {
                    shape: "TestData",
                    source: Box::new(e),
                }),
            }
        }
    }

    #[test]
    fn test_cacheable_modeled_constructor() {
        let test_data = TestData {
            id: "test".to_string(),
            value: 42,
        };

        let cacheable = Cacheable::modeled(test_data.clone());

        assert!(cacheable.is_modeled());
        assert!(!cacheable.is_cached());
        assert_eq!(cacheable.as_modeled().unwrap().id, "test");
        assert_eq!(cacheable.as_modeled().unwrap().value, 42);
    }

    #[test]
    fn test_cacheable_cached_constructor() {
        let bytes = Bytes::from_static(b"test:42");
        let cacheable: Cacheable<TestData> = Cacheable::cached(bytes.clone());

        assert!(!cacheable.is_modeled());
        assert!(cacheable.is_cached());
        assert_eq!(cacheable.as_cached().unwrap(), &bytes);
    }

    #[test]
    fn test_cacheable_as_modeled_mut() {
        let test_data = TestData {
            id: "test".to_string(),
            value: 42,
        };

        let mut cacheable = Cacheable::modeled(test_data);

        if let Some(modeled) = cacheable.as_modeled_mut() {
            modeled.id = "modified".to_string();
            modeled.value = 100;
        }

        assert_eq!(cacheable.as_modeled().unwrap().id, "modified");
        assert_eq!(cacheable.as_modeled().unwrap().value, 100);
    }

    #[test]
    fn test_cacheable_into_modeled() {
        let test_data = TestData {
            id: "test".to_string(),
            value: 42,
        };

        let cacheable = Cacheable::modeled(test_data);
        let extracted = cacheable.into_modeled().unwrap();

        assert_eq!(extracted.id, "test");
        assert_eq!(extracted.value, 42);
    }

    #[test]
    fn test_cacheable_into_cached() {
        let bytes = Bytes::from_static(b"test:42");
        let cacheable: Cacheable<TestData> = Cacheable::cached(bytes.clone());
        let extracted = cacheable.into_cached().unwrap();

        assert_eq!(extracted, bytes);
    }

    #[test]
    fn test_validation_error_display() {
        let error = ValidationError::InvalidData {
            shape: "TestShape",
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "test error",
            )),
        };

        assert_eq!(error.to_string(), "Invalid data for shape TestShape");
    }

    #[test]
    fn test_validation_error_source() {
        let inner_error = std::io::Error::new(std::io::ErrorKind::InvalidData, "test error");
        let error = ValidationError::InvalidData {
            shape: "TestShape",
            source: Box::new(inner_error),
        };

        let source = <ValidationError as Error>::source(&error).unwrap();
        let io_error = source.downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(io_error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn test_wire_cacheable_implementation() {
        let test_data = TestData {
            id: "test".to_string(),
            value: 42,
        };

        // Test to_bytes
        let bytes = test_data.to_bytes();
        assert_eq!(bytes, Bytes::from("test:42"));

        // Test validate with valid data
        let result = TestData::validate(b"valid:123");
        assert!(result.is_ok());

        // Test validate with invalid data
        let result = TestData::validate(b"invalid_no_colon");
        assert!(result.is_err());
        if let Err(ValidationError::InvalidData { shape, .. }) = result {
            assert_eq!(shape, "TestData");
        } else {
            panic!("Expected ValidationError::InvalidData");
        }
    }
}
