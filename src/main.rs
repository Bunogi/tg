#![feature(async_await)]

#[macro_use]
extern crate log;

use crate::update::{MessageData, Update, UpdateStream};
use futures::stream::StreamExt;
use log::LevelFilter;
use simplelog::{Config, TermLogger};
mod update;
mod user;

async fn handle_update_async(update: Update) {
    use Update::*;
    match update {
        Message(msg) => {
            info!(
                "<{}>: {}",
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

    updates
        .for_each(|f| runtime::spawn(handle_update_async(f)))
        .await;
}
