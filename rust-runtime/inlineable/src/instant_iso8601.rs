/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use serde::de::{Error, Unexpected};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use smithy_types::instant::Format;
use smithy_types::Instant;

pub struct InstantIso8601(pub Instant);

impl Serialize for InstantIso8601 {
    fn serialize<S>(&self, serializer: S) -> Result<<S as Serializer>::Ok, <S as Serializer>::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.fmt(Format::DateTime))
    }
}

impl<'de> Deserialize<'de> for InstantIso8601 {
    fn deserialize<D>(deserializer: D) -> Result<Self, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let ts = <&str>::deserialize(deserializer)?;
        Ok(InstantIso8601(
            Instant::from_str(ts, Format::DateTime)
                .map_err(|_| D::Error::invalid_value(Unexpected::Str(ts), &"valid iso8601 date"))?,
        ))
    }
}
