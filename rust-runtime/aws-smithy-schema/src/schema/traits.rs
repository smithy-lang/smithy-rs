/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Typed runtime representations of Smithy serialization traits.
//!
//! These types allow type-safe downcasting from `&dyn Trait` via `as_any()`,
//! enabling protocol implementations to read trait values without string-matching
//! on Shape IDs.

use crate::{ShapeId, Trait};
use std::any::Any;

macro_rules! annotation_trait {
    ($(#[$meta:meta])* $name:ident, $id:expr) => {
        $(#[$meta])*
        #[derive(Debug, Clone)]
        pub struct $name;

        impl $name {
            /// The Shape ID of this trait.
            pub const ID: &'static str = $id;
        }

        impl Trait for $name {
            fn trait_id(&self) -> &ShapeId {
                static ID: std::sync::LazyLock<ShapeId> =
                    std::sync::LazyLock::new(|| ShapeId::new($name::ID));
                &ID
            }

            fn as_any(&self) -> &dyn Any {
                self
            }
        }
    };
}

macro_rules! string_trait {
    ($(#[$meta:meta])* $name:ident, $id:expr) => {
        $(#[$meta])*
        #[derive(Debug, Clone)]
        pub struct $name {
            value: String,
        }

        impl $name {
            /// The Shape ID of this trait.
            pub const ID: &'static str = $id;

            /// Creates a new instance.
            pub fn new(value: impl Into<String>) -> Self {
                Self { value: value.into() }
            }

            /// Returns the trait value.
            pub fn value(&self) -> &str {
                &self.value
            }
        }

        impl Trait for $name {
            fn trait_id(&self) -> &ShapeId {
                static ID: std::sync::LazyLock<ShapeId> =
                    std::sync::LazyLock::new(|| ShapeId::new($name::ID));
                &ID
            }

            fn as_any(&self) -> &dyn Any {
                self
            }
        }
    };
}

// --- Serialization & Protocol traits ---

string_trait!(
    /// The `@jsonName` trait — overrides the JSON key for a member.
    JsonNameTrait,
    "smithy.api#jsonName"
);

string_trait!(
    /// The `@xmlName` trait — overrides the XML element name.
    XmlNameTrait,
    "smithy.api#xmlName"
);

string_trait!(
    /// The `@mediaType` trait — specifies the media type of a blob/string.
    MediaTypeTrait,
    "smithy.api#mediaType"
);

annotation_trait!(
    /// The `@xmlAttribute` trait — serializes a member as an XML attribute.
    XmlAttributeTrait,
    "smithy.api#xmlAttribute"
);

annotation_trait!(
    /// The `@xmlFlattened` trait — removes the wrapping element for lists/maps in XML.
    XmlFlattenedTrait,
    "smithy.api#xmlFlattened"
);

// --- Timestamp ---

/// The `@timestampFormat` trait — specifies the serialization format for timestamps.
#[derive(Debug, Clone)]
pub struct TimestampFormatTrait {
    format: TimestampFormat,
}

/// Timestamp serialization formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampFormat {
    /// Epoch seconds (e.g. `1515531081.123`).
    EpochSeconds,
    /// RFC 3339 date-time (e.g. `2018-01-09T18:47:00Z`).
    DateTime,
    /// RFC 7231 HTTP date (e.g. `Tue, 09 Jan 2018 18:47:00 GMT`).
    HttpDate,
}

impl TimestampFormatTrait {
    /// The Shape ID of this trait.
    pub const ID: &'static str = "smithy.api#timestampFormat";

    /// Creates a new instance from a format string.
    pub fn new(format: &str) -> Self {
        Self {
            format: match format {
                "epoch-seconds" => TimestampFormat::EpochSeconds,
                "date-time" => TimestampFormat::DateTime,
                "http-date" => TimestampFormat::HttpDate,
                other => panic!("unknown timestamp format: {other}"),
            },
        }
    }

    /// Returns the timestamp format.
    pub fn format(&self) -> TimestampFormat {
        self.format
    }
}

impl Trait for TimestampFormatTrait {
    fn trait_id(&self) -> &ShapeId {
        static ID: std::sync::LazyLock<ShapeId> =
            std::sync::LazyLock::new(|| ShapeId::new(TimestampFormatTrait::ID));
        &ID
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// --- HTTP binding traits ---

string_trait!(
    /// The `@httpHeader` trait — binds a member to an HTTP header.
    HttpHeaderTrait,
    "smithy.api#httpHeader"
);

string_trait!(
    /// The `@httpQuery` trait — binds a member to a query parameter.
    HttpQueryTrait,
    "smithy.api#httpQuery"
);

string_trait!(
    /// The `@httpPrefixHeaders` trait — binds a map to prefixed HTTP headers.
    HttpPrefixHeadersTrait,
    "smithy.api#httpPrefixHeaders"
);

annotation_trait!(
    /// The `@httpLabel` trait — binds a member to a URI label.
    HttpLabelTrait,
    "smithy.api#httpLabel"
);

annotation_trait!(
    /// The `@httpPayload` trait — binds a member to the HTTP body.
    HttpPayloadTrait,
    "smithy.api#httpPayload"
);

annotation_trait!(
    /// The `@httpQueryParams` trait — binds a map to query parameters.
    HttpQueryParamsTrait,
    "smithy.api#httpQueryParams"
);

annotation_trait!(
    /// The `@httpResponseCode` trait — binds a member to the HTTP status code.
    HttpResponseCodeTrait,
    "smithy.api#httpResponseCode"
);

// --- Streaming traits ---

annotation_trait!(
    /// The `@streaming` trait — marks a blob or union as streaming.
    StreamingTrait,
    "smithy.api#streaming"
);

annotation_trait!(
    /// The `@eventHeader` trait — binds a member to an event stream header.
    EventHeaderTrait,
    "smithy.api#eventHeader"
);

annotation_trait!(
    /// The `@eventPayload` trait — binds a member to an event stream payload.
    EventPayloadTrait,
    "smithy.api#eventPayload"
);

// --- Documentation / behavior traits ---

annotation_trait!(
    /// The `@sensitive` trait — marks data as sensitive for logging redaction.
    SensitiveTrait,
    "smithy.api#sensitive"
);

// --- Endpoint traits ---

annotation_trait!(
    /// The `@hostLabel` trait — binds a member to a host prefix label.
    HostLabelTrait,
    "smithy.api#hostLabel"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downcast_json_name() {
        let t: Box<dyn Trait> = Box::new(JsonNameTrait::new("userName"));
        assert_eq!(t.trait_id().as_str(), "smithy.api#jsonName");
        let json_name = t.as_any().downcast_ref::<JsonNameTrait>().unwrap();
        assert_eq!(json_name.value(), "userName");
    }

    #[test]
    fn downcast_sensitive() {
        let t: Box<dyn Trait> = Box::new(SensitiveTrait);
        assert_eq!(t.trait_id().as_str(), "smithy.api#sensitive");
        assert!(t.as_any().downcast_ref::<SensitiveTrait>().is_some());
    }

    #[test]
    fn timestamp_format_parsing() {
        assert_eq!(
            TimestampFormatTrait::new("epoch-seconds").format(),
            TimestampFormat::EpochSeconds
        );
        assert_eq!(
            TimestampFormatTrait::new("date-time").format(),
            TimestampFormat::DateTime
        );
        assert_eq!(
            TimestampFormatTrait::new("http-date").format(),
            TimestampFormat::HttpDate
        );
    }
}
