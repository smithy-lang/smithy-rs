use crate::types::DateExt;
use chrono::{Date, Utc};
use ring::{
    digest::{self, Digest},
    hmac::{self, Key, Tag},
};

// HMAC
pub fn encode(s: String) -> Vec<u8> {
    let calculated = digest::digest(&digest::SHA256, s.as_bytes());
    calculated.as_ref().to_vec()
}

/// HashedPayload = Lowercase(HexEncode(Hash(requestPayload)))
pub fn encode_bytes_with_hex<B>(bytes: B) -> String
where
    B: AsRef<[u8]>,
{
    let digest: Digest = digest::digest(&digest::SHA256, bytes.as_ref());
    // no need to lower-case as in step six, as hex::encode
    // already returns a lower-cased string.
    hex::encode(digest)
}

pub fn calculate_signature(signing_key: Tag, string_to_sign: &[u8]) -> String {
    let s_key = Key::new(hmac::HMAC_SHA256, signing_key.as_ref());
    let tag = hmac::sign(&s_key, string_to_sign);

    hex::encode(tag)
}

// kSecret = your secret access key
// kDate = HMAC("AWS4" + kSecret, Date)
// kRegion = HMAC(kDate, Region)
// kService = HMAC(kRegion, Service)
// kSigning = HMAC(kService, "aws4_request")
pub fn generate_signing_key(
    secret: &str,
    date: Date<Utc>,
    region: &str,
    service: &str,
) -> hmac::Tag {
    let secret = format!("AWS4{}", secret);
    let secret = hmac::Key::new(hmac::HMAC_SHA256, &secret.as_bytes());
    let tag = hmac::sign(&secret, date.fmt_aws().as_bytes());

    // sign region
    let key = hmac::Key::new(hmac::HMAC_SHA256, tag.as_ref());
    let tag = hmac::sign(&key, region.as_bytes());

    // sign service
    let key = hmac::Key::new(hmac::HMAC_SHA256, tag.as_ref());
    let tag = hmac::sign(&key, service.as_bytes());

    // sign request
    let key = hmac::Key::new(hmac::HMAC_SHA256, tag.as_ref());
    hmac::sign(&key, "aws4_request".as_bytes())
}
