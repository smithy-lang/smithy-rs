/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Incremental CBOR boundaries only. Modeled values are decoded by the existing codec.
use aws_smithy_schema::serde::SerdeError;

pub(super) fn error(message: &str) -> SerdeError {
    SerdeError::invalid_input(message)
}

/// A complete header, without consuming the value's payload.
pub(super) fn header(bytes: &[u8]) -> Result<Option<(u8, Option<u64>, usize)>, SerdeError> {
    let Some(&first) = bytes.first() else { return Ok(None) };
    let info = first & 31;
    let width = match info {
        0..=23 | 31 => 0,
        24 => 1,
        25 => 2,
        26 => 4,
        27 => 8,
        _ => return Err(error("invalid CBOR additional information")),
    };
    if bytes.len() < width + 1 {
        return Ok(None);
    }
    let value = if info == 31 {
        None
    } else if width == 0 {
        Some(u64::from(info))
    } else {
        Some(bytes[1..=width].iter().fold(0u64, |n, b| (n << 8) | u64::from(*b)))
    };
    Ok(Some((first >> 5, value, width + 1)))
}

#[derive(Default, Debug)]
pub(super) struct Scanner {
    position: usize,
    payload: u64,
    stack: Vec<Option<u64>>,
    started: bool,
}

pub(super) enum Scanned {
    Complete(usize),
    More,
    Yield,
}

impl Scanner {
    /// Remembers its position across frames, avoiding repeatedly scanning growing values.
    pub(super) fn scan(&mut self, bytes: &[u8], budget: usize) -> Result<Scanned, SerdeError> {
        if !self.started {
            self.started = true;
            self.stack.push(Some(1));
        }
        for _ in 0..budget {
            if self.payload != 0 {
                let available = (bytes.len() - self.position) as u64;
                let take = self.payload.min(available);
                self.position += take as usize;
                self.payload -= take;
                if self.payload != 0 {
                    return Ok(Scanned::More);
                }
            }
            while self.stack.last() == Some(&Some(0)) {
                self.stack.pop();
            }
            let Some(parent) = self.stack.last_mut() else {
                return Ok(Scanned::Complete(self.position));
            };
            let Some((major, value, width)) = header(&bytes[self.position..])? else {
                return Ok(Scanned::More);
            };
            self.position += width;
            if major == 7 && value.is_none() {
                if parent.is_some() {
                    return Err(error("unexpected CBOR break"));
                }
                self.stack.pop();
                continue;
            }
            if let Some(left) = parent {
                *left -= 1;
            }
            match (major, value) {
                (0 | 1 | 7, Some(_)) => {}
                (2 | 3, Some(length)) => self.payload = length,
                (2..=5, None) => self.stack.push(None),
                (4, Some(length)) => self.stack.push(Some(length)),
                (5, Some(length)) => self.stack.push(Some(
                    length.checked_mul(2).ok_or_else(|| error("CBOR map length overflow"))?,
                )),
                (6, Some(_)) => self.stack.push(Some(1)),
                _ => return Err(error("invalid indefinite CBOR value")),
            }
            // Semantic checks, including the configured nesting limit for modeled
            // containers, belong to the existing codec, not this boundary scanner.
        }
        Ok(Scanned::Yield)
    }
}
