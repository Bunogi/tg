mod message;
mod updatestream;

pub use message::*;
pub use updatestream::UpdateStream;

use super::chat::ApiChat;
use super::user::User;
use serde::Deserialize;
use std::collections::VecDeque;

#[derive(Debug, Deserialize)]
struct ApiUpdate {
    update_id: u64,
    message: Option<ApiMessage>,
    edited_message: Option<ApiMessage>,
    channel_post: Option<ApiMessage>,
    edited_channel_post: Option<ApiMessage>,
    // inline_query: InlineQuery,
    // chosen_inline: ChosenInlineResult,
    // callback_query: CallbackQuery,
    // shipping_query: ShippingQuery,
    // pre_checkout_query: PreCheckOutQuery,
}

#[derive(Debug, Deserialize)]
pub struct ApiMessage {
    message_id: u64,
    from: Option<User>,
    date: u64,
    text: Option<String>,
    forward_from: Option<User>,
    chat: ApiChat,
}

impl ApiMessage {
    fn into_message(self) -> Message {
        let from = self.from.as_ref().unwrap().clone();
        let date = self.date;
        let chat = ApiChat::as_chat(&self.chat);
        let data = MessageData::new(self);

        Message {
            from,
            date,
            data,
            chat,
        }
    }
}

#[derive(Debug)]
pub enum Update {
    Message(Message),
    MessageEdited(Message),
    ChannelPost(Message),
    ChannelPostEdited(Message),
}

impl ApiUpdate {
    fn into_update(self) -> Update {
        if let Some(msg) = self.message {
            Update::Message(msg.into_message())
        } else if let Some(msg) = self.edited_message {
            Update::MessageEdited(msg.into_message())
        } else if let Some(msg) = self.channel_post {
            Update::ChannelPost(msg.into_message())
        } else if let Some(msg) = self.edited_channel_post {
            Update::ChannelPostEdited(msg.into_message())
        } else {
            panic!("Invalid ApiUpdate: {:?}", self)
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    ok: bool,
    #[serde(rename = "result")]
    updates: VecDeque<ApiUpdate>,
}
