/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::page::{File, Page};
use html_escape::encode_safe;
use std::fs;
use std::io::{Result, Write};
use std::path::{Path, PathBuf};
use unidiff::Line;

pub fn write_html(
    output_dir: &Path,
    title: Option<String>,
    subtitle: Option<String>,
    pages: &[Page],
) -> Result<()> {
    for (page_num, page) in pages.iter().enumerate() {
        let file_path = file_path(output_dir, page_num);
        let mut file = fs::File::create(file_path)?;

        write_header(&mut file, title.as_deref(), subtitle.as_deref(), pages)?;
        for (file_num, page_file) in page.files.iter().enumerate() {
            write_file(&mut file, page_file, page_num, file_num)?;
        }
        write_footer(&mut file, page_num, pages.len())?;
    }
    Ok(())
}

fn file_id(page_num: usize, file_num: usize) -> String {
    format!("file-{page_num}-{file_num}")
}

fn file_name(page_num: usize) -> String {
    match page_num {
        0 => "index.html".into(),
        _ => format!("index_page_{page_num}.html"),
    }
}

fn file_path(output_dir: &Path, page_num: usize) -> PathBuf {
    output_dir.join(file_name(page_num))
}

fn write_header<W: Write>(
    mut w: W,
    title: Option<&str>,
    subtitle: Option<&str>,
    pages: &[Page],
) -> Result<()> {
    let title = encode_safe(title.unwrap_or("Diff"));
    writeln!(w, "<!doctype html>")?;
    writeln!(w, "<html>")?;
    writeln!(w, "<head>")?;
    writeln!(w, "  <metadata charset=\"utf-8\">")?;
    writeln!(w, "  <title>{title}</title>",)?;
    writeln!(w, "  <link rel=\"stylesheet\" href=\"https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.7.0/styles/default.min.css\" />")?;
    writeln!(w, "  <script src=\"https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.7.0/highlight.min.js\"></script>")?;
    writeln!(w, "  <style>\n{}\n</style>", include_str!("difftags.css"))?;
    writeln!(w, "  <script>\n{}\n</script>", include_str!("difftags.js"))?;
    writeln!(w, "</head>")?;
    writeln!(w, "<body>")?;
    writeln!(w, "  <h1>{title}</h1>")?;
    if let Some(subtitle) = subtitle {
        writeln!(w, "  <p>{subtitle}</p>")?;
    }

    writeln!(w, "  <h3>Files changed:</h3>")?;
    writeln!(w, "  <ul>")?;
    for (page_num, page) in pages.iter().enumerate() {
        for (file_num, page_file) in page.files.iter().enumerate() {
            writeln!(
                w,
                "    <li><a href=\"{}#{}\">{}</a></li>",
                file_name(page_num),
                file_id(page_num, file_num),
                encode_safe(page_file.name())
            )?;
        }
    }
    writeln!(w, "  </ul>")?;

    Ok(())
}

fn write_footer<W: Write>(mut w: W, page_num: usize, page_count: usize) -> Result<()> {
    writeln!(w, "  <div class=\"pagination\">Pages: ")?;
    for index in 0..page_count {
        writeln!(
            w,
            "  <a href=\"{}\"{}>{}</a>",
            file_name(index),
            if index == page_num {
                " class=\"current-page\""
            } else {
                ""
            },
            index + 1
        )?;
    }
    writeln!(w, "  </div>")?;
    writeln!(w, "  <script>hljs.highlightAll();</script>")?;
    writeln!(w, "</body>")?;
    writeln!(w, "</html>")
}

fn write_file<W: Write>(mut w: W, file: &File, page_num: usize, file_num: usize) -> Result<()> {
    writeln!(w, "  <div class=\"file\">")?;
    writeln!(
        w,
        "    <h3 class=\"file-name\"><a id=\"{}\">{}</a></h3>",
        file_id(page_num, file_num),
        encode_safe(file.name())
    )?;
    if let File::Modified { old_name, .. } = file {
        if file.name() != old_name {
            writeln!(
                w,
                "    <h4 class=\"file-name\">Renamed from {}</h3>",
                encode_safe(old_name)
            )?;
        }
    }

    for (section_num, section) in file.sections().iter().enumerate() {
        writeln!(
            w,
            "    <div class=\"context-row\">@@ -{},{} +{},{} @@</div>",
            section.start_line.0, section.start_line.1, section.end_line.0, section.end_line.1
        )?;
        if let Some(context_prefix) = &section.context_prefix {
            write_diff_table(
                &mut w,
                context_prefix,
                DiffTableType::Prefix,
                page_num,
                file_num,
                section_num * 10000 + 1,
            )?;
        }
        write_diff_table(
            &mut w,
            &section.diff,
            DiffTableType::Main,
            page_num,
            file_num,
            section_num * 10000 + 2,
        )?;
        if let Some(context_suffix) = &section.context_suffix {
            write_diff_table(
                &mut w,
                context_suffix,
                DiffTableType::Suffix,
                page_num,
                file_num,
                section_num * 10000 + 3,
            )?;
        }
    }
    writeln!(w, "  </div>")?;
    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
enum DiffTableType {
    Prefix,
    Main,
    Suffix,
}

fn write_diff_table<W: Write>(
    mut w: W,
    lines: &[Line],
    typ: DiffTableType,
    page_num: usize,
    file_num: usize,
    table_num: usize,
) -> Result<()> {
    let table_id = format!("cd-{page_num}-{file_num}-{table_num}");
    if typ != DiffTableType::Main {
        writeln!(
            w,
            "    <div id=\"{table_id}-exp\" class=\"context-row\"><a href=\"#\" onclick=\"expandTable(event, '{table_id}')\">{} Expand context</a></div>",
            match typ {
                DiffTableType::Prefix => "&#8613;",
                DiffTableType::Suffix => "&#8615;",
                _ => unreachable!(),
            }
        )?;
    }
    writeln!(
        w,
        "    <table id=\"{table_id}\" class=\"diff{}\" cellspacing=\"0\" width=\"100%\">",
        if typ != DiffTableType::Main {
            " hidden"
        } else {
            ""
        }
    )?;
    for line in lines {
        write_line(&mut w, line)?;
    }
    writeln!(w, "    </table>")
}

fn write_line<W: Write>(mut w: W, line: &Line) -> Result<()> {
    write!(
        w,
        "      <tr{}>",
        match line.line_type.as_str() {
            "-" => " class=\"lr\"",
            "+" => " class=\"la\"",
            _ => "",
        }
    )?;
    write!(
        w,
        "<td class=\"lineno\"><pre>{:>5}  {:>5}  {}</pre></td>",
        line.source_line_no
            .map(|n| n.to_string())
            .unwrap_or_else(|| "".to_string()),
        line.target_line_no
            .map(|n| n.to_string())
            .unwrap_or_else(|| "".to_string()),
        line.line_type
    )?;
    writeln!(
        w,
        "<td><pre><code>{}</code></pre></td></tr>",
        encode_safe(&line.value)
    )
}
