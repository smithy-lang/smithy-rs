/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Typed HTTP trait values stored in a schema's generic trait map.

use aws_smithy_schema::{Schema, ShapeId, Trait};
use std::any::Any;

/// XML-specific information attached to a structured HTTP payload member.
///
/// The payload schema retains the target's metadata and members, with its XML
/// root name overridden by the payload member. Other protocols use the ordinary
/// target schema. Each generated payload schema owns its codec cache.
#[derive(Debug, Clone, Copy)]
pub struct XmlMetadata {
    payload_schema: &'static Schema<'static>,
}

impl XmlMetadata {
    /// The private server metadata trait's Shape ID.
    pub const TRAIT_ID: ShapeId<'static> = aws_smithy_schema::shape_id!("smithy.rust.server.xml", "metadata");

    /// Creates metadata referring to a prebuilt XML payload schema.
    pub const fn new(payload_schema: &'static Schema<'static>) -> Self {
        Self { payload_schema }
    }

    /// Returns the target schema with the payload member's XML root name.
    pub const fn payload_schema(&self) -> &'static Schema<'static> {
        self.payload_schema
    }

    /// Reads XML metadata from a payload member's trait map.
    pub fn from_schema<'s>(schema: &'s Schema<'_>) -> Option<&'s Self> {
        schema.traits()?.get(&Self::TRAIT_ID)?.as_any().downcast_ref()
    }
}

impl Trait for XmlMetadata {
    fn trait_id(&self) -> &ShapeId<'static> {
        &Self::TRAIT_ID
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The `smithy.api#httpError` trait on an error structure.
#[derive(Debug, Clone, Copy)]
pub struct HttpErrorTrait {
    code: u16,
}

impl HttpErrorTrait {
    /// The Shape ID for this trait.
    pub const TRAIT_ID: ShapeId<'static> = aws_smithy_schema::shape_id!("smithy.api", "httpError");

    /// Creates an HTTP error trait with the modeled status code.
    pub const fn new(code: u16) -> Self {
        Self { code }
    }

    /// Returns the modeled HTTP error status code.
    pub const fn code(&self) -> u16 {
        self.code
    }

    /// Reads this typed trait from a schema's fallback map.
    ///
    /// Initializes the lazy trait map on first access. Returns `None` if the
    /// schema has no map or the entry is absent or has a different Rust type.
    pub fn from_schema<'s>(schema: &'s Schema<'_>) -> Option<&'s Self> {
        schema.traits()?.get(&Self::TRAIT_ID)?.as_any().downcast_ref()
    }
}

impl Trait for HttpErrorTrait {
    fn trait_id(&self) -> &ShapeId<'static> {
        &Self::TRAIT_ID
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The `HttpConfiguration` members every built-in protocol trait shares (Smithy's
/// `aws.protocols#HttpConfiguration` mixin).
///
/// Recorded as written in the model; nothing in the server acts on them yet. HTTP version
/// negotiation comes from the server's own configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpConfiguration {
    http: Option<Vec<String>>,
    event_stream_http: Option<Vec<String>>,
}

impl HttpConfiguration {
    /// Sets the `http` member.
    ///
    /// Public so protocol traits defined outside this crate (Smithy protocols declare these
    /// members per protocol, by the `aws.protocols#HttpConfiguration` mixin convention) can embed
    /// this struct the way the built-in protocol traits do.
    pub fn with_http(mut self, versions: Vec<String>) -> Self {
        self.http = Some(versions);
        self
    }

    /// Sets the `eventStreamHttp` member.
    pub fn with_event_stream_http(mut self, versions: Vec<String>) -> Self {
        self.event_stream_http = Some(versions);
        self
    }

    /// The priority-ordered HTTP versions the service supports, if the model lists them.
    pub fn http(&self) -> Option<&[String]> {
        self.http.as_deref()
    }

    /// The priority-ordered HTTP versions event streams require, if the model lists them.
    /// When unset, Smithy defines the value as [`http`](Self::http).
    pub fn event_stream_http(&self) -> Option<&[String]> {
        self.event_stream_http.as_deref()
    }
}

macro_rules! protocol_trait {
    ($(#[$doc:meta])* $name:ident, $namespace:literal, $trait_name:literal $(, $field:ident: $ty:ty, $setter:ident, $getter:ident, $field_doc:literal)?) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Default, PartialEq, Eq)]
        pub struct $name {
            http_configuration: HttpConfiguration,
            $($field: $ty,)?
        }

        impl $name {
            /// The Shape ID for this trait.
            pub const TRAIT_ID: ShapeId<'static> = aws_smithy_schema::shape_id!($namespace, $trait_name);

            /// Creates the trait with no members set.
            pub fn new() -> Self {
                Self::default()
            }

            /// Sets the `http` member.
            pub fn with_http(mut self, versions: Vec<String>) -> Self {
                self.http_configuration.http = Some(versions);
                self
            }

            /// Sets the `eventStreamHttp` member.
            pub fn with_event_stream_http(mut self, versions: Vec<String>) -> Self {
                self.http_configuration.event_stream_http = Some(versions);
                self
            }

            /// Returns the `http` and `eventStreamHttp` members.
            pub fn http_configuration(&self) -> &HttpConfiguration {
                &self.http_configuration
            }

            $(
                #[doc = concat!("Sets the `", $field_doc, "` member.")]
                pub fn $setter(mut self, value: $ty) -> Self {
                    self.$field = value;
                    self
                }

                #[doc = concat!("Returns the `", $field_doc, "` member.")]
                pub fn $getter(&self) -> $ty {
                    self.$field
                }
            )?

            /// Reads this trait from the traits recorded on a service schema.
            pub fn from_service<'s>(service: &'s crate::schema::ServiceSchema<'_>) -> Option<&'s Self> {
                service.traits()?.get(&Self::TRAIT_ID)?.as_any().downcast_ref()
            }
        }

        impl Trait for $name {
            fn trait_id(&self) -> &ShapeId<'static> {
                &Self::TRAIT_ID
            }

            fn as_any(&self) -> &dyn Any {
                self
            }
        }
    };
}

protocol_trait!(
    /// The `aws.protocols#awsJson1_0` protocol trait on a service.
    AwsJson1_0Trait, "aws.protocols", "awsJson1_0"
);
protocol_trait!(
    /// The `aws.protocols#awsJson1_1` protocol trait on a service.
    AwsJson1_1Trait, "aws.protocols", "awsJson1_1"
);
protocol_trait!(
    /// The `aws.protocols#restJson1` protocol trait on a service.
    RestJson1Trait, "aws.protocols", "restJson1"
);
protocol_trait!(
    /// The `aws.protocols#restXml` protocol trait on a service.
    ///
    /// `noErrorWrapping` (deprecated in Smithy) drops the `<ErrorResponse>` envelope around
    /// errors: a received exception frame is then rooted at `<Error>`.
    RestXmlTrait, "aws.protocols", "restXml",
    no_error_wrapping: bool, with_no_error_wrapping, no_error_wrapping, "noErrorWrapping"
);
protocol_trait!(
    /// The `smithy.protocols#rpcv2Cbor` protocol trait on a service.
    Rpcv2CborTrait, "smithy.protocols", "rpcv2Cbor"
);

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_schema::{shape_id, ShapeType, StringTrait, TraitMap};
    use std::sync::LazyLock;

    static ERROR_TRAITS: LazyLock<TraitMap> = LazyLock::new(|| {
        let mut map = TraitMap::new();
        map.insert(Box::new(StringTrait::new(shape_id!("smithy.api", "error"), "client")));
        map.insert(Box::new(HttpErrorTrait::new(404)));
        map
    });

    static ERROR: Schema<'static> =
        Schema::new_struct(shape_id!("example", "Missing"), ShapeType::Structure, &[]).with_traits(&ERROR_TRAITS);

    static SERVICE_TRAITS: LazyLock<TraitMap> = LazyLock::new(|| {
        let mut map = TraitMap::new();
        map.insert(Box::new(
            RestXmlTrait::new()
                .with_http(vec!["h2".into(), "http/1.1".into()])
                .with_event_stream_http(vec!["h2".into()])
                .with_no_error_wrapping(true),
        ));
        map.insert(Box::new(RestJson1Trait::new()));
        map
    });

    static SERVICE: crate::schema::ServiceSchema<'static> = crate::schema::ServiceSchema::new(
        shape_id!("example", "Service"),
        None,
        &[RestXmlTrait::TRAIT_ID, RestJson1Trait::TRAIT_ID],
        &[],
    )
    .with_traits(&SERVICE_TRAITS);

    #[test]
    fn protocol_traits_are_read_from_the_service_schema() {
        let rest_xml = RestXmlTrait::from_service(&SERVICE).unwrap();
        assert!(rest_xml.no_error_wrapping());
        assert_eq!(
            rest_xml.http_configuration().http(),
            Some(&["h2".to_owned(), "http/1.1".to_owned()][..])
        );
        assert_eq!(
            rest_xml.http_configuration().event_stream_http(),
            Some(&["h2".to_owned()][..])
        );
        assert_eq!(rest_xml.trait_id().as_str(), "aws.protocols#restXml");

        // Members left unset in the model stay unset.
        let rest_json = RestJson1Trait::from_service(&SERVICE).unwrap();
        assert_eq!(rest_json.http_configuration(), &HttpConfiguration::default());
        assert_eq!(rest_json.http_configuration().http(), None);

        // Protocols the service does not declare, and services without traits, have none.
        assert!(Rpcv2CborTrait::from_service(&SERVICE).is_none());
        static BARE: crate::schema::ServiceSchema<'static> =
            crate::schema::ServiceSchema::new(shape_id!("example", "Bare"), None, &[], &[]);
        assert!(RestXmlTrait::from_service(&BARE).is_none());
        assert_eq!(AwsJson1_0Trait::TRAIT_ID.as_str(), "aws.protocols#awsJson1_0");
        assert_eq!(AwsJson1_1Trait::TRAIT_ID.as_str(), "aws.protocols#awsJson1_1");
        assert_eq!(Rpcv2CborTrait::TRAIT_ID.as_str(), "smithy.protocols#rpcv2Cbor");
    }

    #[test]
    fn typed_http_error_preserves_trait_identity() {
        let schema = &ERROR;
        assert!(schema.http().is_none());
        let traits = schema.traits().unwrap();
        assert!(traits.contains_fqn("smithy.api#error"));
        assert!(traits.contains_fqn("smithy.api#httpError"));
        assert_eq!(traits.len(), 2);
        let http_error = HttpErrorTrait::from_schema(schema).unwrap();
        assert_eq!(http_error.code(), 404);
        assert_eq!(http_error.trait_id().as_str(), "smithy.api#httpError");
    }

    #[test]
    fn absent_or_differently_typed_entries_return_none() {
        let plain = Schema::new(shape_id!("example", "Plain"), ShapeType::String);
        assert!(HttpErrorTrait::from_schema(&plain).is_none());
        let without_http_error: LazyLock<TraitMap> = LazyLock::new(|| {
            let mut map = TraitMap::new();
            map.insert(Box::new(StringTrait::new(shape_id!("smithy.api", "error"), "server")));
            map
        });
        assert!(HttpErrorTrait::from_schema(
            &Schema::new(shape_id!("example", "Plain"), ShapeType::String).with_traits(&without_http_error)
        )
        .is_none());
        let differently_typed: LazyLock<TraitMap> = LazyLock::new(|| {
            let mut map = TraitMap::new();
            map.insert(Box::new(StringTrait::new(HttpErrorTrait::TRAIT_ID, "404")));
            map
        });
        assert!(HttpErrorTrait::from_schema(&plain.with_traits(&differently_typed)).is_none());
    }
}
