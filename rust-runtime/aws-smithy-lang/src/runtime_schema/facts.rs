/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Decoding of the traits that runtime schemas carry.
//!
//! A `Model` guarantees that references resolve, not that every trait value is well formed,
//! so each schema-relevant trait is validated once here, before anything is allocated. The
//! decoded facts borrow their strings from the model.

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticSet};
use crate::node::Node;
use crate::shape::Traits;
use crate::ShapeId;
use aws_smithy_schema::traits::{HttpTrait, TimestampFormat};
use aws_smithy_schema::Schema;

/// The schema-relevant traits applied to one shape or member.
///
/// Mirrors the known trait fields of `Schema` and `knownTraitSetter` in `SchemaGenerator.kt`.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct TraitFacts<'m> {
    pub(super) sensitive: bool,
    pub(super) json_name: Option<&'m str>,
    pub(super) timestamp_format: Option<TimestampFormat>,
    pub(super) xml_name: Option<&'m str>,
    pub(super) xml_attribute: bool,
    pub(super) xml_flattened: bool,
    pub(super) xml_namespace: Option<(&'m str, Option<&'m str>)>,
    pub(super) media_type: Option<&'m str>,
    /// Model-borrowed. Interned by [`TraitFacts::apply`], because a header name must be
    /// `'static`; see `with_http_header`.
    pub(super) http_header: Option<&'m str>,
    pub(super) http_label: bool,
    pub(super) http_payload: bool,
    pub(super) http_prefix_headers: Option<&'m str>,
    pub(super) http_query: Option<&'m str>,
    pub(super) http_query_params: bool,
    pub(super) http_response_code: bool,
    pub(super) streaming: bool,
    pub(super) event_header: bool,
    pub(super) event_payload: bool,
    pub(super) host_label: bool,
}

impl<'m> TraitFacts<'m> {
    /// Decodes the schema-relevant traits of `owner`, reporting malformed values.
    pub(super) fn decode(
        owner: &ShapeId,
        traits: Traits<'m>,
        diagnostics: &mut DiagnosticSet,
    ) -> Self {
        let mut facts = Self::default();
        for applied in traits.iter() {
            let node = applied.value();
            let mut decoder = Decoder {
                owner,
                id: applied.id(),
                node,
                diagnostics: &mut *diagnostics,
            };
            match applied.id().as_str() {
                "smithy.api#sensitive" => facts.sensitive = decoder.annotation(),
                "smithy.api#jsonName" => facts.json_name = decoder.string(),
                "smithy.api#timestampFormat" => facts.timestamp_format = decoder.timestamp_format(),
                "smithy.api#xmlName" => facts.xml_name = decoder.string(),
                "smithy.api#xmlAttribute" => facts.xml_attribute = decoder.annotation(),
                "smithy.api#xmlFlattened" => facts.xml_flattened = decoder.annotation(),
                "smithy.api#xmlNamespace" => facts.xml_namespace = decoder.xml_namespace(),
                "smithy.api#mediaType" => facts.media_type = decoder.string(),
                "smithy.api#httpHeader" => facts.http_header = decoder.string(),
                "smithy.api#httpLabel" => facts.http_label = decoder.annotation(),
                "smithy.api#httpPayload" => facts.http_payload = decoder.annotation(),
                "smithy.api#httpPrefixHeaders" => facts.http_prefix_headers = decoder.string(),
                "smithy.api#httpQuery" => facts.http_query = decoder.string(),
                "smithy.api#httpQueryParams" => facts.http_query_params = decoder.annotation(),
                "smithy.api#httpResponseCode" => facts.http_response_code = decoder.annotation(),
                "smithy.api#streaming" => facts.streaming = decoder.annotation(),
                "smithy.api#eventHeader" => facts.event_header = decoder.annotation(),
                "smithy.api#eventPayload" => facts.event_payload = decoder.annotation(),
                "smithy.api#hostLabel" => facts.host_label = decoder.annotation(),
                _ => {}
            }
        }
        facts
    }

    /// Whether the member is bound to part of an HTTP message other than the body.
    pub(super) fn has_http_binding(&self) -> bool {
        self.http_header.is_some()
            || self.http_query.is_some()
            || self.http_label
            || self.http_prefix_headers.is_some()
            || self.http_query_params
            || self.http_payload
    }

    /// Sets these traits on `schema`.
    pub(super) fn apply<'a>(&self, mut schema: Schema<'a>) -> Schema<'a>
    where
        'm: 'a,
    {
        if self.sensitive {
            schema = schema.with_sensitive();
        }
        if let Some(value) = self.json_name {
            schema = schema.with_json_name(value);
        }
        if let Some(format) = self.timestamp_format {
            schema = schema.with_timestamp_format(format);
        }
        if let Some(value) = self.xml_name {
            schema = schema.with_xml_name(value);
        }
        if self.xml_attribute {
            schema = schema.with_xml_attribute();
        }
        if self.xml_flattened {
            schema = schema.with_xml_flattened();
        }
        if let Some((uri, prefix)) = self.xml_namespace {
            schema = schema.with_xml_namespace(uri, prefix);
        }
        if let Some(value) = self.media_type {
            schema = schema.with_media_type(value);
        }
        if let Some(value) = self.http_header {
            // `with_http_header` requires `&'static str`, so a model-borrowed name cannot be
            // passed here by mistake: it is interned (deduplicated, never freed) instead.
            schema = schema.with_http_header(aws_smithy_schema::intern_header_name(value));
        }
        if self.http_label {
            schema = schema.with_http_label();
        }
        if self.http_payload {
            schema = schema.with_http_payload();
        }
        if let Some(value) = self.http_prefix_headers {
            schema = schema.with_http_prefix_headers(value);
        }
        if let Some(value) = self.http_query {
            schema = schema.with_http_query(value);
        }
        if self.http_query_params {
            schema = schema.with_http_query_params();
        }
        if self.http_response_code {
            schema = schema.with_http_response_code();
        }
        if self.streaming {
            schema = schema.with_streaming();
        }
        if self.event_header {
            schema = schema.with_event_header();
        }
        if self.event_payload {
            schema = schema.with_event_payload();
        }
        if self.host_label {
            schema = schema.with_host_label();
        }
        schema
    }
}

/// The schema-relevant traits applied to an operation.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct OperationFacts<'m> {
    /// `@http` method, URI, and status code (`None` for the default, 200).
    pub(super) http: Option<(&'m str, &'m str, Option<u16>)>,
    /// `aws.customizations#s3UnwrappedXmlOutput`.
    pub(super) xml_unwrapped_output: bool,
}

impl<'m> OperationFacts<'m> {
    pub(super) fn decode(
        owner: &ShapeId,
        traits: Traits<'m>,
        diagnostics: &mut DiagnosticSet,
    ) -> Self {
        let mut facts = Self::default();
        for applied in traits.iter() {
            let mut decoder = Decoder {
                owner,
                id: applied.id(),
                node: applied.value(),
                diagnostics: &mut *diagnostics,
            };
            match applied.id().as_str() {
                "smithy.api#http" => facts.http = decoder.http(),
                "aws.customizations#s3UnwrappedXmlOutput" => {
                    facts.xml_unwrapped_output = decoder.annotation()
                }
                _ => {}
            }
        }
        facts
    }

    /// The `@http` trait as a schema trait.
    pub(super) fn http_trait(&self) -> Option<HttpTrait<'m>> {
        self.http
            .map(|(method, uri, code)| HttpTrait::new(method, uri, code))
    }
}

/// Validates one trait value, reporting a located diagnostic when it is malformed.
struct Decoder<'d, 'm> {
    owner: &'d ShapeId,
    id: &'m ShapeId,
    node: &'m Node,
    diagnostics: &'d mut DiagnosticSet,
}

impl<'m> Decoder<'_, 'm> {
    fn report(&mut self, node: &Node, message: impl std::fmt::Display) {
        self.diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::InvalidTrait,
                format!("trait `{}` on `{}` {message}", self.id, self.owner),
            )
            .with_source_location(node.source_location().clone())
            .with_shape_id(self.owner.clone()),
        );
    }

    /// An annotation trait is an object (normally empty). `true` is the deprecated Smithy 1.0
    /// form, which Smithy still accepts.
    fn annotation(&mut self) -> bool {
        let node = self.node;
        if node.as_object().is_some() || node.as_bool() == Some(true) {
            true
        } else {
            self.report(
                node,
                format_args!("must be an object, found {}", node.type_name()),
            );
            false
        }
    }

    fn string(&mut self) -> Option<&'m str> {
        let node = self.node;
        self.string_at(node)
    }

    fn string_at(&mut self, node: &'m Node) -> Option<&'m str> {
        let value = node.as_str();
        if value.is_none() {
            self.report(
                node,
                format_args!("must be a string, found {}", node.type_name()),
            );
        }
        value
    }

    fn timestamp_format(&mut self) -> Option<TimestampFormat> {
        let node = self.node;
        match self.string_at(node)? {
            "epoch-seconds" => Some(TimestampFormat::EpochSeconds),
            "date-time" => Some(TimestampFormat::DateTime),
            "http-date" => Some(TimestampFormat::HttpDate),
            other => {
                self.report(
                    node,
                    format_args!(
                        "must be one of `date-time`, `epoch-seconds`, or `http-date`, found `{other}`"
                    ),
                );
                None
            }
        }
    }

    /// Looks up a property of an object-valued trait, reporting a non-object value or a missing
    /// required property.
    fn property(&mut self, name: &str, required: bool) -> Result<Option<&'m Node>, ()> {
        let node = self.node;
        let Some(object) = node.as_object() else {
            self.report(
                node,
                format_args!("must be an object, found {}", node.type_name()),
            );
            return Err(());
        };
        let value = object.get(name);
        if value.is_none() && required {
            self.report(node, format_args!("is missing required property `{name}`"));
            return Err(());
        }
        Ok(value)
    }

    fn xml_namespace(&mut self) -> Option<(&'m str, Option<&'m str>)> {
        let uri = self.property("uri", true).ok()??;
        let prefix = self.property("prefix", false).ok()?;
        let uri = self.string_at(uri);
        let prefix = match prefix {
            Some(prefix) => Some(self.string_at(prefix)?),
            None => None,
        };
        Some((uri?, prefix))
    }

    fn http(&mut self) -> Option<(&'m str, &'m str, Option<u16>)> {
        let method = self.property("method", true).ok()??;
        let uri = self.property("uri", true).ok()??;
        let code = self.property("code", false).ok()?;
        let method = self.string_at(method);
        let uri = self.string_at(uri);
        let code = match code {
            None => None,
            Some(node) => match node.as_number().and_then(|n| n.as_u64()) {
                Some(code @ 100..=999) => Some(code as u16),
                _ => {
                    self.report(
                        node,
                        "property `code` must be an HTTP status code from 100 to 999",
                    );
                    return None;
                }
            },
        };
        // Generated schemas record the default status code as `None`.
        Some((method?, uri?, code.filter(|&code| code != 200)))
    }
}
