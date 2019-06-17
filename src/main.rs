#![feature(async_await)]

#[macro_use]
extern crate log;
#[macro_use]
extern crate rusqlite;

use crate::redis::RedisConnection;
use crate::telegram::{chat::ChatType, message::MessageData, update::Update, Telegram};
use db::AsyncSqlConnection;
use futures::stream::StreamExt;
use rusqlite::Connection;
use std::process::exit;

mod commands;
mod db;
mod redis;
mod telegram;
mod util;

async fn handle_update(
    context: Telegram,
    update: Update,
    redis: RedisConnection,
    db: AsyncSqlConnection,
) {
    use Update::*;
    match update {
        Message(ref msg) => {
        if let MessageData::Text(ref text) = msg.data {
            //Is command
            if text.chars().nth(0).unwrap() == '/' {
                commands::handle_command(&msg, text, context, redis, db).await;
            } else {
                let unix_time = util::get_unix_timestamp();
                let lock = db.get().await;
                lock.execute(
                    include_sql!("logmessage.sql"),
                    params![
                        msg.chat.id as isize,
                        msg.from.id as isize,
                        text,
                        unix_time as isize
                    ],
                )
                .unwrap();
            }
            info!(
                "[{}] <{}>: {}",
                msg.chat.kind,
                msg.from,
                match &msg.data {
                    MessageData::Text(s) => s.to_string(),
                    MessageData::Forward(u, s) => format!("[Forwarded From {}]: {}", u, s),
                    MessageData::Other => "[Unsupported]".to_string(),
                }
            );
        }
        },
        MessageEdited(msg) => {
            let lock = db.get().await;
            lock.execute(
                include_sql!("logedit.sql"), params![msg.chat.id as isize, msg.from.id as isize, msg.id as isize],
            ).unwrap();
            info!(
                "[{}] user <{}> edited message {}",
                msg.chat.kind,
                msg.from,
                msg.id,
            )
        }
        _ => warn!("Update event {:?} not handled!", update)
    }
}

#[runtime::main(runtime_tokio::Tokio)]
async fn main() -> std::io::Result<()> {
    env_logger::init();
    info!("Connecting to redis...");
    let redis_conn = match redis::RedisConnection::connect().await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to connect to redis: {}", e);
            exit(1);
        }
    };
    info!("Opening database...");
    let db_conn = AsyncSqlConnection::new(Connection::open("logs.db").unwrap());

    {
        info!("Creating tables if necesarry...");
        let db = db_conn.get().await;
        db.execute_batch(include_sql!("create.sql")).unwrap();
        info!("Done!");
    }

    let telegram = Telegram::new(std::env::var("TELEGRAM_BOT_TOKEN").unwrap()).await;

    info!("Running bot...");
    loop {
        telegram
            .updates()
            .for_each(|f| {
                runtime::spawn(handle_update(
                    telegram.clone(),
                    f,
                    redis_conn.clone(),
                    db_conn.clone(),
                ))
            })
            .await;
        info!("BOT: Stream ended, reconnecting");
    }
}
