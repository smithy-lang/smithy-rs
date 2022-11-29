use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum AuthorsInner {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "AuthorsInner", into = "AuthorsInner")]
pub struct Authors(pub(super) Vec<String>);

impl From<AuthorsInner> for Authors {
    fn from(value: AuthorsInner) -> Self {
        match value {
            AuthorsInner::Single(author) => Authors(vec![author]),
            AuthorsInner::Multiple(authors) => Authors(authors),
        }
    }
}

impl Into<AuthorsInner> for Authors {
    fn into(mut self) -> AuthorsInner {
        match self.0.len() {
            0 => AuthorsInner::Single("".to_string()),
            1 => AuthorsInner::Single(self.0.pop().unwrap()),
            _ => AuthorsInner::Multiple(self.0),
        }
    }
}

impl Authors {
    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.0.iter()
    }

    // Checks whether the number of authors is 0 or any author has a empty name.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty() || self.iter().any(String::is_empty)
    }

    pub fn validate_usernames(&self) -> Result<()> {
        fn validate_username(author: &str) -> Result<()> {
            if !author.chars().all(|c| c.is_alphanumeric() || c == '-') {
                bail!("Author, \"{author}\", is not a valid GitHub username: [a-zA-Z0-9\\-]")
            }
            Ok(())
        }
        for author in self.iter() {
            validate_username(author)?
        }
        Ok(())
    }
}
