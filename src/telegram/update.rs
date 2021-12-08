mod updatestream;
pub use updatestream::UpdateStream;

use super::{message::Message, ApiUpdate};
use std::convert::TryFrom;

#[derive(Debug)]
pub enum Update {
    Message(Message),
    MessageEdited(Message),
    ChannelPost(Message),
    ChannelPostEdited(Message),
}

impl TryFrom<ApiUpdate> for Update {
    type Error = ();

    fn try_from(from: ApiUpdate) -> Result<Self, Self::Error> {
        if let Some(msg) = from.message {
            Ok(Update::Message(msg.into()))
        } else if let Some(msg) = from.edited_message {
            Ok(Update::MessageEdited(msg.into()))
        } else if let Some(msg) = from.channel_post {
            Ok(Update::ChannelPost(msg.into()))
        } else if let Some(msg) = from.edited_channel_post {
            Ok(Update::ChannelPostEdited(msg.into()))
        } else {
            Err(())
        }
    }
}
