use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct User {
    pub id: i64,
    pub first_name: String,
    pub is_bot: bool,
    pub last_name: Option<String>,
    pub username: Option<String>,
}

impl fmt::Display for User {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(u) = &self.username {
            write!(f, "{}", u)
        } else if let Some(last) = &self.last_name {
            write!(f, "{} {}", self.first_name, last)
        } else {
            write!(f, "{}", self.first_name)
        }
    }
}
