pub mod chat;
pub mod message;
pub mod update;
pub mod user;

use crate::redis::RedisConnection;
use futures::compat::*;
use futures::prelude::*;
use futures01::{future::Future as Future01, stream::Stream};
use message::Message;
use reqwest::{
    r#async::{multipart, Client},
    Url,
};
use serde::Deserialize;
use std::fmt;
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

#[derive(Clone, Debug, Deserialize)]
pub struct Sticker {
    pub file_id: String,
    pub width: u32,
    pub height: u32,
    pub emoji: Option<String>,
    pub set_name: Option<String>,
    pub file_size: Option<usize>,
}

impl fmt::Display for Sticker {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(ref emoji) = self.emoji {
            write!(f, "{} ", emoji)?;
        }

        write!(f, "Sticker")?;
        if let Some(ref set) = self.set_name {
            write!(f, " from pack {}", set)?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct ApiMessage {
    #[serde(rename = "message_id")]
    id: u64,
    from: Option<User>,
    date: u64,
    text: Option<String>,
    forward_from: Option<User>,
    reply_to_message: Option<Box<ApiMessage>>,
    sticker: Option<Sticker>,
    chat: ApiChat,
}

#[derive(Clone)]
pub struct Telegram {
    client: Client,
    base_url: String,
    bot_user: User,
    bot_mention: String,
    token: String,
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
        debug!("This bot is named {}", bot_mention);

        Self {
            client,
            base_url,
            bot_user,
            bot_mention,
            token,
        }
    }

    pub fn bot_mention(&self) -> &str {
        &self.bot_mention
    }

    pub fn bot_user(&self) -> &User {
        &self.bot_user
    }

    pub fn updates(&self) -> UpdateStream<'_> {
        let url = self.get_url("getUpdates");
        UpdateStream::new(&self.client, url)
    }

    fn get_url(&self, endpoint: &str) -> Url {
        Url::parse(&format!("{}/{}", self.base_url, endpoint)).unwrap()
    }

    async fn send_message_raw(&self, serialized: serde_json::Value) -> Result<Message, ()> {
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

    pub async fn reply_with_markup(
        &self,
        msg_id: u64,
        chat_id: i64,
        text: String,
        markup: serde_json::Value,
    ) -> Result<Message, ()> {
        let json = serde_json::json!({
            "reply_to_message_id": msg_id,
            "chat_id": chat_id,
            "text": text,
            "disable_notification": true,
            "reply_markup": markup
        });
        self.send_message_raw(json).await
    }

    pub async fn send_message_silent(&self, chat_id: i64, text: String) -> Result<Message, ()> {
        let json = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "disable_notification": true,
        });
        self.send_message_raw(json).await
    }

    //TODO return proper error type
    pub async fn send_message(&self, chat_id: i64, text: String) -> Result<Message, ()> {
        let json = serde_json::json!({
            "chat_id": chat_id,
            "text": text
        });
        self.send_message_raw(json).await
    }

    //Returns redis path of the downloaded file
    pub async fn download_file<'a>(
        &'a self,
        mut redis: RedisConnection,
        file_id: &'a str,
    ) -> Result<String, ()> {
        #[derive(Deserialize)]
        struct File {
            file_path: String,
        }
        #[derive(Deserialize)]
        struct FileResp {
            ok: bool,
            result: File,
        }

        let get_file = self.get_url("getFile");
        let json = serde_json::json!({ "file_id": file_id });
        let file_path = self
            .client
            .get(get_file)
            .json(&json)
            .send()
            .and_then(|response| response.into_body().concat2())
            .map(|f| serde_json::from_slice(&f).unwrap())
            .map(|m: FileResp| m.result.file_path)
            .map_err(|e| error!("Failed to get download link for {}: {:?}", file_id, &e))
            .compat()
            .await?;

        let file: Vec<u8> = self
            .client
            .get(
                Url::parse(&format!(
                    "https://api.telegram.org/file/bot{}/{}",
                    self.token, file_path
                ))
                .unwrap(),
            )
            .send()
            .and_then(|response| response.into_body().concat2())
            .map(|f| f.iter().cloned().collect::<Vec<u8>>())
            .map_err(|e| error!("Failed to download {}: {:?}", file_id, &e))
            .compat()
            .await?;

        let key = format!("tg.download.{}", file_id);

        redis
            .set(&key, &file)
            .map_err(|e| error!("Couldn't set key {}: {:?}", key, e))
            .await?;

        Ok(key)
    }

    //TODO take slice instead when multiple lifetimes in async fns are a thing
    pub async fn send_photo<'a>(
        &'a self,
        chat_id: i64,
        data: Vec<u8>,
        caption: Option<String>,
        silent: bool,
    ) -> Result<Message, ()> {
        let url = self.get_url("sendPhoto");
        let form = multipart::Form::new()
            .part("photo", multipart::Part::bytes(data).file_name("image.png"))
            .part("chat_id", multipart::Part::text(chat_id.to_string()))
            .part(
                "disable_notification",
                multipart::Part::text(silent.to_string()),
            );

        let form = if let Some(c) = caption {
            form.part("caption", multipart::Part::text(c))
        } else {
            form
        };

        #[derive(Deserialize)]
        struct Response {
            ok: bool,
            result: ApiMessage,
        }

        self.client
            .post(url)
            .multipart(form)
            .send()
            .and_then(|response| response.into_body().concat2())
            .map(|f| serde_json::from_slice(&f).unwrap())
            .map(|u: Response| u.result.into())
            .map_err(|_| ())
            .compat()
            .await
    }

    pub async fn get_chat_member(&self, chat_id: i64, user_id: i64) -> Result<User, ()> {
        let url = self.get_url("getChatMember");
        let json = serde_json::json!({
            "chat_id": chat_id,
            "user_id": user_id,
        });

        #[derive(Deserialize)]
        struct RespResult {
            user: User,
        }

        #[derive(Deserialize)]
        struct Response {
            ok: bool,
            result: RespResult,
        };

        self.client
            .get(url)
            .json(&json)
            .send()
            .and_then(|response| response.into_body().concat2())
            .map(|f| serde_json::from_slice(&f).unwrap())
            .map(|u: Response| u.result.user)
            .map_err(|_| ())
            .compat()
            .await
    }
}
