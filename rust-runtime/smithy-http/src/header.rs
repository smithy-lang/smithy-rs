/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Utilities for parsing information from headers

use smithy_types::instant::Format;
use smithy_types::Instant;
use std::str::FromStr;

#[derive(Debug)]
pub struct ParseError;

/// Read all the dates from the header map at `key` according the `format`
///
/// This is separate from `read_many` below because we need to invoke `Instant::read` to take advantage
/// of comma-aware parsing
pub fn many_dates(
    headers: &http::HeaderMap,
    key: &str,
    format: Format,
) -> Result<Vec<Instant>, ParseError> {
    let mut out = vec![];
    for header in headers.get_all(key).iter() {
        let mut header = header.to_str().map_err(|_| ParseError)?;
        while !header.is_empty() {
            let (v, next) = Instant::read(header, format, ',').map_err(|_| ParseError)?;
            out.push(v);
            header = next;
        }
    }
    Ok(out)
}

/// Read many comma / header delimited values from HTTP headers for `FromStr` types
pub fn read_many<T>(headers: &http::HeaderMap, key: &str) -> Result<Vec<T>, ParseError>
where
    T: FromStr,
{
    let mut out = vec![];
    for header in headers.get_all(key).iter() {
        let mut header = header.as_bytes();
        while !header.is_empty() {
            let (v, next) = read_one::<T>(&header)?;
            out.push(v);
            header = next;
        }
    }
    Ok(out)
}

/// Read one comma delimited value for `FromStr` types
pub fn read_one<T>(s: &[u8]) -> Result<(T, &[u8]), ParseError>
where
    T: FromStr,
{
    let (head, rest) = split_at_delim(s);
    let head = std::str::from_utf8(head).map_err(|_| ParseError)?;
    Ok((T::from_str(head).map_err(|_| ParseError)?, rest))
}

fn split_at_delim(s: &[u8]) -> (&[u8], &[u8]) {
    let next_delim = s.iter().position(|b| b == &b',').unwrap_or(s.len());
    let (first, next) = s.split_at(next_delim);
    (first, then_delim(next).unwrap())
}

fn then_delim(s: &[u8]) -> Result<&[u8], ParseError> {
    if s.is_empty() {
        Ok(&s)
    } else if s.starts_with(b",") {
        Ok(&s[1..])
    } else {
        Err(ParseError)
    }
}

#[cfg(test)]
mod test {
    use crate::header::read_many;

    #[test]
    fn read_many_bools() {
        let test_request = http::Request::builder()
            .header("X-Bool-Multi", "true,false")
            .header("X-Bool-Multi", "true")
            .header("X-Bool", "true")
            .header("X-Bool-Invalid", "truth,falsy")
            .header("X-Bool-Single", "true,false,true,true")
            .body(())
            .unwrap();
        assert_eq!(
            read_many::<bool>(test_request.headers(), "X-Bool-Multi").expect("valid"),
            vec![true, false, true]
        );

        assert_eq!(
            read_many::<bool>(test_request.headers(), "X-Bool").unwrap(),
            vec![true]
        );
        assert_eq!(
            read_many::<bool>(test_request.headers(), "X-Bool-Single").unwrap(),
            vec![true, false, true, true]
        );
        read_many::<bool>(test_request.headers(), "X-Bool-Invalid").expect_err("invalid");
    }

    #[test]
    fn read_many_u16() {
        let test_request = http::Request::builder()
            .header("X-Multi", "123,456")
            .header("X-Multi", "789")
            .header("X-Num", "777")
            .header("X-Num-Invalid", "12ef3")
            .header("X-Num-Single", "1,2,3,4,5")
            .body(())
            .unwrap();
        assert_eq!(
            read_many::<u16>(test_request.headers(), "X-Multi").expect("valid"),
            vec![123, 456, 789]
        );

        assert_eq!(
            read_many::<u16>(test_request.headers(), "X-Num").unwrap(),
            vec![777]
        );
        assert_eq!(
            read_many::<u16>(test_request.headers(), "X-Num-Single").unwrap(),
            vec![1, 2, 3, 4, 5]
        );
        read_many::<u16>(test_request.headers(), "X-Num-Invalid").expect_err("invalid");
    }
}
