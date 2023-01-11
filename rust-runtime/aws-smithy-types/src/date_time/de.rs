use super::*;
use serde::de::Visitor;
use serde::Deserialize;
struct DateTimeVisitor;

enum VisitorState {
    Second,
    SubsecondNanos,
    Unexpected,
}

struct NonHumanReadableDateTimeVisitor {
    state: VisitorState,
    seconds: i64,
    subsecond_nanos: u32,
}

impl<'de> Visitor<'de> for DateTimeVisitor {
    type Value = DateTime;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("expected RFC-3339 Date Time")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match DateTime::from_str(v, Format::DateTime) {
            Ok(e) => Ok(e),
            Err(e) => Err(serde::de::Error::custom(e)),
        }
    }
}

impl<'de> Visitor<'de> for NonHumanReadableDateTimeVisitor {
    type Value = Self;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("expected (i64, u32)")
    }

    fn visit_i64<E>(mut self, v: i64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match self.state {
            VisitorState::Second => {
                self.seconds = v;
                self.state = VisitorState::SubsecondNanos;
            }
            _ => return Err(serde::de::Error::custom("`seconds` value must be i64")),
        };
        Ok(self)
    }
    fn visit_u32<E>(mut self, v: u32) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match self.state {
            VisitorState::SubsecondNanos => {
                self.subsecond_nanos = v;
                self.state = VisitorState::Unexpected;
            }
            _ => {
                return Err(serde::de::Error::custom(
                    "`subsecond_nanos` value must be u32",
                ))
            }
        };
        Ok(self)
    }
}

impl<'de> Deserialize<'de> for DateTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            deserializer.deserialize_str(DateTimeVisitor)
        } else {
            let visitor = NonHumanReadableDateTimeVisitor {
                state: VisitorState::Second,
                seconds: 0,
                subsecond_nanos: 0,
            };
            let visitor = deserializer.deserialize_tuple(2, visitor)?;
            Ok(DateTime {
                seconds: visitor.seconds,
                subsecond_nanos: visitor.subsecond_nanos,
            })
        }
    }
}
