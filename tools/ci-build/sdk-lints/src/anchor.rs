/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::bail;

pub fn anchors(name: &str) -> (String, String) {
    (
        format!("{ANCHOR_START}{name} -->"),
        format!("{ANCHOR_END}{name} -->"),
    )
}

const ANCHOR_START: &str = "<!-- anchor_start:";
const ANCHOR_END: &str = "<!-- anchor_end:";

pub fn replace_anchor(
    haystack: &mut String,
    anchors: &(impl AsRef<str>, impl AsRef<str>),
    new_content: &str,
    default_insertion_position: Option<usize>,
) -> anyhow::Result<bool> {
    let anchor_start = anchors.0.as_ref();
    let anchor_end = anchors.1.as_ref();
    let start = haystack.find(anchor_start);
    if start.is_none() {
        if haystack.contains(anchor_end) {
            bail!("found end anchor but no start anchor");
        }

        let (prefix, suffix) = match default_insertion_position {
            Some(position) => haystack.split_at(position),
            None => (haystack.as_ref(), ""),
        };
        let mut out = String::new();
        out.push_str(prefix);
        out.push('\n');
        out.push_str(anchor_start);
        out.push_str(new_content);
        out.push_str(anchor_end);
        out.push_str(suffix);
        *haystack = out;
        return Ok(true);
    }
    let start = start.unwrap_or_else(|| haystack.find(anchor_start).expect("must be present"));
    let end = match haystack[start..].find(anchor_end) {
        Some(end) => end + start,
        None => bail!("expected matching end anchor {anchor_end}"),
    };
    let prefix = &haystack[..start + anchor_start.len()];
    let suffix = &haystack[end..];
    let mut out = String::new();
    out.push_str(prefix);
    out.push_str(new_content);
    out.push_str(suffix);
    if haystack != &out {
        *haystack = out;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod test {
    use crate::anchor::{anchors, replace_anchor};

    #[test]
    fn updates_empty() {
        let mut text = "this is the start".to_string();
        assert!(replace_anchor(&mut text, &anchors("foo"), "hello!", None).unwrap());
        assert_eq!(
            text,
            "this is the start\n<!-- anchor_start:foo -->hello!<!-- anchor_end:foo -->"
        );
    }

    #[test]
    fn updates_existing() {
        let mut text =
            "this is the start\n<!-- anchor_start:foo -->hello!<!-- anchor_end:foo -->".to_string();
        assert!(replace_anchor(&mut text, &anchors("foo"), "goodbye!", None).unwrap());
        assert_eq!(
            text,
            "this is the start\n<!-- anchor_start:foo -->goodbye!<!-- anchor_end:foo -->"
        );

        // no replacement should return false
        assert!(!replace_anchor(&mut text, &anchors("foo"), "goodbye!", None).unwrap(),)
    }
}
