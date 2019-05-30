use crate::update::ApiMessage;
use crate::user::User;

#[derive(Debug)]
pub struct Message {
    pub from: User,
    pub date: u64,
    pub data: MessageData,
}

#[derive(Debug)]
pub enum MessageData {
    Text(String),
    Forward(User, String),
    //Unsupported
    Other,
}

impl MessageData {
    pub fn new(msg: ApiMessage) -> Self {
        if let Some(text) = msg.text {
            if let Some(forwarded) = msg.forward_from {
                MessageData::Forward(forwarded, text)
            } else {
                MessageData::Text(text)
            }
        } else {
            MessageData::Other
        }
    }
}
