mod updatestream;
pub use updatestream::UpdateStream;

use super::{message::Message, ApiUpdate};
use std::convert::From;

#[derive(Debug)]
pub enum Update {
    Message(Message),
    MessageEdited(Message),
    ChannelPost(Message),
    ChannelPostEdited(Message),
}

impl From<ApiUpdate> for Update {
    fn from(from: ApiUpdate) -> Self {
        if let Some(msg) = from.message {
            Update::Message(msg.into())
        } else if let Some(msg) = from.edited_message {
            Update::MessageEdited(msg.into())
        } else if let Some(msg) = from.channel_post {
            Update::ChannelPost(msg.into())
        } else if let Some(msg) = from.edited_channel_post {
            Update::ChannelPostEdited(msg.into())
        } else {
            panic!("Invalid ApiUpdate: {:?}", from)
        }
    }
}
