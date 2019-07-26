use crate::commands::{self, handle_command};
use crate::db::SqlPool;
use crate::include_sql;
use crate::telegram::{
    chat::ChatType,
    message::{Message, MessageData},
    update::Update,
    Telegram,
};
use crate::util::get_user_id;

pub async fn handle_update(
    context: Telegram,
    update: Update,
    redis_pool: darkredis::ConnectionPool,
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

async fn log_message(msg: &Message, db_pool: SqlPool) {
    match msg.data {
        MessageData::Text(ref text) => {
            let lock = db_pool.get().await;
            lock.execute(
                include_sql!("logmessage.sql"),
                params![
                    msg.id as isize,
                    msg.chat.id as isize,
                    msg.from.id as isize,
                    text,
                    msg.date as isize
                ],
            )
            .unwrap();
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
        _ => (), //other message types are not logged yet
    }
}

async fn handle_message(
    msg: &Message,
    context: Telegram,
    redis_pool: darkredis::ConnectionPool,
    db_pool: SqlPool,
) {
    //Never log private chats
    let mut should_log = if let ChatType::Private = msg.chat.kind {
        false
    } else {
        true
    };
    match msg.data {
        MessageData::Text(ref text) => {
            //Is command
            if text.chars().nth(0).unwrap() == '/' {
                handle_command(
                    &msg,
                    text,
                    context.clone(),
                    redis_pool.clone(),
                    db_pool.clone(),
                )
                .await;
                should_log = false;
            }
        }
        MessageData::Reply(ref data, ref other_message) => {
            //Replying to the bot
            if other_message.from.id == context.bot_user().id {
                should_log = false;
                //Check and handle commands that support replying
                let mut redis = redis_pool.get().await;
                let key = format!("tg.replycommand.{}.{}", msg.chat.id, other_message.id);
                match redis.get(&key).await {
                    Ok(Some(command)) => {
                        match std::str::from_utf8(&command).unwrap().trim() {
                            //TODO: These could possibly be condensed into a macro for readability,
                            //especially if some kind of enum type is used instead.
                            commands::ACTION_QUOTE => {
                                if let MessageData::Text(ref text) = **data {
                                    let userid =
                                        get_user_id(msg.chat.id, &text, db_pool.clone()).await;
                                    if userid.is_none() {
                                        return;
                                    } else {
                                        let _ = crate::commands::quote(
                                            userid.unwrap(),
                                            msg.chat.id,
                                            msg.id,
                                            context.clone(),
                                            db_pool.clone(),
                                            redis_pool.clone(),
                                        )
                                        .await
                                        .map_err(|e| {
                                            error!("failed to quote from reply message: {:?}", e)
                                        });
                                    }
                                }
                            }
                            commands::ACTION_SIMULATE => {
                                if let MessageData::Text(ref text) = **data {
                                    let userid =
                                        get_user_id(msg.chat.id, &text, db_pool.clone()).await;
                                    if userid.is_none() {
                                        return;
                                    } else {
                                        let _ = crate::commands::simulate(
                                            userid.unwrap(),
                                            msg.chat.id,
                                            msg.id,
                                            context.clone(),
                                            db_pool.clone(),
                                            redis_pool.clone(),
                                        )
                                        .await
                                        .map_err(|e| {
                                            error!("failed to simulate from reply message: {:?}", e)
                                        });
                                    }
                                }
                            }
                            commands::ACTION_ADD_DISASTER_POINT => {
                                if let MessageData::Text(ref text) = **data {
                                    let userid =
                                        get_user_id(msg.chat.id, &text, db_pool.clone()).await;
                                    if userid.is_none() {
                                        return;
                                    } else {
                                        let _ = crate::commands::disaster::add_point(
                                            userid.unwrap(),
                                            &msg,
                                            context.clone(),
                                            db_pool.clone(),
                                            redis_pool.clone(),
                                        )
                                        .await
                                        .map_err(|e| {
                                            error!("failed to add a disaster point from reply message: {:?}", e)
                                        });
                                    }
                                }
                            }
                            _ => error!(
                                "invalid reply command found in redis: {}",
                                &String::from_utf8_lossy(&command)
                            ),
                        }
                    }
                    Ok(None) => (),
                    Err(e) => error!("redis error getting reply command: {:?}", e),
                }
            }
        }
        _ => (),
    }

    if should_log {
        //Replies should be logged as normal messages for now
        if let MessageData::Reply(ref data, _) = msg.data {
            let message = msg.with_data(data);
            log_message(&message, db_pool.clone()).await;
        } else {
            log_message(msg, db_pool.clone()).await;
        }
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

    info!("[{}] <{}>: {}", msg.chat, msg.from, msg.data);
}
