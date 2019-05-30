#![feature(async_await)]

#[macro_use]
extern crate log;

use futures::stream::StreamExt;
use log::LevelFilter;
use simplelog::{Config, TermLogger};

use telegram::{
    chat::ChatType,
    update::{MessageData, Update},
    Telegram,
};

mod telegram;

async fn handle_update(update: Update) {
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
    let client = Telegram::new(std::env::var("TELEGRAM_BOT_TOKEN").unwrap());
    let updates = client.updates();

    info!("Running bot...");
    updates
        .for_each(|f| runtime::spawn(handle_update(f)))
        .await;
}
