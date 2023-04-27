/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::*;
use serde::ser::SerializeTuple;

impl serde::Serialize for DateTime {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if serializer.is_human_readable() {
            match self.fmt(Format::DateTime) {
                Ok(val) => serializer.serialize_str(&val),
                Err(e) => Err(serde::ser::Error::custom(e)),
            }
        } else {
            let mut tup_ser = serializer.serialize_tuple(2)?;
            tup_ser.serialize_element(&self.seconds)?;
            tup_ser.serialize_element(&self.subsecond_nanos)?;
            tup_ser.end()
        }
    }
}
