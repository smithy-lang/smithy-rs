/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::lexer::Cursor;
use super::syntax::{IdlParseError, NodeSyntax, Span, TraitValueSyntax};
use std::collections::HashSet;

impl Cursor<'_> {
    pub(super) fn node(&mut self, depth: usize) -> Result<NodeSyntax, IdlParseError> {
        self.ws0();
        let start = self.pos;
        match self.peek() {
            Some('"') => Ok(NodeSyntax::String(self.string()?)),
            Some('[') => self.node_array(depth),
            Some('{') => self.node_object(depth),
            Some('-' | '0'..='9') => self.number(),
            Some(_) if self.keyword("null") => Ok(NodeSyntax::Null(Span::new(start, self.pos))),
            Some(_) if self.keyword("true") => {
                Ok(NodeSyntax::Bool(true, Span::new(start, self.pos)))
            }
            Some(_) if self.keyword("false") => {
                Ok(NodeSyntax::Bool(false, Span::new(start, self.pos)))
            }
            Some(_) => Ok(NodeSyntax::SyntacticShapeId(self.shape_id()?)),
            None => Err(self.error("expected a node value")),
        }
    }

    fn child_depth(&self, depth: usize) -> Result<usize, IdlParseError> {
        if depth >= self.max_depth {
            Err(IdlParseError::resource(
                self.pos,
                format!("nesting exceeds the maximum depth of {}", self.max_depth),
            ))
        } else {
            Ok(depth + 1)
        }
    }

    fn node_array(&mut self, depth: usize) -> Result<NodeSyntax, IdlParseError> {
        let depth = self.child_depth(depth)?;
        let start = self.pos;
        self.consume("[");
        self.ws0();
        let mut values = Vec::new();
        while !self.consume("]") {
            if self.eof() {
                return Err(IdlParseError::syntax(start, "unterminated array"));
            }
            values.push(self.node(depth)?);
            self.ws0();
        }
        Ok(NodeSyntax::Array(values, Span::new(start, self.pos)))
    }

    fn node_object(&mut self, depth: usize) -> Result<NodeSyntax, IdlParseError> {
        let depth = self.child_depth(depth)?;
        let start = self.pos;
        self.consume("{");
        let entries = self.object_entries(depth, "}")?;
        Ok(NodeSyntax::Object(entries, Span::new(start, self.pos)))
    }

    fn object_entries(
        &mut self,
        depth: usize,
        close: &str,
    ) -> Result<Vec<(super::syntax::Spanned<String>, NodeSyntax)>, IdlParseError> {
        self.ws0();
        let mut seen = HashSet::new();
        let mut entries = Vec::new();
        while !self.consume(close) {
            if self.eof() {
                return Err(self.error(format!("expected `{close}`")));
            }
            let key = self.object_key()?;
            self.ensure_unique(&mut seen, &key)?;
            self.ws0();
            self.expect(":", "`:` after an object key")?;
            let value = self.node(depth)?;
            entries.push((key, value));
            self.ws0();
        }
        Ok(entries)
    }

    /// Parses the contents after a consumed trait `(`, including its closing `)`.
    pub(super) fn trait_value(
        &mut self,
        open: Span,
        depth: usize,
    ) -> Result<TraitValueSyntax, IdlParseError> {
        self.ws0();
        if self.consume(")") {
            return Ok(TraitValueSyntax::Omitted { at: open });
        }

        // Direct key/value trait syntax is selected only once the first key and colon exist.
        let checkpoint = self.pos;
        let structured = if self.starts_with("\"\"\"") {
            false
        } else if matches!(self.peek(), Some('"' | '_' | 'A'..='Z' | 'a'..='z')) {
            match self.object_key() {
                Ok(_) => {
                    self.ws0();
                    self.starts_with(":")
                }
                Err(_) => false,
            }
        } else {
            false
        };
        self.pos = checkpoint;

        if structured {
            let depth = self.child_depth(depth)?;
            let entries = self.object_entries(depth, ")")?;
            Ok(TraitValueSyntax::Explicit(NodeSyntax::Object(
                entries,
                Span::new(open.start, self.pos),
            )))
        } else {
            let value = self.node(depth)?;
            self.ws0();
            self.expect(")", "`)` after a trait value")?;
            Ok(TraitValueSyntax::Explicit(value))
        }
    }

    fn number(&mut self) -> Result<NodeSyntax, IdlParseError> {
        let start = self.pos;
        self.consume("-");
        match self.peek() {
            Some('0') => {
                self.bump();
                if matches!(self.peek(), Some('0'..='9')) {
                    return Err(self.error("a number cannot contain a leading zero"));
                }
            }
            Some('1'..='9') => {
                while matches!(self.peek(), Some('0'..='9')) {
                    self.bump();
                }
            }
            _ => return Err(self.error("expected digits in a number")),
        }
        if self.consume(".") {
            let fraction = self.pos;
            while matches!(self.peek(), Some('0'..='9')) {
                self.bump();
            }
            if self.pos == fraction {
                return Err(self.error("expected digits after the decimal point"));
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.bump();
            if matches!(self.peek(), Some('+' | '-')) {
                self.bump();
            }
            let exponent = self.pos;
            while matches!(self.peek(), Some('0'..='9')) {
                self.bump();
            }
            if self.pos == exponent {
                return Err(self.error("expected exponent digits"));
            }
        }
        if self.peek().is_some_and(|c| {
            c == '.' || c == '#' || c == '$' || c.is_ascii_alphanumeric() || c == '_'
        }) {
            return Err(self.error("invalid character after a number"));
        }
        let text = self.input()[start..self.pos].to_owned();
        Ok(NodeSyntax::Number(super::syntax::Spanned::new(
            text,
            Span::new(start, self.pos),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Result<NodeSyntax, IdlParseError> {
        let mut cursor = Cursor::new(input, 8, 10);
        let node = cursor.node(0)?;
        cursor.ws0();
        if !cursor.eof() {
            return Err(cursor.error("trailing input"));
        }
        Ok(node)
    }

    #[test]
    fn parses_every_node_kind() {
        for value in [
            "null",
            "true",
            "false",
            "-1",
            "2.5e+3",
            "\"x\"",
            "Name",
            "ns.foo#Name$member",
            "[1, \"x\", false]",
            "{a: 1, \"b\": [Thing]}",
        ] {
            parse(value).unwrap_or_else(|err| panic!("{value}: {err:?}"));
        }
    }

    #[test]
    fn rejects_bad_numbers_and_duplicate_keys() {
        for value in ["01", "1.", "1e", "1x", "{a: 1, a: 2}"] {
            assert!(parse(value).is_err(), "accepted {value}");
        }
        assert_eq!(
            parse("{a: 1, a: 2}").unwrap_err().kind,
            super::super::syntax::ParseErrorKind::DuplicateKey
        );
    }

    #[test]
    fn depth_is_bounded() {
        let mut cursor = Cursor::new("[[[]]]", 2, 10);
        assert_eq!(
            cursor.node(0).unwrap_err().kind,
            super::super::syntax::ParseErrorKind::ResourceLimit
        );
    }

    #[test]
    fn structured_and_single_trait_values_differ() {
        let mut cursor = Cursor::new("a: 1, b: [2])", 8, 10);
        let value = cursor.trait_value(Span::new(0, 1), 0).unwrap();
        assert!(matches!(
            value,
            TraitValueSyntax::Explicit(NodeSyntax::Object(_, _))
        ));

        let mut cursor = Cursor::new("[1, 2])", 8, 10);
        let value = cursor.trait_value(Span::new(0, 1), 0).unwrap();
        assert!(matches!(
            value,
            TraitValueSyntax::Explicit(NodeSyntax::Array(_, _))
        ));
    }
}
