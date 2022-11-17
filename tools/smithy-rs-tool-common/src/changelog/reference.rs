use std::{fmt, str::FromStr};

use anyhow::bail;
use serde::{de, Deserialize, Deserializer, Serialize};

use super::Authors;

#[derive(Clone, Debug)]
pub struct ReferenceId {
    pub repo: String,
    pub number: usize,
}

impl ReferenceId {
    pub fn to_md_link(&self) -> String {
        format!(
            "[{repo}#{number}](https://github.com/awslabs/{repo}/issues/{number})",
            repo = self.repo,
            number = self.number
        )
    }
}

impl<'de> Deserialize<'de> for ReferenceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        FromStr::from_str(&s).map_err(de::Error::custom)
    }
}

impl Serialize for ReferenceId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("{}#{}", self.repo, self.number))
    }
}

impl fmt::Display for ReferenceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.repo, self.number)
    }
}

impl FromStr for ReferenceId {
    type Err = anyhow::Error;

    fn from_str(reference: &str) -> std::result::Result<Self, Self::Err> {
        match reference.split_once('#') {
            None => bail!(
                "Reference must of the form `repo#number` but found {}",
                reference
            ),
            Some((repo, number)) => {
                let number = number.parse::<usize>()?;
                if !matches!(repo, "smithy-rs" | "aws-sdk-rust") {
                    bail!("unexpected repo: {}", repo);
                }
                Ok(ReferenceId {
                    number,
                    repo: repo.to_string(),
                })
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]

enum ReferenceItemInner {
    Id(ReferenceId),
    Item { id: ReferenceId, authors: Authors },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(from = "ReferenceItemInner", into = "ReferenceItemInner")]
pub struct ReferenceItem {
    pub id: ReferenceId,
    pub authors: Authors,
}

impl From<ReferenceItemInner> for ReferenceItem {
    fn from(value: ReferenceItemInner) -> Self {
        match value {
            ReferenceItemInner::Id(id) => Self {
                id,
                authors: Default::default(),
            },
            ReferenceItemInner::Item { id, authors } => Self { id, authors },
        }
    }
}

impl Into<ReferenceItemInner> for ReferenceItem {
    fn into(self) -> ReferenceItemInner {
        if self.authors.0.len() > 2 {
            return ReferenceItemInner::Item {
                id: self.id,
                authors: self.authors,
            };
        }
        ReferenceItemInner::Id(self.id)
    }
}
