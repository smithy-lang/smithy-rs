/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

/// A correct, small, but not especially fast
/// base64 implementation
// TODO: Fuzz and test against the base64 crate
const BASE64_ENCODE_TABLE: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode<T: AsRef<[u8]>>(inp: T) -> String {
    let inp = inp.as_ref();
    encode_inner(inp)
}

fn encode_inner(inp: &[u8]) -> String {
    // Base 64 encodes groups of 6 bits into characters—this means that each
    // 3 byte group (24 bits) is encoded into 4 base64 characters.
    let char_ct = ((inp.len() + 2) / 3) * 4;
    let mut output = String::with_capacity(char_ct);
    for chunk in inp.chunks(3) {
        let mut block: i32 = 0;
        // Write the chunks into the beginning of a 32 bit int
        for (idx, chunk) in chunk.iter().enumerate() {
            block |= (*chunk as i32) << ((3 - idx) * 8);
        }
        let num_sextets = ((chunk.len() * 8) + 5) / 6;
        for idx in 0..num_sextets {
            let slice = block >> (26 - (6 * idx));
            let idx = (slice as u8) & 0b0011_1111;
            output.push(BASE64_ENCODE_TABLE[idx as usize] as char);
        }
        for _ in 0..(4 - num_sextets) {
            output.push('=');
        }
    }
    // be sure we got it right
    debug_assert_eq!(output.capacity(), char_ct);
    output
}

#[cfg(test)]
mod test {
    use crate::base64::encode;
    // TODO: base64 decoder
    // TODO: round trip testing / fuzzing
    // TODO: dev-dependency on base64 crate? to test against it?

    #[test]
    fn test_base64() {
        assert_eq!(encode("abc"), "YWJj");
        assert_eq!(encode("anything you want."), "YW55dGhpbmcgeW91IHdhbnQu");
        assert_eq!(encode("anything you want"), "YW55dGhpbmcgeW91IHdhbnQ=");
        assert_eq!(encode("anything you wan"), "YW55dGhpbmcgeW91IHdhbg==");
    }

    #[test]
    fn test_base64_long() {
        let decoded = "Alas, eleventy-one years is far too short a time to live among such excellent and admirable hobbits. I don't know half of you half as well as I should like, and I like less than half of you half as well as you deserve.";
        let encoded = "QWxhcywgZWxldmVudHktb25lIHllYXJzIGlzIGZhciB0b28gc2hvcnQgYSB0aW1lIHRvIGxpdmUgYW1vbmcgc3VjaCBleGNlbGxlbnQgYW5kIGFkbWlyYWJsZSBob2JiaXRzLiBJIGRvbid0IGtub3cgaGFsZiBvZiB5b3UgaGFsZiBhcyB3ZWxsIGFzIEkgc2hvdWxkIGxpa2UsIGFuZCBJIGxpa2UgbGVzcyB0aGFuIGhhbGYgb2YgeW91IGhhbGYgYXMgd2VsbCBhcyB5b3UgZGVzZXJ2ZS4=";
        assert_eq!(encode(decoded), encoded);
    }

    #[test]
    fn test_base64_utf8() {
        let decoded = "ユニコードとはか？";
        let encoded = "44Om44OL44Kz44O844OJ44Go44Gv44GL77yf";
        assert_eq!(encode(decoded), encoded);
    }
    #[test]
    fn test_base64_control_chars() {
        let decoded = "hello\tworld\n";
        let encoded = "aGVsbG8Jd29ybGQK";
        assert_eq!(encode(decoded), encoded);
    }
}
