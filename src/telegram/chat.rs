use std::convert::From;
use std::fmt;

#[derive(Clone, Debug)]
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

impl fmt::Display for Chat {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.kind {
            ChatType::Channel { ref title }
            | ChatType::Group { ref title }
            | ChatType::SuperGroup { ref title } => write!(f, "{} {}", self.kind, title),
            ChatType::Private => write!(f, "{}", self.kind),
        }
    }
}

#[derive(Clone, Debug)]
pub enum ChatType {
    Private,
    Group { title: String },
    SuperGroup { title: String },
    Channel { title: String },
}

impl fmt::Display for ChatType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ChatType::Private => write!(f, "private"),
            ChatType::Group { .. } => write!(f, "group"),
            ChatType::SuperGroup { .. } => write!(f, "supergroup"),
            ChatType::Channel { .. } => write!(f, "channel"),
        }
    }
}
