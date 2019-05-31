pub mod chat;
pub mod message;
pub mod update;
pub mod user;

use futures::compat::*;
use futures01::{future::Future as Future01, stream::Stream};
use message::Message;
use reqwest::{r#async::Client, Url};
use serde::Deserialize;
use update::UpdateStream;
use user::User;

#[derive(Deserialize, Debug)]
struct ApiChat {
    id: i64,
    #[serde(rename = "type")]
    chat_type: String,
    title: Option<String>,
    username: Option<String>,
}

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
struct ApiMessage {
    message_id: u64,
    from: Option<User>,
    date: u64,
    text: Option<String>,
    forward_from: Option<User>,
    chat: ApiChat,
}

#[derive(Clone)]
pub struct Telegram {
    client: Client,
    base_url: String,
    bot_user: User,
    bot_mention: String,
}

impl Telegram {
    pub async fn new(token: String) -> Self {
        //Get the bot user info
        let client = Client::new();
        let base_url = format!("https://api.telegram.org/bot{}", token);
        let url = Url::parse(&format!("{}/{}", base_url, "getMe")).unwrap();

        //TODO return error, use a more generic struct for api responses
        #[derive(Deserialize)]
        struct Response {
            ok: bool,
            result: User,
        }

        let bot_user = client
            .get(url)
            .send()
            .and_then(|response| response.into_body().concat2())
            .map(|f| serde_json::from_slice(&f).unwrap())
            .map(|json: Response| json.result)
            .map_err(|_| ())
            .compat()
            .await
            .unwrap();

        let bot_mention = format!("@{}", bot_user.username.as_ref().unwrap());

        Self {
            client,
            base_url,
            bot_user,
            bot_mention,
        }
    }

    pub fn bot_mention<'a>(&'a self) -> &'a str {
        &self.bot_mention
    }

    pub fn updates<'a>(&'a self) -> UpdateStream<'a> {
        let url = self.get_url("getUpdates");
        UpdateStream::new(&self.client, url)
    }

    fn get_url(&self, endpoint: &str) -> Url {
        Url::parse(&format!("{}/{}", self.base_url, endpoint)).unwrap()
    }

    async fn send_message_data(
        &self,
        chat_id: i64,
        serialized: serde_json::Value,
    ) -> Result<Message, ()> {
        let url = self.get_url("sendMessage");

        #[derive(Deserialize)]
        struct Response {
            ok: bool,
            result: ApiMessage,
        };

        self.client
            .get(url)
            .json(&serialized)
            .send()
            .and_then(|response| response.into_body().concat2())
            .map(|f| serde_json::from_slice(&f).unwrap())
            .map_err(|_| ())
            .map(|m: Response| m.result.into())
            .compat()
            .await
    }

    pub async fn send_message_silent(&self, chat_id: i64, text: String) -> Result<Message, ()> {
        let json = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "disable_notification": true,
        });
        self.send_message_data(chat_id, json).await
    }

    //TODO return proper error type
    pub async fn send_message(&self, chat_id: i64, text: String) -> Result<Message, ()> {
        let json = serde_json::json!({
            "chat_id": chat_id,
            "text": text
        });
        self.send_message_data(chat_id, json).await
    }

    pub async fn get_chat_member(&self, chat_id: i64, user_id: i64) -> Result<User, ()> {
        let url = self.get_url("getChatMember");
        let json = serde_json::json!({
            "chat_id": chat_id,
            "user_id": user_id,
        });

        #[derive(Deserialize)]
        struct Response {
            ok: bool,
            result: User,
        };

        self.client
            .get(url)
            .json(&json)
            .send()
            .and_then(|response| response.into_body().concat2())
            .map(|f| serde_json::from_slice(&f).unwrap())
            .map(|u: Response| u.result)
            .map_err(|_| ())
            .compat()
            .await
    }
}
