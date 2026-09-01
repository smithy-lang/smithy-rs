/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// Operation route prefix admission policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefixPolicy {
    canonical_allowed: bool,
    prefixes: &'static [&'static str],
}

impl PrefixPolicy {
    /// Default policy: canonical route is allowed and no extra prefixes are accepted.
    pub const DEFAULT: Self = Self {
        canonical_allowed: true,
        prefixes: &[],
    };

    /// Creates a prefix policy from normalized prefix strings.
    pub const fn new(canonical_allowed: bool, prefixes: &'static [&'static str]) -> Self {
        Self {
            canonical_allowed,
            prefixes,
        }
    }

    /// Returns whether the canonical unprefixed route is allowed.
    pub fn canonical_allowed(&self) -> bool {
        self.canonical_allowed
    }

    /// Returns normalized prefixes accepted by this policy.
    pub fn prefixes(&self) -> &'static [&'static str] {
        self.prefixes
    }

    /// Returns candidate paths to try for this policy.
    pub fn candidates<'a>(&self, path: &'a str) -> PrefixCandidates<'a> {
        PrefixCandidates {
            policy: *self,
            path,
            state: PrefixCandidateState::Canonical,
            prefix_index: 0,
        }
    }
}

/// Iterator over canonical/prefix-stripped path candidates.
pub struct PrefixCandidates<'a> {
    policy: PrefixPolicy,
    path: &'a str,
    state: PrefixCandidateState,
    prefix_index: usize,
}

#[derive(Debug, Clone, Copy)]
enum PrefixCandidateState {
    Canonical,
    Prefixes,
    Done,
}

impl<'a> Iterator for PrefixCandidates<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.state {
                PrefixCandidateState::Canonical => {
                    self.state = PrefixCandidateState::Prefixes;
                    if self.policy.canonical_allowed {
                        return Some(self.path);
                    }
                }
                PrefixCandidateState::Prefixes => {
                    while let Some(prefix) = self.policy.prefixes.get(self.prefix_index) {
                        self.prefix_index += 1;
                        if let Some(stripped) = strip_prefix(self.path, prefix) {
                            return Some(stripped);
                        }
                    }
                    self.state = PrefixCandidateState::Done;
                }
                PrefixCandidateState::Done => return None,
            }
        }
    }
}

fn strip_prefix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if prefix == "/" {
        return Some(path);
    }
    let stripped = path.strip_prefix(prefix)?;
    if stripped.is_empty() {
        Some("/")
    } else if stripped.starts_with('/') {
        Some(stripped)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::PrefixPolicy;

    #[test]
    fn default_policy_allows_canonical_only() {
        let candidates: Vec<_> = PrefixPolicy::DEFAULT.candidates("/foo").collect();
        assert_eq!(candidates, ["/foo"]);
    }

    #[test]
    fn only_prefix_policy_strips_prefix_and_disallows_canonical() {
        let policy = PrefixPolicy::new(false, &["/v1"]);
        let candidates: Vec<_> = policy.candidates("/v1/foo").collect();
        assert_eq!(candidates, ["/foo"]);
        let candidates: Vec<_> = policy.candidates("/foo").collect();
        assert!(candidates.is_empty());
    }

    #[test]
    fn also_prefix_policy_keeps_canonical_and_strips_prefix() {
        let policy = PrefixPolicy::new(true, &["/v1"]);
        let candidates: Vec<_> = policy.candidates("/v1/foo").collect();
        assert_eq!(candidates, ["/v1/foo", "/foo"]);
    }
}
