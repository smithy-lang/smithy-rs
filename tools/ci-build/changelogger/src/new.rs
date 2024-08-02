/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::Context;
use clap::Parser;
use smithy_rs_tool_common::changelog::{FrontMatter, Markdown, Reference, Target};
use smithy_rs_tool_common::git::find_git_repository_root;
use smithy_rs_tool_common::here;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Parser, Debug, Eq, PartialEq)]
pub struct NewArgs {
    /// Target audience for the change (if not provided, user's editor will open for authoring one)
    #[clap(long, short = 't')]
    pub applies_to: Option<Vec<Target>>,
    /// List of git usernames for the authors of the change (if not provided, user's editor will open for authoring one)
    #[clap(long, short)]
    pub authors: Option<Vec<String>>,
    /// List of relevant issues and PRs (if not provided, user's editor will open for authoring one)
    #[clap(long, short)]
    pub references: Option<Vec<Reference>>,
    /// Whether or not the change contains a breaking change (defaults to false)
    #[clap(long, action)]
    pub breaking: bool,
    /// Whether or not the change implements a new feature (defaults to false)
    #[clap(long, action)]
    pub new_feature: bool,
    /// Whether or not the change fixes a bug (defaults to false)
    #[clap(long, short, action)]
    pub bug_fix: bool,
    /// The changelog entry message (if not provided, user's editor will open for authoring one)
    #[clap(long, short)]
    pub message: Option<String>,
    /// Basename of a changelog markdown file (defaults to a random 6-digit basename)
    #[clap(long)]
    pub basename: Option<PathBuf>,
}

impl From<NewArgs> for Markdown {
    fn from(value: NewArgs) -> Self {
        Markdown {
            front_matter: FrontMatter {
                applies_to: value.applies_to.unwrap_or_default().into_iter().collect(),
                authors: value.authors.unwrap_or_default(),
                references: value.references.unwrap_or_default(),
                breaking: value.breaking,
                new_feature: value.new_feature,
                bug_fix: value.bug_fix,
            },
            message: value.message.unwrap_or_default(),
        }
    }
}

pub fn subcommand_new(args: NewArgs) -> anyhow::Result<()> {
    let mut md_full_filename = find_git_repository_root("smithy-rs", ".").context(here!())?;
    md_full_filename.push(".changelog");
    md_full_filename.push(args.basename.clone().unwrap_or(PathBuf::from(format!(
            "{:?}.md",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("should get the current Unix epoch time")
                .as_secs(),
        ))));

    let changelog_entry = new_entry(Markdown::from(args))?;
    std::fs::write(&md_full_filename, &changelog_entry).with_context(|| {
        format!(
            "failed to write the following changelog entry to {:?}:\n{}",
            md_full_filename.as_path(),
            changelog_entry
        )
    })?;

    println!(
        "\nThe following changelog entry has been written to {:?}:\n{}",
        md_full_filename.as_path(),
        changelog_entry
    );

    Ok(())
}

fn new_entry(markdown: Markdown) -> anyhow::Result<String> {
    // Due to the inability for `serde_yaml` to output single line array syntax, an array of values
    // will be serialized as follows:
    //
    // key:
    // - value1
    // - value2
    //
    // as opposed to:
    //
    // key: [value1, value2]
    //
    // This doesn't present practical issues when rendering changelogs. See
    // https://github.com/dtolnay/serde-yaml/issues/355
    let front_matter = serde_yaml::to_string(&markdown.front_matter)?;
    // the last `\n` satisfies the `fix end of files` check in `sdk-lints`
    let changelog_entry = format!("---\n{}---\n{}\n", front_matter, markdown.message);
    let changelog_entry = if any_required_field_needs_to_be_filled(&markdown) {
        edit::edit(changelog_entry).context("failed while editing changelog entry)")?
    } else {
        changelog_entry
    };

    Ok(changelog_entry)
}

fn any_required_field_needs_to_be_filled(markdown: &Markdown) -> bool {
    macro_rules! any_empty {
        () => { false };
        ($head:expr $(, $tail:expr)*) => {
            $head.is_empty() || any_empty!($($tail),*)
        };
    }
    any_empty!(
        &markdown.front_matter.applies_to,
        &markdown.front_matter.authors,
        &markdown.front_matter.references,
        &markdown.message
    )
}

#[cfg(test)]
mod tests {
    use crate::new::{any_required_field_needs_to_be_filled, new_entry, NewArgs};
    use smithy_rs_tool_common::changelog::{Reference, Target};
    use std::str::FromStr;

    #[test]
    fn test_new_entry_from_args() {
        // make sure `args` populates required fields (so the function
        // `any_required_field_needs_to_be_filled` should return false), otherwise an editor would
        // be opened during the test execution for human input, causing the test to get struck
        let args = NewArgs {
            applies_to: Some(vec![Target::Client]),
            authors: Some(vec!["ysaito1001".to_owned()]),
            references: Some(vec![Reference::from_str("smithy-rs#1234").unwrap()]),
            breaking: false,
            new_feature: true,
            bug_fix: false,
            message: Some("Implement a long-awaited feature for S3".to_owned()),
            basename: None,
        };
        let markdown = args.into();
        assert!(
            !any_required_field_needs_to_be_filled(&markdown),
            "one or more required fields were not populated"
        );

        let expected = "---\napplies_to:\n- client\nauthors:\n- ysaito1001\nreferences:\n- smithy-rs#1234\nbreaking: false\nnew_feature: true\nbug_fix: false\n---\nImplement a long-awaited feature for S3\n";
        let actual = new_entry(markdown).unwrap();

        assert_eq!(expected, &actual);
    }
}
