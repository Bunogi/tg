use std::convert::From;

#[derive(Debug)]
pub struct Chat {
    pub id: i64,
    pub kind: ChatType,
}

impl From<super::ApiChat> for Chat {
    fn from(from: super::ApiChat) -> Self {
        let id = from.id;
        use ChatType::*;
        let kind = match from.chat_type.as_str() {
            "private" => Private,
            "group" => Group {
                title: from.title.unwrap(),
            },
            "supergroup" => SuperGroup {
                title: from.title.unwrap(),
            },
            "channel" => Channel {
                title: from.title.unwrap(),
            },
            _ => panic!("Invalid chat type from API: {}", from.chat_type),
        };

        Self { id, kind }
    }
}

impl Chat {}

#[derive(Debug)]
pub enum ChatType {
    Private,
    Group { title: String },
    SuperGroup { title: String },
    Channel { title: String },
}
