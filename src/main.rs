#![feature(async_await)]

#[macro_use]
extern crate log;
#[macro_use]
extern crate rusqlite;

use db::AsyncSqlConnection;
use futures::stream::StreamExt;
use log::LevelFilter;
use redis::RedisConnection;
use rusqlite::Connection;
use simplelog::{Config, TermLogger};
use std::time::{SystemTime, UNIX_EPOCH};
use telegram::{chat::ChatType, message::MessageData, update::Update, user::User, Telegram};

mod db;
mod redis;
mod telegram;

//Will get the user from cache if it is cached, otherwise request the data
async fn get_user(
    chat_id: i64,
    user_id: i64,
    context: Telegram,
    mut redis: RedisConnection,
) -> User {
    let user_path = format!("chat.\"{}\".\"{}\"", chat_id, user_id);
    match redis.get(&user_path).await.unwrap() {
        Some(u) => u,
        None => {
            let user = context.get_chat_member(chat_id, user_id).await.unwrap();
            redis.set(&user_path, &user).await;
            user
        }
    }
}

async fn handle_update(
    context: Telegram,
    update: Update,
    redis: RedisConnection,
    db: AsyncSqlConnection,
) {
    use Update::*;
    if let Message(msg) = update {
        if let MessageData::Text(ref text) = msg.data {
            let command: Vec<&str> = text.split_whitespace().collect();
            if command[0] == context.bot_mention() {
                //run command
                // if command.len() > 1 && command[1] == "show" {
                //     struct LoggedMessage {
                //         userid: isize,
                //         message: String,
                //         instant: isize,
                //     }

                //     let conn = db.get().await;
                //     let messages = {
                //         let mut stmt = conn
                //             .prepare_cached(include_sql!("getmessages.sql"))
                //             .unwrap();

                //         stmt.query_map(params![msg.chat.id], |row|
                //             Ok(LoggedMessage {
                //                 userid: row.get(0).unwrap(),
                //                 message: row.get(1).unwrap(),
                //                 instant: row.get(2).unwrap(),
                //             }))
                //         .unwrap()
                //         .collect::<Result<Vec<LoggedMessage>, rusqlite::Error>>()
                //         .unwrap()
                //     };

                //     let mut reply = String::new();
                //     for m in messages {
                //         reply += &format!(
                //             "{}",
                //             format_user(
                //                 get_user(msg.chat.id, m.userid as i64, context.clone(), redis.clone()).await
                //             )
                //         )
                //     }

                //     context.send_message(msg.chat.id, reply).await.unwrap();
                // } else {
                context
                    .send_message_silent(msg.chat.id, "No such command".to_string())
                    .await
                    .unwrap();
            // }
            } else {
                let unix_time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
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
                match msg.chat.kind {
                    ChatType::Private => "<direct>".to_string(),
                    ChatType::Group { ref title } => format!("in group {}", title),
                    ChatType::SuperGroup { ref title } => format!("in supergroup {}", title),
                    ChatType::Channel { ref title } => format!("in channel {}", title),
                },
                msg.from,
                match msg.data {
                    MessageData::Text(s) => s,
                    MessageData::Forward(u, s) => format!("[Forwarded From {}]: {}", u, s),
                    MessageData::Other => "[Unsupported]".to_string(),
                }
            );
        }
    }
}

#[runtime::main(runtime_tokio::Tokio)]
async fn main() {
    TermLogger::init(LevelFilter::Info, Config::default()).unwrap();
    let mut redis_conn = redis::RedisConnection::connect().await.unwrap();
    let mut db_conn = AsyncSqlConnection::new(Connection::open("logs.db").unwrap());

    {
        let db = db_conn.get().await;
        db.execute_batch(include_sql!("create.sql")).unwrap();
    }

    let telegram = Telegram::new(std::env::var("TELEGRAM_BOT_TOKEN").unwrap()).await;
    let updates = telegram.updates();

    info!("Running bot...");
    updates
        .for_each(|f| {
            runtime::spawn(handle_update(
                telegram.clone(),
                f,
                redis_conn.clone(),
                db_conn.clone(),
            ))
        })
        .await;
}
