/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The [`DocumentV2`]-specific parts of [`DiscriminatedDocument`].

use std::borrow::Cow;

use super::{number_as_timestamp, sealed, string_as_blob, string_as_timestamp};
use crate::document::document_v2::variant_name;
use crate::{DateTime, DiscriminatedDocument, DocumentError, DocumentSettings, DocumentV2};

impl DiscriminatedDocument<DocumentV2> {
    /// Wraps a [`DocumentV2`], with no discriminator and no settings.
    ///
    /// Attach a discriminator with
    /// [`try_with_discriminator`](DiscriminatedDocument::try_with_discriminator).
    pub fn new_v2(document: DocumentV2) -> Self {
        Self::wrap(document)
    }
}

impl From<DocumentV2> for DiscriminatedDocument<DocumentV2> {
    fn from(document: DocumentV2) -> Self {
        Self::new_v2(document)
    }
}

impl sealed::Sealed for DocumentV2 {
    fn coerce_to_blob<'a>(
        &'a self,
        settings: Option<&dyn DocumentSettings>,
    ) -> Result<Cow<'a, [u8]>, DocumentError> {
        match self {
            DocumentV2::Blob(bytes) => Ok(Cow::Borrowed(bytes)),
            DocumentV2::String(s) => string_as_blob(s, settings),
            other => Err(DocumentError::type_mismatch(format!(
                "expected blob, found {}",
                variant_name(other)
            ))),
        }
    }

    fn coerce_to_timestamp(
        &self,
        settings: Option<&dyn DocumentSettings>,
    ) -> Result<DateTime, DocumentError> {
        match self {
            DocumentV2::Timestamp(timestamp) => Ok(*timestamp),
            DocumentV2::Number(n) => number_as_timestamp(n, settings),
            DocumentV2::String(s) => string_as_timestamp(s, settings),
            other => Err(DocumentError::type_mismatch(format!(
                "expected timestamp, found {}",
                variant_name(other)
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Number;
    use std::sync::Arc;

    #[derive(Debug)]
    struct TestSettings;

    impl DocumentSettings for TestSettings {
        fn protocol_id(&self) -> &str {
            "com.example#Test"
        }

        fn coerce_string_to_blob(&self, s: &str) -> Result<Vec<u8>, DocumentError> {
            // Not base64, so that tests can tell the settings path from the default.
            Ok(s.as_bytes().to_vec())
        }

        fn coerce_string_to_timestamp(&self, _s: &str) -> Result<DateTime, DocumentError> {
            Ok(DateTime::from_secs(0))
        }

        fn coerce_number_to_timestamp(&self, _n: &Number) -> Result<DateTime, DocumentError> {
            Ok(DateTime::from_secs(1))
        }
    }

    fn settings() -> Arc<dyn DocumentSettings> {
        Arc::new(TestSettings)
    }

    fn wrap(document: DocumentV2) -> DiscriminatedDocument<DocumentV2> {
        DiscriminatedDocument::new_v2(document)
    }

    #[test]
    fn new_v2_attaches_no_discriminator_or_settings() {
        let document = wrap(DocumentV2::String("hi".to_owned()));
        assert_eq!(document.discriminator(), None);
        assert!(document.settings().is_none());
        assert_eq!(document.document().as_string(), Some("hi"));
        assert_eq!(
            document.into_document(),
            DocumentV2::String("hi".to_owned())
        );
        let from: DiscriminatedDocument<DocumentV2> = DocumentV2::Bool(true).into();
        assert_eq!(from.document(), &DocumentV2::Bool(true));
    }

    #[test]
    fn try_with_discriminator_validates_v2_documents_too() {
        let document = wrap(DocumentV2::Null)
            .try_with_discriminator("com.example#Shape")
            .unwrap();
        assert_eq!(document.discriminator(), Some("com.example#Shape"));
        assert!(matches!(
            wrap(DocumentV2::Null).try_with_discriminator("Shape"),
            Err(DocumentError::InvalidInput { .. })
        ));
    }

    #[test]
    fn equality_ignores_settings() {
        let document = || {
            wrap(DocumentV2::Blob(b"x".to_vec()))
                .try_with_discriminator("com.example#A")
                .unwrap()
        };
        assert_eq!(document(), document().with_settings(settings()));
        assert_ne!(
            document(),
            wrap(DocumentV2::Blob(b"y".to_vec()))
                .try_with_discriminator("com.example#A")
                .unwrap()
        );
    }

    #[test]
    fn as_blob_returns_native_blobs_without_settings() {
        let document = wrap(DocumentV2::Blob(b"raw".to_vec()));
        assert!(matches!(document.as_blob().unwrap(), Cow::Borrowed(b"raw")));
        // Settings don't apply to a value that is already a blob.
        let document = document.with_settings(settings());
        assert_eq!(document.as_blob().unwrap().as_ref(), b"raw");
    }

    #[test]
    fn as_blob_decodes_strings() {
        let document = wrap(DocumentV2::String("YWJjZA==".to_owned()));
        assert_eq!(document.as_blob().unwrap().as_ref(), b"abcd");

        let document = document.with_settings(settings());
        assert_eq!(document.as_blob().unwrap().as_ref(), b"YWJjZA==");

        let document = wrap(DocumentV2::String("not base64!".to_owned()));
        assert!(matches!(
            document.as_blob(),
            Err(DocumentError::InvalidInput { .. })
        ));
    }

    #[test]
    fn as_blob_rejects_other_variants() {
        let document = wrap(DocumentV2::Number(Number::PosInt(1)));
        assert!(matches!(
            document.as_blob(),
            Err(DocumentError::TypeMismatch { .. })
        ));
    }

    #[test]
    fn as_timestamp_returns_native_timestamps_without_settings() {
        let timestamp = DateTime::from_secs_f64(1234.5);
        let document = wrap(DocumentV2::Timestamp(timestamp));
        assert_eq!(document.as_timestamp().unwrap(), timestamp);
        let document = document.with_settings(settings());
        assert_eq!(document.as_timestamp().unwrap(), timestamp);
    }

    #[test]
    fn as_timestamp_converts_numbers_and_strings() {
        let number = wrap(DocumentV2::Number(Number::Float(1234.5)));
        assert_eq!(
            number.as_timestamp().unwrap(),
            DateTime::from_secs_f64(1234.5)
        );
        let number = wrap(DocumentV2::Number(Number::NegInt(-5)));
        assert_eq!(number.as_timestamp().unwrap(), DateTime::from_secs(-5));
        assert_eq!(
            number.with_settings(settings()).as_timestamp().unwrap(),
            DateTime::from_secs(1)
        );

        let string = wrap(DocumentV2::String("1970-01-01T00:00:10Z".to_owned()));
        assert_eq!(string.as_timestamp().unwrap(), DateTime::from_secs(10));
        assert_eq!(
            string.with_settings(settings()).as_timestamp().unwrap(),
            DateTime::from_secs(0)
        );
    }

    #[test]
    fn as_timestamp_rejects_unconvertible_values() {
        let error = |document| wrap(document).as_timestamp().unwrap_err();
        for document in [
            DocumentV2::Number(Number::PosInt(u64::MAX)),
            DocumentV2::Number(Number::Float(f64::NAN)),
            DocumentV2::String("yesterday".to_owned()),
        ] {
            assert!(matches!(
                error(document),
                DocumentError::InvalidInput { .. }
            ));
        }
        assert!(matches!(
            error(DocumentV2::Bool(true)),
            DocumentError::TypeMismatch { .. }
        ));
    }
}
