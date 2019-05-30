use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct User {
    pub id: u64,
    pub first_name: String,
    pub is_bot: bool,
    pub last_name: Option<String>,
    pub username: Option<String>,
}
