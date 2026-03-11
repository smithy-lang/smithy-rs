/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Well-known trait [`ShapeId`]s relevant to serialization and deserialization.
//!
//! These are provided as statics so that codec and protocol implementations can
//! reference them without constructing a new [`ShapeId`] on each use.

use crate::ShapeId;

// Serialization and protocol traits
// https://smithy.io/2.0/spec/protocol-traits.html

/// `smithy.api#jsonName` — changes the serialized property name in JSON.
pub static JSON_NAME: ShapeId = crate::shape_id!("smithy.api", "jsonName");

/// `smithy.api#mediaType` — describes the contents of a blob or string.
pub static MEDIA_TYPE: ShapeId = crate::shape_id!("smithy.api", "mediaType");

/// `smithy.api#timestampFormat` — defines a custom timestamp serialization format.
pub static TIMESTAMP_FORMAT: ShapeId = crate::shape_id!("smithy.api", "timestampFormat");

/// `smithy.api#xmlAttribute` — serializes a member as an XML attribute.
pub static XML_ATTRIBUTE: ShapeId = crate::shape_id!("smithy.api", "xmlAttribute");

/// `smithy.api#xmlFlattened` — unwraps list/map values into the containing structure.
pub static XML_FLATTENED: ShapeId = crate::shape_id!("smithy.api", "xmlFlattened");

/// `smithy.api#xmlName` — changes the serialized XML element or attribute name.
pub static XML_NAME: ShapeId = crate::shape_id!("smithy.api", "xmlName");

/// `smithy.api#xmlNamespace` — adds an XML namespace to an element.
pub static XML_NAMESPACE: ShapeId = crate::shape_id!("smithy.api", "xmlNamespace");

// HTTP binding traits
// https://smithy.io/2.0/spec/http-bindings.html

/// `smithy.api#httpHeader` — binds a member to an HTTP header.
pub static HTTP_HEADER: ShapeId = crate::shape_id!("smithy.api", "httpHeader");

/// `smithy.api#httpLabel` — binds a member to a URI label.
pub static HTTP_LABEL: ShapeId = crate::shape_id!("smithy.api", "httpLabel");

/// `smithy.api#httpPayload` — binds a member to the HTTP payload.
pub static HTTP_PAYLOAD: ShapeId = crate::shape_id!("smithy.api", "httpPayload");

/// `smithy.api#httpPrefixHeaders` — binds a map to prefixed HTTP headers.
pub static HTTP_PREFIX_HEADERS: ShapeId = crate::shape_id!("smithy.api", "httpPrefixHeaders");

/// `smithy.api#httpQuery` — binds a member to a query string parameter.
pub static HTTP_QUERY: ShapeId = crate::shape_id!("smithy.api", "httpQuery");

/// `smithy.api#httpQueryParams` — binds a map to query string parameters.
pub static HTTP_QUERY_PARAMS: ShapeId = crate::shape_id!("smithy.api", "httpQueryParams");

/// `smithy.api#httpResponseCode` — binds a member to the HTTP response status code.
pub static HTTP_RESPONSE_CODE: ShapeId = crate::shape_id!("smithy.api", "httpResponseCode");
