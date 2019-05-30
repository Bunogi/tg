use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct ApiChat {
    id: i64,
    #[serde(rename = "type")]
    chat_type: String,
    title: Option<String>,
    username: Option<String>,
}

impl ApiChat {
    pub fn as_chat(&self) -> Chat {
        let id = self.id;
        use ChatType::*;
        let kind = match self.chat_type.as_str() {
            "private" => Private,
            "group" => Group {
                title: self.title.clone().unwrap(),
            },
            "supergroup" => SuperGroup {
                title: self.title.clone().unwrap(),
            },
            "channel" => Channel {
                title: self.title.clone().unwrap(),
            },
            _ => panic!("Invalid chat type from API: {}", self.chat_type),
        };

        Chat { id, kind }
    }
}

#[derive(Debug)]
pub struct Chat {
    pub id: i64,
    pub kind: ChatType,
}

#[derive(Debug)]
pub enum ChatType {
    Private,
    Group { title: String },
    SuperGroup { title: String },
    Channel { title: String },
}
