/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Timestamp-format resolution shared by request and response bindings.

use aws_smithy_schema::Schema;
use aws_smithy_types::date_time::Format;

/// Where a bound value travels; determines its default timestamp format.
#[derive(Copy, Clone, Debug)]
pub(crate) enum BindingLocation {
    Header,
    Query,
    Label,
}

/// Resolves the value schema first, then the bound member, then the location default.
///
/// For lists, `value_schema` is the element schema, including traits inherited from its target.
pub(crate) fn resolve_timestamp_format(
    value_schema: &Schema<'_>,
    member: &Schema<'_>,
    location: BindingLocation,
) -> Format {
    let default = match location {
        BindingLocation::Header => Format::HttpDate,
        BindingLocation::Query | BindingLocation::Label => Format::DateTime,
    };
    timestamp_format_or(value_schema, timestamp_format_or(member, default))
}

/// Resolves an explicit format, retaining the prepared binding format when it is absent.
pub(crate) fn timestamp_format_or(schema: &Schema<'_>, default: Format) -> Format {
    use aws_smithy_schema::traits::TimestampFormat as SchemaFormat;
    match schema.timestamp_format().map(|trait_| trait_.format()) {
        Some(SchemaFormat::EpochSeconds) => Format::EpochSeconds,
        Some(SchemaFormat::HttpDate) => Format::HttpDate,
        Some(SchemaFormat::DateTime) => Format::DateTime,
        Some(_) | None => default,
    }
}
