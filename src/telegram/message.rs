use super::{chat::Chat, user::User, ApiMessage, Sticker};
use std::convert::From;
use std::fmt;

#[derive(Clone, Debug)]
pub struct Message {
    pub id: u64,
    pub from: User,
    pub date: u64,
    pub data: MessageData,
    pub chat: Chat,
}

impl From<ApiMessage> for Message {
    fn from(message: ApiMessage) -> Self {
        let date = message.date;
        let chat = message.chat.into();
        let data = if let Some(msg) = message.reply_to_message {
            //This will not cause infinite recursion because the messages Telegram sends as
            //reply_to_message doesn't contain another replied to message
            let data = if let Some(text) = message.text {
                if let Some(forwarded) = message.forward_from {
                    MessageData::Forward(forwarded, text)
                } else {
                    MessageData::Text(text)
                }
            } else if let Some(sticker) = message.sticker {
                MessageData::Sticker(sticker)
            } else {
                MessageData::Other
            };
            let converted = (*msg).into();
            MessageData::Reply(Box::new(data), Box::new(converted))
        } else if let Some(text) = message.text {
            if let Some(forwarded) = message.forward_from {
                MessageData::Forward(forwarded, text)
            } else {
                MessageData::Text(text)
            }
        } else if let Some(sticker) = message.sticker {
            MessageData::Sticker(sticker)
        } else {
            MessageData::Other
        };

        Self {
            id: message.id,
            from: message.from.unwrap(),
            date,
            data,
            chat,
        }
    }
}

impl Message {
    //Creates a copy of this message and hand it data. Should only be used when working with replies
    pub fn with_data(&self, data: &MessageData) -> Self {
        let mut out = self.clone();
        out.data = data.clone();
        out
    }
}

#[derive(Clone, Debug)]
pub enum MessageData {
    Text(String),
    Forward(User, String),
    Sticker(Sticker),
    Reply(Box<MessageData>, Box<Message>),
    //Unsupported
    Other,
}

impl fmt::Display for MessageData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            MessageData::Text(text) => write!(f, "{}", text),
            MessageData::Forward(u, s) => write!(f, "[Forwarded from {}]: {}", u, s),
            MessageData::Reply(this, other) => write!(f, "[Reply to {}]: {}", other.id, this),
            MessageData::Sticker(s) => write!(f, "[{}]", s),
            MessageData::Other => write!(f, "[Unsupported]"),
        }
    }
}
