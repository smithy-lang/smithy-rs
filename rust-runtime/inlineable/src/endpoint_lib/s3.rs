/*
 *  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 *  SPDX-License-Identifier: Apache-2.0
 */

use crate::endpoint_lib::diagnostic::DiagnosticCollector;
use crate::endpoint_lib::host::is_valid_host_label;

/// Evaluates whether a string is a DNS-compatible bucket name that can be used with virtual hosted-style addressing.
pub(crate) fn is_virtual_hostable_s3_bucket(
    host_label: &str,
    allow_subdomains: bool,
    e: &mut DiagnosticCollector,
) -> bool {
    if !is_valid_host_label(host_label, allow_subdomains, e) {
        false
    } else if !allow_subdomains {
        is_virtual_hostable_segment(host_label)
    } else {
        host_label.split('.').all(is_virtual_hostable_segment)
    }
}

fn is_virtual_hostable_segment(host_label: &str) -> bool {
    let bytes = host_label.as_bytes();
    // ^[a-z\d][a-z\d\-.]{1,61}[a-z\d]$ — length 3..=63, bookended by [a-z0-9]
    if !(3..=63).contains(&bytes.len()) {
        return false;
    }
    let (&first, rest) = match bytes.split_first() {
        Some(v) => v,
        None => return false, // unreachable: length >= 3
    };
    let (&last, middle) = match rest.split_last() {
        Some(v) => v,
        None => return false, // unreachable: length >= 3
    };
    let is_bucket_char = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    if !is_bucket_char(&first) || !is_bucket_char(&last) {
        return false;
    }
    // validate middle chars and reject ".-" / "-." adjacency
    let valid_chars = middle
        .iter()
        .all(|b| is_bucket_char(b) || *b == b'-' || *b == b'.');
    let no_dot_dash = !bytes
        .windows(2)
        .any(|w| matches!(w, [b'.', b'-'] | [b'-', b'.']));
    valid_chars && no_dot_dash && !is_ipv4(bytes)
}

/// Matches `^(\d+\.){3}\d+$`
fn is_ipv4(bytes: &[u8]) -> bool {
    let mut dots = 0;
    let mut has_digit = false;
    for &b in bytes {
        if b.is_ascii_digit() {
            has_digit = true;
        } else if b == b'.' {
            if !has_digit {
                return false;
            }
            dots += 1;
            has_digit = false;
        } else {
            return false;
        }
    }
    dots == 3 && has_digit
}

#[cfg(test)]
mod test {
    use super::*;

    #[derive(Clone, Copy)]
    enum Subdomains {
        Allow,
        Deny,
    }

    fn is_virtual_hostable(label: &str, subdomains: Subdomains) -> bool {
        is_virtual_hostable_s3_bucket(
            label,
            matches!(subdomains, Subdomains::Allow),
            &mut DiagnosticCollector::new(),
        )
    }

    #[test]
    fn check_s3_bucket() {
        // double dashes are valid
        assert!(is_virtual_hostable("a--b--x-s3", Subdomains::Deny));
        // dot-dash adjacency rejected with subdomains
        assert!(!is_virtual_hostable("a-.b-.c", Subdomains::Allow));
    }

    #[test]
    fn valid_buckets() {
        assert!(is_virtual_hostable("abc", Subdomains::Deny));
        assert!(is_virtual_hostable("my-bucket", Subdomains::Deny));
        assert!(is_virtual_hostable("my-bucket-123", Subdomains::Deny));
        assert!(is_virtual_hostable("a0b", Subdomains::Deny));
        assert!(is_virtual_hostable("abc.def.ghi", Subdomains::Allow));
    }

    #[test]
    fn length_bounds() {
        // too short
        assert!(!is_virtual_hostable("ab", Subdomains::Deny));
        // minimum length
        assert!(is_virtual_hostable("abc", Subdomains::Deny));
        // 63 chars — maximum
        assert!(is_virtual_hostable(
            &format!("a{}b", "c".repeat(61)),
            Subdomains::Deny
        ));
        // 64 chars — too long
        assert!(!is_virtual_hostable(
            &format!("a{}b", "c".repeat(62)),
            Subdomains::Deny
        ));
    }

    #[test]
    fn first_last_char() {
        // must start with [a-z0-9]
        assert!(!is_virtual_hostable("-abc", Subdomains::Deny));
        assert!(!is_virtual_hostable(".abc", Subdomains::Deny));
        // must end with [a-z0-9]
        assert!(!is_virtual_hostable("abc-", Subdomains::Deny));
        assert!(!is_virtual_hostable("abc.", Subdomains::Deny));
        // uppercase rejected
        assert!(!is_virtual_hostable("Abc", Subdomains::Deny));
        assert!(!is_virtual_hostable("abC", Subdomains::Deny));
    }

    #[test]
    fn dot_dash_adjacency() {
        assert!(!is_virtual_hostable("bucket.-name", Subdomains::Deny));
        assert!(!is_virtual_hostable("bucket-.name", Subdomains::Deny));
        assert!(!is_virtual_hostable("a.-b", Subdomains::Allow));
        assert!(!is_virtual_hostable("a-.b", Subdomains::Allow));
    }

    #[test]
    fn invalid_characters() {
        assert!(!is_virtual_hostable("abc_def", Subdomains::Deny));
        assert!(!is_virtual_hostable("abc def", Subdomains::Deny));
        assert!(!is_virtual_hostable("abc!def", Subdomains::Deny));
    }

    // Ported from Java SDK's RuleUrlTest.java isIpAddr test cases
    #[test]
    fn ipv4_rejected() {
        assert!(!is_virtual_hostable("0.0.0.0", Subdomains::Deny));
        assert!(!is_virtual_hostable("127.0.0.1", Subdomains::Allow));
        assert!(!is_virtual_hostable("192.168.1.1", Subdomains::Allow));
    }

    #[test]
    fn ipv4_like_but_valid_bucket() {
        // contains letters — not IPv4
        assert!(is_virtual_hostable("abc.def.ghi.jkl", Subdomains::Allow));
        // more than 4 segments — not IPv4
        assert!(is_virtual_hostable(
            "1a2.2b3.3c4.4d5.5e6",
            Subdomains::Allow
        ));
    }

    #[test]
    fn is_ipv4_unit() {
        // Ported from Java SDK RuleUrlTest isIpAddr cases
        assert!(is_ipv4(b"0.0.0.0"));
        assert!(is_ipv4(b"127.0.0.1"));
        assert!(is_ipv4(b"132.248.181.171"));
        // fewer than 4 segments
        assert!(!is_ipv4(b"127.0.0"));
        assert!(!is_ipv4(b"127.0"));
        assert!(!is_ipv4(b"127"));
        // more than 4 segments
        assert!(!is_ipv4(b"127.0.0.1.1"));
        // non-numeric
        assert!(!is_ipv4(b"foo.1.1.1"));
        assert!(!is_ipv4(b"1.foo.1.1"));
        assert!(!is_ipv4(b"amazon.com"));
        assert!(!is_ipv4(b"localhost"));
        // empty segment
        assert!(!is_ipv4(b".1.1.1"));
        assert!(!is_ipv4(b"1..1.1"));
    }
}
