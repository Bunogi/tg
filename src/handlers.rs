use crate::commands::handle_command;
use crate::db::SqlPool;
use crate::include_sql;
use crate::redis::RedisPool;
use crate::telegram::{
    message::{Message, MessageData},
    update::Update,
    Telegram,
};

pub async fn handle_update(
    context: Telegram,
    update: Update,
    redis_pool: RedisPool,
    db_pool: SqlPool,
) {
    use Update::*;
    match update {
        Message(ref msg) => {
            handle_message(msg, context.clone(), redis_pool.clone(), db_pool.clone()).await;
        }
        MessageEdited(msg) => {
            let lock = db_pool.get().await;
            lock.execute(
                include_sql!("logedit.sql"),
                params![msg.chat.id as isize, msg.from.id as isize, msg.id as isize],
            )
            .unwrap();
            info!(
                "[{}] user {} edited message {}",
                msg.chat.kind, msg.from, msg.id,
            )
        }
        _ => warn!("Update event {:?} not handled!", update),
    }
}

async fn handle_message(msg: &Message, context: Telegram, redis_pool: RedisPool, db_pool: SqlPool) {
    match msg.data {
        MessageData::Text(ref text) => {
            //Is command
            if text.chars().nth(0).unwrap() == '/' {
                handle_command(&msg, text, context, redis_pool, db_pool.clone()).await;
            } else {
                let unix_time = chrono::Utc::now().timestamp();
                let lock = db_pool.get().await;
                lock.execute(
                    include_sql!("logmessage.sql"),
                    params![
                        msg.id as isize,
                        msg.chat.id as isize,
                        msg.from.id as isize,
                        text,
                        unix_time as isize
                    ],
                )
                .unwrap();
            }
        }
        MessageData::Sticker(ref sticker) => {
            let lock = db_pool.get().await;
            lock.execute(
                include_sql!("logsticker.sql"),
                params![
                    msg.from.id as isize,
                    msg.chat.id as isize,
                    msg.id as isize,
                    sticker.file_id,
                    sticker.emoji,
                    sticker.set_name,
                    msg.date as isize,
                ],
            )
            .unwrap();
        }
        _ => (),
    }

    //Take a snapshot of the user's data
    let conn = db_pool.get().await;
    conn.execute(
        include_sql!("updateuserdata.sql"),
        params![
            msg.from.id as isize,
            msg.chat.id,
            msg.from.first_name,
            msg.from.last_name,
            msg.from.username
        ],
    )
    .unwrap();

    info!(
        "[{}] <{}>: {}",
        msg.chat.kind,
        msg.from,
        match &msg.data {
            MessageData::Text(s) => s.to_string(),
            MessageData::Forward(u, s) => format!("[Forwarded From {}]: {}", u, s),
            MessageData::Other => "[Unsupported]".to_string(),
            MessageData::Sticker(s) => format!("[{}]", s),
        }
    );
}
