pub mod chat;
pub mod message;
pub mod update;
pub mod user;

use futures::prelude::*;
use message::Message;
use reqwest::{multipart, Client, Url};
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
}

#[derive(Debug, Deserialize)]
struct ApiUpdate {
    update_id: u64,
    message: Option<ApiMessage>,
    edited_message: Option<ApiMessage>,
    channel_post: Option<ApiMessage>,
    edited_channel_post: Option<ApiMessage>,
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
    id: i64,
    from: Option<User>,
    date: i64,
    text: Option<String>,
    forward_from: Option<User>,
    reply_to_message: Option<Box<ApiMessage>>,
    sticker: Option<Sticker>,
    chat: ApiChat,
}

#[derive(Clone, Debug)]
pub struct Telegram {
    client: Client,
    base_url: String,
    bot_user: User,
    bot_mention: String,
    token: String,
}

#[derive(Deserialize)]
struct Response<T> {
    result: Option<T>,
    ok: bool,
    description: Option<String>,
}

impl Telegram {
    pub async fn connect(token: String) -> Result<Self, String> {
        //Get the bot user info
        let client = Client::new();
        let base_url = format!("https://api.telegram.org/bot{}", token);
        let url = Url::parse(&format!("{}/{}", base_url, "getMe")).unwrap();

        //TODO return error, use a more generic struct for api responses
        #[derive(Deserialize)]
        struct Response {
            result: User,
        }

        let bot_user = client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("sending bot request: {}", e))?
            .json::<Response>()
            .await
            .map_err(|e| format!("deserializing response: {}", e))
            .map(|json: Response| json.result)?;

        let bot_mention = format!("@{}", bot_user.username.as_ref().unwrap());
        info!("This bot is named {}", bot_mention);

        Ok(Self {
            client,
            base_url,
            bot_user,
            bot_mention,
            token,
        })
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

    async fn send_message_raw(&self, serialized: serde_json::Value) -> Result<Message, String> {
        let url = self.get_url("sendMessage");

        let reply = self
            .client
            .get(url)
            .json(&serialized)
            .send()
            .await
            .map_err(|e| format!("posting message: {}", e))?
            .bytes()
            .await;

        let res: Response<ApiMessage> = serde_json::from_slice(&reply.unwrap()).unwrap();

        if res.ok {
            Ok(res.result.unwrap().into())
        } else {
            Err(res.description.unwrap())
        }
    }

    pub async fn reply_to(
        &self,
        msg_id: i64,
        chat_id: i64,
        text: String,
    ) -> Result<Message, String> {
        let json = serde_json::json!({
            "reply_to_message_id": msg_id,
            "chat_id": chat_id,
            "text": text,
        });
        self.send_message_raw(json).await
    }

    pub async fn reply_with_markup(
        &self,
        msg_id: i64,
        chat_id: i64,
        text: String,
        markup: serde_json::Value,
    ) -> Result<Message, String> {
        let json = serde_json::json!({
            "reply_to_message_id": msg_id,
            "chat_id": chat_id,
            "text": text,
            "disable_notification": true,
            "reply_markup": markup
        });
        self.send_message_raw(json).await
    }

    pub async fn send_message_silently_with_markdown(
        &self,
        chat_id: i64,
        text: String,
    ) -> Result<Message, String> {
        let json = serde_json::json!({
            "text": text,
            "chat_id": chat_id,
            "disable_notification": true,
            "parse_mode": "Markdown"
        });
        self.send_message_raw(json).await
    }

    pub async fn reply_and_close_keyboard(
        &self,
        msg_id: i64,
        chat_id: i64,
        text: String,
    ) -> Result<Message, String> {
        let json = serde_json::json!({
            "reply_to_message_id": msg_id,
            "chat_id": chat_id,
            "text": text,
            "disable_notification": true,
            "reply_markup": {
                "remove_keyboard": true,
                "selective": true
            }
        });
        self.send_message_raw(json).await
    }

    pub async fn send_message_silent(&self, chat_id: i64, text: String) -> Result<Message, String> {
        let json = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "disable_notification": true,
        });
        self.send_message_raw(json).await
    }

    // pub async fn send_message(&self, chat_id: i64, text: String) -> Result<Message, ()> {
    //     let json = serde_json::json!({
    //         "chat_id": chat_id,
    //         "text": text
    //     });
    //     self.send_message_raw(json).await
    // }

    pub async fn download_file(
        &self,
        redis: &mut darkredis::Connection,
        file_id: &str,
    ) -> Result<Vec<u8>, String> {
        let key = format!("tg.download.{}", file_id);
        match redis.get(&key).await {
            Ok(Some(f)) => Ok(f),
            Ok(None) => {
                info!("Downloading file {} from Telegram...", file_id);
                #[derive(Deserialize)]
                struct File {
                    file_path: String,
                }

                let get_file = self.get_url("getFile");
                let json = serde_json::json!({ "file_id": file_id });
                let file = self
                    .client
                    .get(get_file)
                    .json(&json)
                    .send()
                    .await
                    .unwrap()
                    .json::<Response<File>>()
                    .await
                    .unwrap();

                if file.ok {
                    let file = self
                        .client
                        .get(
                            Url::parse(&format!(
                                "https://api.telegram.org/file/bot{}/{}",
                                self.token,
                                file.result.unwrap().file_path
                            ))
                            .unwrap(),
                        )
                        .send()
                        .await
                        .unwrap()
                        .bytes()
                        .await
                        .map_err(|e| format!("Failed to download {}: {:?}", file_id, &e))?;

                    redis
                        .set(&key, &file)
                        .map_err(|e| format!("Couldn't set key {}: {:?}", key, e))
                        .await?;

                    Ok(file.to_vec())
                } else {
                    Err(format!(
                        "couldn't get download link for {}: {}",
                        file_id,
                        file.description.unwrap()
                    ))
                }
            }
            Err(e) => Err(format!(
                "Failed to get possibly predownloaded file {}: {}",
                file_id, e
            )),
        }
    }

    // pub async fn send_photo<'a>(
    //     &'a self,
    //     chat_id: i64,
    //     data: Vec<u8>,
    //     caption: Option<String>,
    //     silent: bool,
    // ) -> Result<Message, ()> {
    //     let url = self.get_url("sendPhoto");
    //     let form = multipart::Form::new()
    //         .part("photo", multipart::Part::bytes(data).file_name("image.png"))
    //         .part("chat_id", multipart::Part::text(chat_id.to_string()))
    //         .part(
    //             "disable_notification",
    //             multipart::Part::text(silent.to_string()),
    //         );

    //     let form = if let Some(c) = caption {
    //         form.part("caption", multipart::Part::text(c))
    //     } else {
    //         form
    //     };

    //     #[derive(Deserialize)]
    //     struct Response {
    //         ok: bool,
    //         result: ApiMessage,
    //     }

    //     self.client
    //         .post(url)
    //         .multipart(form)
    //         .send()
    //         .and_then(|response| response.into_body().concat2())
    //         .map(|f| serde_json::from_slice(&f).unwrap())
    //         .map(|u: Response| u.result.into())
    //         .map_err(|_| ())
    //         .compat()
    //         .await
    // }

    //Send a message in png format, panics if data is not a valid PNG image
    pub async fn send_png_lossless(
        &self,
        chat_id: i64,
        data: Vec<u8>,
        caption: Option<String>,
        silent: bool,
    ) -> Result<Message, String> {
        let url = self.get_url("sendDocument");
        let form = multipart::Form::new()
            .part(
                "document",
                multipart::Part::bytes(data).file_name("image.png"),
            )
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

        let reply = self
            .client
            .post(url)
            .multipart(form)
            .send()
            .await
            .unwrap()
            .json::<Response<ApiMessage>>()
            .await
            .unwrap();

        if reply.ok {
            Ok(reply.result.unwrap().into())
        } else {
            Err(reply.description.unwrap())
        }
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
            result: RespResult,
        }

        self.client
            .get(url)
            .json(&json)
            .send()
            .await
            .unwrap()
            .json::<Response>()
            .await
            .map(|u| u.result.user)
            .map_err(|_| ())
    }

    pub async fn delete_message(&self, chat_id: i64, message_id: i64) {
        let url = self.get_url("deleteMessage");
        let json = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id
        });

        let result = self
            .client
            .get(url)
            .json(&json)
            .send()
            .await
            .unwrap()
            .json::<Response<bool>>()
            .await
            .unwrap();

        if !result.ok {
            error!(
                "Couldn't delete message {} in chat {}: \"{}\"",
                message_id,
                chat_id,
                result.description.unwrap()
            );
        }
    }
}
