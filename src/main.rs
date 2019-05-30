#![feature(async_await)]

#[macro_use]
extern crate log;

use crate::update::{MessageData, Update, UpdateStream};
use futures::stream::StreamExt;
use log::LevelFilter;
use simplelog::{Config, TermLogger};
use crate::chat::ChatType;
mod update;
mod user;
mod chat;

async fn handle_update_async(update: Update) {
    use Update::*;
    match update {
        Message(msg) => {
            info!(
                "[{}] <{}>: {}",
                match msg.chat.kind {
                    ChatType::Private => "<direct>".to_string(),
                    ChatType::Group {title} => format!("in group {}", title),
                    ChatType::SuperGroup {title} => format!("in supergroup {}", title),
                    ChatType::Channel {title} => format!("in channel {}", title),
                },
                msg.from.first_name,
                match msg.data {
                    MessageData::Text(s) => s,
                    MessageData::Forward(u, s) => {
                        format!("[Forwarded From {}]: {}", u.first_name, s)
                    }
                    MessageData::Other => "[Unsupported]".to_string(),
                }
            );
        }
        _ => (),
    }
}

#[runtime::main(runtime_tokio::Tokio)]
async fn main() {
    TermLogger::init(LevelFilter::Info, Config::default()).unwrap();
    let updates = UpdateStream::new(std::env::var("TELEGRAM_BOT_TOKEN").unwrap());

    info!("Running bot...");
    updates
        .for_each(|f| runtime::spawn(handle_update_async(f)))
        .await;
}
