/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::syntax::{IdlParseError, ShapeIdSyntax, Span, Spanned};
use std::collections::HashSet;
use winnow::prelude::*;
use winnow::token::take_while;

fn is_identifier_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

fn is_identifier_continue(c: char) -> bool {
    is_identifier_start(c) || c.is_ascii_digit()
}

/// Winnow owns token repetition and committed parser mechanics. The crate cursor owns
/// Smithy's context-sensitive whitespace and source spans.
fn identifier_token<'i>(input: &mut &'i str) -> ModalResult<&'i str> {
    take_while(1.., is_identifier_continue).parse_next(input)
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Trivia {
    pub(super) had_break: bool,
}

pub(super) struct Cursor<'a> {
    input: &'a str,
    pub(super) pos: usize,
    pub(super) max_depth: usize,
    pub(super) max_shapes: usize,
    pub(super) shapes: usize,
}

impl<'a> Cursor<'a> {
    pub(super) fn new(input: &'a str, max_depth: usize, max_shapes: usize) -> Self {
        Self {
            input,
            pos: 0,
            max_depth,
            max_shapes,
            shapes: 0,
        }
    }

    pub(super) fn input(&self) -> &'a str {
        self.input
    }

    pub(super) fn eof(&self) -> bool {
        self.pos == self.input.len()
    }

    pub(super) fn rest(&self) -> &'a str {
        &self.input[self.pos..]
    }

    pub(super) fn starts_with(&self, value: &str) -> bool {
        self.rest().starts_with(value)
    }

    pub(super) fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    pub(super) fn bump(&mut self) -> Option<char> {
        let value = self.peek()?;
        self.pos += value.len_utf8();
        Some(value)
    }

    pub(super) fn consume(&mut self, value: &str) -> bool {
        if self.starts_with(value) {
            self.pos += value.len();
            true
        } else {
            false
        }
    }

    pub(super) fn expect(&mut self, value: &str, what: &str) -> Result<Span, IdlParseError> {
        let start = self.pos;
        if self.consume(value) {
            Ok(Span::new(start, self.pos))
        } else {
            Err(self.error(format!("expected {what}")))
        }
    }

    pub(super) fn error(&self, message: impl Into<String>) -> IdlParseError {
        IdlParseError::syntax(self.pos, message)
    }

    pub(super) fn sp0(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.bump();
        }
    }

    pub(super) fn sp1(&mut self) -> Result<(), IdlParseError> {
        let start = self.pos;
        self.sp0();
        if self.pos == start {
            Err(self.error("expected a space or tab"))
        } else {
            Ok(())
        }
    }

    /// Smithy WS: spaces, tabs, line endings, commas, and line comments.
    pub(super) fn ws0(&mut self) -> Trivia {
        let mut trivia = Trivia::default();
        loop {
            match self.peek() {
                Some(' ' | '\t' | ',') => {
                    self.bump();
                }
                Some('\n') => {
                    trivia.had_break = true;
                    self.bump();
                }
                Some('\r') => {
                    trivia.had_break = true;
                    self.bump();
                    self.consume("\n");
                }
                Some('/') if self.starts_with("//") => {
                    trivia.had_break = true;
                    self.consume_line_comment();
                }
                _ => break,
            }
        }
        trivia
    }

    /// Checks a declaration boundary without consuming a following documentation block.
    pub(super) fn declaration_break(&mut self) -> Result<(), IdlParseError> {
        let mut had_break = false;
        loop {
            match self.peek() {
                Some(' ' | '\t') => {
                    self.bump();
                }
                // Commas are whitespace only in BR's trailing `[WS]`, after the break.
                Some(',') if had_break => {
                    self.bump();
                }
                Some('\n') => {
                    had_break = true;
                    self.bump();
                }
                Some('\r') => {
                    had_break = true;
                    self.bump();
                    self.consume("\n");
                }
                // A documentation comment starts its own line, so a break precedes it; it is left
                // for the next declaration to consume.
                Some('/') if self.starts_with("///") && self.at_line_start() => return Ok(()),
                Some('/') if self.starts_with("//") => {
                    had_break = true;
                    self.consume_line_comment();
                }
                _ => break,
            }
        }
        if self.eof() || had_break {
            Ok(())
        } else {
            // Point at the token that should have started a new line.
            Err(self.error("expected a line break between statements"))
        }
    }

    fn consume_line_comment(&mut self) {
        debug_assert!(self.starts_with("//"));
        while let Some(c) = self.bump() {
            if c == '\n' {
                break;
            }
            if c == '\r' {
                self.consume("\n");
                break;
            }
        }
    }

    /// Whether the cursor is at the first non-whitespace character of its line. Only then does
    /// `///` start a documentation comment; after other tokens it is an ordinary comment.
    fn at_line_start(&self) -> bool {
        // Only whitespace and commas are skipped, so this scans just the leading trivia.
        self.input.as_bytes()[..self.pos]
            .iter()
            .rev()
            .find(|b| !matches!(b, b' ' | b'\t' | b','))
            .is_none_or(|b| matches!(b, b'\n' | b'\r'))
    }

    /// Consumes leading trivia and returns its documentation comments.
    ///
    /// Like Smithy Java, every documentation line since the previous token is kept, joined by
    /// `\n`, even when blank lines or ordinary comments separate them.
    pub(super) fn documentation(&mut self) -> Option<Spanned<String>> {
        let mut lines = Vec::new();
        let mut first = None;
        let mut end = self.pos;
        loop {
            match self.peek() {
                Some(' ' | '\t' | ',' | '\n') => {
                    self.bump();
                }
                Some('\r') => {
                    self.bump();
                    self.consume("\n");
                }
                Some('/') if self.starts_with("///") && self.at_line_start() => {
                    let start = self.pos;
                    self.consume("///");
                    let text_start = self.pos;
                    while !self.eof() && !matches!(self.peek(), Some('\r' | '\n')) {
                        self.bump();
                    }
                    let text = &self.input[text_start..self.pos];
                    lines.push(text.strip_prefix(' ').unwrap_or(text).to_owned());
                    first.get_or_insert(start);
                    end = self.pos;
                }
                Some('/') if self.starts_with("//") => self.consume_line_comment(),
                _ => break,
            }
        }
        first.map(|start| Spanned::new(lines.join("\n"), Span::new(start, end)))
    }

    pub(super) fn identifier(&mut self) -> Result<Spanned<String>, IdlParseError> {
        let start = self.pos;
        let Some(first) = self.peek() else {
            return Err(self.error("expected an identifier"));
        };
        if !is_identifier_start(first) {
            return Err(self.error("expected an identifier"));
        }
        let mut rest = self.rest();
        let token =
            identifier_token(&mut rest).map_err(|_| self.error("expected an identifier"))?;
        self.pos += token.len();
        Ok(Spanned::new(token.to_owned(), Span::new(start, self.pos)))
    }

    pub(super) fn keyword(&mut self, value: &str) -> bool {
        if !self.starts_with(value) {
            return false;
        }
        let tail = &self.rest()[value.len()..];
        if tail.chars().next().is_some_and(is_identifier_continue) {
            return false;
        }
        self.pos += value.len();
        true
    }

    pub(super) fn shape_id(&mut self) -> Result<ShapeIdSyntax, IdlParseError> {
        let start = self.pos;
        let first = self.identifier()?;
        let mut value = first.value;
        let mut absolute = false;

        while self.consume(".") {
            absolute = true;
            value.push('.');
            value.push_str(&self.identifier()?.value);
        }
        if self.consume("#") {
            absolute = true;
            value.push('#');
            value.push_str(&self.identifier()?.value);
        } else if absolute {
            return Err(self.error("expected `#` and a shape name after the namespace"));
        }

        let member = if self.consume("$") {
            let member = self.identifier()?.value;
            value.push('$');
            value.push_str(&member);
            Some(member)
        } else {
            None
        };
        Ok(ShapeIdSyntax {
            value,
            absolute,
            member,
            span: Span::new(start, self.pos),
        })
    }

    pub(super) fn string(&mut self) -> Result<Spanned<String>, IdlParseError> {
        if self.starts_with("\"\"\"") {
            self.text_block()
        } else {
            self.quoted_string()
        }
    }

    fn quoted_string(&mut self) -> Result<Spanned<String>, IdlParseError> {
        let start = self.pos;
        self.expect("\"", "a quoted string")?;
        let mut out = String::new();
        loop {
            let Some(c) = self.bump() else {
                return Err(IdlParseError::syntax(start, "unterminated quoted string"));
            };
            match c {
                '"' => break,
                '\\' => self.escape(&mut out)?,
                '\r' => {
                    self.consume("\n");
                    out.push('\n');
                }
                '\n' => out.push('\n'),
                '\0' => return Err(self.error("NUL is not allowed in a string")),
                c => out.push(c),
            }
        }
        Ok(Spanned::new(out, Span::new(start, self.pos)))
    }

    fn escape(&mut self, out: &mut String) -> Result<(), IdlParseError> {
        let Some(c) = self.bump() else {
            return Err(self.error("unterminated escape sequence"));
        };
        match c {
            '"' | '\\' | '/' => out.push(c),
            'b' => out.push('\u{0008}'),
            'f' => out.push('\u{000c}'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            '\n' => {}
            '\r' => {
                self.consume("\n");
            }
            'u' => {
                let first = self.hex_quad()?;
                let scalar = if (0xD800..=0xDBFF).contains(&first) {
                    if !self.consume("\\u") {
                        return Err(
                            self.error("high surrogate must be followed by a low surrogate")
                        );
                    }
                    let second = self.hex_quad()?;
                    if !(0xDC00..=0xDFFF).contains(&second) {
                        return Err(self.error("invalid low surrogate"));
                    }
                    0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                } else if (0xDC00..=0xDFFF).contains(&first) {
                    return Err(self.error("unexpected low surrogate"));
                } else {
                    first
                };
                let value =
                    char::from_u32(scalar).ok_or_else(|| self.error("invalid Unicode escape"))?;
                out.push(value);
            }
            _ => return Err(self.error(format!("invalid escape sequence `\\{c}`"))),
        }
        Ok(())
    }

    fn hex_quad(&mut self) -> Result<u32, IdlParseError> {
        let start = self.pos;
        let end = start.saturating_add(4);
        if end > self.input.len() || !self.input.is_char_boundary(end) {
            return Err(self.error("incomplete Unicode escape"));
        }
        let digits = &self.input[start..end];
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(self.error("Unicode escape must contain four hexadecimal digits"));
        }
        self.pos = end;
        Ok(u32::from_str_radix(digits, 16).expect("validated hexadecimal digits"))
    }

    fn text_block(&mut self) -> Result<Spanned<String>, IdlParseError> {
        let start = self.pos;
        self.consume("\"\"\"");
        self.sp0();
        match self.peek() {
            Some('\n') => {
                self.bump();
            }
            Some('\r') => {
                self.bump();
                self.consume("\n");
            }
            _ => return Err(self.error("a text block opening delimiter must end its line")),
        }
        let content_start = self.pos;
        let rest = self.rest();
        let mut search_from = 0;
        let relative_end = loop {
            let Some(found) = rest[search_from..].find("\"\"\"") else {
                return Err(IdlParseError::syntax(start, "unterminated text block"));
            };
            let found = search_from + found;
            let preceding_slashes = rest[..found]
                .bytes()
                .rev()
                .take_while(|&byte| byte == b'\\')
                .count();
            if preceding_slashes % 2 == 0 {
                break found;
            }
            search_from = found + 1;
        };
        let content_end = self.pos + relative_end;
        let raw = self.input[content_start..content_end]
            .replace("\r\n", "\n")
            .replace('\r', "\n");
        self.pos = content_end + 3;

        // The closing delimiter line participates in the common-space prefix. Keeping its
        // empty remainder also preserves the trailing LF required by Smithy text blocks.
        let lines: Vec<&str> = raw.split('\n').collect();
        let indent = lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| {
                let spaces = line.bytes().take_while(|&byte| byte == b' ').count();
                let is_closing_line = index + 1 == lines.len();
                (is_closing_line || !line[spaces..].is_empty()).then_some(spaces)
            })
            .min()
            .unwrap_or(0);
        let normalized = lines
            .iter()
            .map(|line| {
                let strip = line.bytes().take(indent).take_while(|&b| b == b' ').count();
                line[strip..].trim_end_matches(' ')
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Interpret escapes only after indentation normalization.
        let mut decoder = Cursor::new(&normalized, self.max_depth, self.max_shapes);
        let mut out = String::new();
        while let Some(c) = decoder.bump() {
            if c == '\\' {
                decoder.escape(&mut out).map_err(|mut err| {
                    // Normalization removes indentation and can collapse CRLF, so decoder
                    // offsets cannot be translated linearly back into the source.
                    err.offset = start;
                    err
                })?;
            } else {
                out.push(c);
            }
        }
        Ok(Spanned::new(out, Span::new(start, self.pos)))
    }

    pub(super) fn object_key(&mut self) -> Result<Spanned<String>, IdlParseError> {
        if self.starts_with("\"\"\"") {
            Err(self.error("a text block cannot be used as an object key"))
        } else if self.starts_with("\"") {
            self.string()
        } else {
            self.identifier()
        }
    }

    pub(super) fn ensure_unique(
        &self,
        seen: &mut HashSet<String>,
        key: &Spanned<String>,
    ) -> Result<(), IdlParseError> {
        if seen.insert(key.value.clone()) {
            Ok(())
        } else {
            Err(IdlParseError::duplicate(key.span.start, &key.value))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_use_winnow_and_require_full_tokens() {
        let mut cursor = Cursor::new("_a9 rest", 10, 10);
        assert_eq!(cursor.identifier().unwrap().value, "_a9");
        assert!(!cursor.keyword("rest"));
        cursor.sp1().unwrap();
        assert!(cursor.keyword("rest"));

        let mut cursor = Cursor::new("structureValue", 10, 10);
        assert!(!cursor.keyword("structure"));
        assert_eq!(cursor.identifier().unwrap().value, "structureValue");
    }

    #[test]
    fn shape_ids_retain_resolution_information() {
        let mut cursor = Cursor::new("example.foo#Shape$member Other", 10, 10);
        let id = cursor.shape_id().unwrap();
        assert!(id.absolute);
        assert_eq!(id.root_name(), "Shape");
        assert_eq!(id.member.as_deref(), Some("member"));
        cursor.sp1().unwrap();
        assert!(!cursor.shape_id().unwrap().absolute);
    }

    #[test]
    fn strings_and_text_blocks_decode() {
        let mut cursor = Cursor::new("\"a\\n\\u00e9\\\nb\"", 10, 10);
        assert_eq!(cursor.string().unwrap().value, "a\néb");
        let mut cursor = Cursor::new("\"\"\"\n    one  \n      two\n    \"\"\"", 10, 10);
        assert_eq!(cursor.string().unwrap().value, "one\n  two\n");
    }

    #[test]
    fn documentation_joins_every_line_since_the_previous_token() {
        let mut cursor = Cursor::new(
            "// old\n/// first\n\n// plain\n/// second\n///third\nshape",
            10,
            10,
        );
        let docs = cursor.documentation().unwrap();
        assert_eq!(docs.value, "first\nsecond\nthird");
        assert_eq!(docs.span.start, 7);
        assert!(cursor.keyword("shape"));
    }

    #[test]
    fn trailing_triple_slash_is_an_ordinary_comment() {
        let mut cursor = Cursor::new("x /// not docs\nshape", 10, 10);
        assert!(cursor.keyword("x"));
        assert!(cursor.declaration_break().is_ok());
        assert!(cursor.documentation().is_none());
        assert!(cursor.keyword("shape"));
    }
}
