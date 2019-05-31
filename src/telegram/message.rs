use super::{chat::Chat, user::User, ApiMessage};
use std::convert::From;

#[derive(Debug)]
pub struct Message {
    pub from: User,
    pub date: u64,
    pub data: MessageData,
    pub chat: Chat,
}

impl From<ApiMessage> for Message {
    fn from(message: ApiMessage) -> Self {
        let date = message.date;
        let chat = message.chat.into();
        let data = if let Some(text) = message.text {
            if let Some(forwarded) = message.forward_from {
                MessageData::Forward(forwarded, text)
            } else {
                MessageData::Text(text)
            }
        } else {
            MessageData::Other
        };

        Self {
            from: message.from.unwrap(),
            date,
            data,
            chat,
        }
    }
}

#[derive(Debug)]
pub enum MessageData {
    Text(String),
    Forward(User, String),
    //Unsupported
    Other,
}
