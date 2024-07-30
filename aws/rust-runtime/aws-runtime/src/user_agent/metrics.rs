/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use once_cell::sync::Lazy;
use std::borrow::Cow;
use std::collections::HashMap;

macro_rules! iterable_enum {
    ($docs:tt, $enum_name:ident, $( $variant:ident ),*) => {
        #[derive(Clone, Debug, Eq, Hash, PartialEq)]
        #[non_exhaustive]
        #[doc = $docs]
        #[allow(missing_docs)] // for variants, not for the Enum itself
        pub enum $enum_name {
            $( $variant ),*
        }

        #[allow(dead_code)]
        impl $enum_name {
            pub(crate) fn iter() -> impl Iterator<Item = &'static $enum_name> {
                const VARIANTS: &[$enum_name] = &[
                    $( $enum_name::$variant ),*
                ];
                VARIANTS.iter()
            }
        }
    };
}

struct Base64Iterator {
    current: Vec<usize>,
    base64_chars: Vec<char>,
}

impl Base64Iterator {
    #[allow(dead_code)]
    fn new() -> Self {
        Base64Iterator {
            current: vec![0], // Start with the first character
            base64_chars: (b'A'..=b'Z') // 'A'-'Z'
                .chain(b'a'..=b'z') // 'a'-'z'
                .chain(b'0'..=b'9') // '0'-'9'
                .chain([b'+', b'-']) // '+' and '-'
                .map(|c| c as char)
                .collect(),
        }
    }

    fn increment(&mut self) {
        let mut i = 0;
        while i < self.current.len() {
            self.current[i] += 1;
            if self.current[i] < self.base64_chars.len() {
                // The value at current position hasn't reached 64
                return;
            }
            self.current[i] = 0;
            i += 1;
        }
        self.current.push(0); // Add new digit if all positions overflowed
    }
}

impl Iterator for Base64Iterator {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current.is_empty() {
            return None; // No more items
        }

        // Convert the current indices to characters
        let result: String = self
            .current
            .iter()
            .rev()
            .map(|&idx| self.base64_chars[idx])
            .collect();

        // Increment to the next value
        self.increment();
        Some(result)
    }
}

#[allow(dead_code)]
const MAX_METRICS_ID_NUMBER: usize = 350;

pub(super) static FEATURE_ID_TO_METRIC_VALUE: Lazy<HashMap<BusinessMetric, Cow<'static, str>>> =
    Lazy::new(|| {
        let mut m = HashMap::new();
        for (metric, value) in BusinessMetric::iter()
            .cloned()
            .zip(Base64Iterator::new())
            .take(MAX_METRICS_ID_NUMBER)
        {
            m.insert(metric, Cow::Owned(value));
        }
        m
    });

iterable_enum!(
    "Enumerates human readable identifiers for the features tracked by metrics",
    BusinessMetric,
    ResourceModel,
    Waiter,
    Paginator,
    RetryModeLegacy,
    RetryModeStandard,
    RetryModeAdaptive,
    S3Transfer,
    S3CryptoV1n,
    S3CryptoV2,
    S3ExpressBucket,
    S3AccessGrants,
    GzipRequestCompression,
    ProtocolRpcV2Cbor,
    EndpointOverride,
    AccountIdEndpoint,
    AccountIdModePreferred,
    AccountIdModeDisabled,
    AccountIdModeRequired,
    Sigv4aSigning,
    ResolvedAccountId
);

#[cfg(test)]
mod tests {
    use crate::user_agent::metrics::{
        Base64Iterator, FEATURE_ID_TO_METRIC_VALUE, MAX_METRICS_ID_NUMBER,
    };
    use crate::user_agent::BusinessMetric;
    use convert_case::{Boundary, Case, Casing};
    use std::collections::HashMap;
    use std::fmt::{Display, Formatter};

    impl Display for BusinessMetric {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            f.write_str(
                &format!("{:?}", self)
                    .as_str()
                    .from_case(Case::Pascal)
                    .with_boundaries(&[Boundary::DigitUpper, Boundary::LowerUpper])
                    .to_case(Case::ScreamingSnake),
            )
        }
    }

    #[test]
    fn feature_id_to_metric_value() {
        const EXPECTED: &str = r#"
{
  "RESOURCE_MODEL": "A",
  "WAITER": "B",
  "PAGINATOR": "C",
  "RETRY_MODE_LEGACY": "D",
  "RETRY_MODE_STANDARD": "E",
  "RETRY_MODE_ADAPTIVE": "F",
  "S3_TRANSFER": "G",
  "S3_CRYPTO_V1N": "H",
  "S3_CRYPTO_V2": "I",
  "S3_EXPRESS_BUCKET": "J",
  "S3_ACCESS_GRANTS": "K",
  "GZIP_REQUEST_COMPRESSION": "L",
  "PROTOCOL_RPC_V2_CBOR": "M",
  "ENDPOINT_OVERRIDE": "N",
  "ACCOUNT_ID_ENDPOINT": "O",
  "ACCOUNT_ID_MODE_PREFERRED": "P",
  "ACCOUNT_ID_MODE_DISABLED": "Q",
  "ACCOUNT_ID_MODE_REQUIRED": "R",
  "SIGV4A_SIGNING": "S",
  "RESOLVED_ACCOUNT_ID": "T"
}
        "#;

        let expected: HashMap<&str, &str> = serde_json::from_str(EXPECTED).unwrap();
        assert_eq!(expected.len(), FEATURE_ID_TO_METRIC_VALUE.len());

        for (feature_id, metric_value) in &*FEATURE_ID_TO_METRIC_VALUE {
            assert_eq!(
                expected.get(format!("{feature_id}").as_str()).unwrap(),
                metric_value,
            );
        }
    }

    #[test]
    fn test_base64_iter() {
        // 350 is the max number of metric IDs we support for now
        let ids: Vec<String> = Base64Iterator::new()
            .into_iter()
            .take(MAX_METRICS_ID_NUMBER)
            .collect();
        assert_eq!("A", ids[0]);
        assert_eq!("Z", ids[25]);
        assert_eq!("a", ids[26]);
        assert_eq!("z", ids[51]);
        assert_eq!("0", ids[52]);
        assert_eq!("9", ids[61]);
        assert_eq!("+", ids[62]);
        assert_eq!("-", ids[63]);
        assert_eq!("AA", ids[64]);
        assert_eq!("AB", ids[65]);
        assert_eq!("A-", ids[127]);
        assert_eq!("BA", ids[128]);
        assert_eq!("Ed", ids[349]);
    }
}
