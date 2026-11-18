/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Prints the token stream of each Rust source file given as an argument.
//!
//! For every file, a `file\t<path>` line is followed by one `<line>\t<token>`
//! line per token. Whitespace and non-doc comments are not tokens, so two files
//! print identically exactly when they differ only in those. Doc comments are
//! kept as `#[doc = "..."]` attributes. Adjacent punctuation is joined into
//! Rust's multi-character operators by longest match, as rustc's lexer does, so
//! `a && b` and `a & &b` stay distinct. Backslashes and newlines in tokens are
//! escaped so each token occupies one line.

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};
use std::io::{self, BufWriter, Write};
use std::process::ExitCode;

/// Multi-character operators, longest first.
const OPERATORS: &[&str] = &[
    "<<=", ">>=", "...", "..=", "::", "->", "=>", "==", "!=", "<=", ">=", "&&", "||", "+=", "-=",
    "*=", "/=", "%=", "^=", "&=", "|=", "<<", ">>", "..",
];

fn write_token(out: &mut impl Write, line: usize, token: &str) -> io::Result<()> {
    let escaped = token.replace('\\', "\\\\").replace('\n', "\\n");
    writeln!(out, "{line}\t{escaped}")
}

/// Writes a run of punctuation with no whitespace between its characters.
fn write_punct_run(out: &mut impl Write, run: &mut Vec<(usize, char)>) -> io::Result<()> {
    let text: String = run.iter().map(|(_, c)| c).collect();
    let mut start = 0;
    while start < run.len() {
        let rest = &text[start..];
        let length = OPERATORS
            .iter()
            .find(|operator| rest.starts_with(*operator))
            .map_or(1, |operator| operator.len());
        write_token(out, run[start].0, &text[start..start + length])?;
        start += length;
    }
    run.clear();
    Ok(())
}

fn write_stream(out: &mut impl Write, stream: TokenStream) -> io::Result<()> {
    let mut run = Vec::new();
    for tree in stream {
        let line = tree.span().start().line;
        if let TokenTree::Punct(punct) = &tree {
            run.push((line, punct.as_char()));
            if punct.spacing() == Spacing::Alone {
                write_punct_run(out, &mut run)?;
            }
            continue;
        }
        write_punct_run(out, &mut run)?;
        match tree {
            TokenTree::Group(group) => {
                let (open, close) = match group.delimiter() {
                    Delimiter::Parenthesis => ("(", ")"),
                    Delimiter::Brace => ("{", "}"),
                    Delimiter::Bracket => ("[", "]"),
                    Delimiter::None => ("", ""),
                };
                if !open.is_empty() {
                    write_token(out, line, open)?;
                }
                write_stream(out, group.stream())?;
                if !close.is_empty() {
                    write_token(out, group.span_close().start().line, close)?;
                }
            }
            TokenTree::Punct(_) => unreachable!("punctuation is buffered above"),
            TokenTree::Ident(ident) => write_token(out, line, &ident.to_string())?,
            TokenTree::Literal(literal) => write_token(out, line, &literal.to_string())?,
        }
    }
    write_punct_run(out, &mut run)
}

fn main() -> ExitCode {
    let mut out = BufWriter::new(io::stdout().lock());
    for path in std::env::args().skip(1) {
        let parsed = std::fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|source| {
                source
                    .parse::<TokenStream>()
                    .map_err(|error| error.to_string())
            });
        let result = match parsed {
            Ok(stream) => {
                writeln!(out, "file\t{path}").and_then(|()| write_stream(&mut out, stream))
            }
            Err(error) => {
                eprintln!("{path}: {error}");
                return ExitCode::FAILURE;
            }
        };
        if let Err(error) = result {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    }
    if let Err(error) = out.flush() {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
