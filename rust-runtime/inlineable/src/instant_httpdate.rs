use serde::de::{Error, Unexpected};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use smithy_types::instant::Format;
use smithy_types::Instant;

pub struct InstantHttpDate(pub Instant);

impl Serialize for InstantHttpDate {
    fn serialize<S>(&self, serializer: S) -> Result<<S as Serializer>::Ok, <S as Serializer>::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.fmt(Format::HttpDate))
    }
}

impl<'de> Deserialize<'de> for InstantHttpDate {
    fn deserialize<D>(deserializer: D) -> Result<Self, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let ts = <&str>::deserialize(deserializer)?;
        Ok(InstantHttpDate(
            Instant::from_str(ts, Format::HttpDate)
                .map_err(|_| D::Error::invalid_value(Unexpected::Str(ts), &"valid http date"))?,
        ))
    }
}
