use crate::{
    commands::{self, handle_command},
    include_sql, params,
    telegram::{
        chat::ChatType,
        message::{Message, MessageData},
        update::Update,
        Telegram,
    },
    util::{calculate_sticker_hash, get_user_id},
    Context,
};
use tokio_postgres::types::Type;

pub async fn handle_update(update: Update, telegram: &Telegram, context: &Context) {
    use Update::*;
    match update {
        Message(ref msg) => {
            handle_message(msg, &telegram, context).await;
        }
        MessageEdited(msg) => {
            let conn = context.db_pool.get().await.unwrap();
            conn.execute(
                include_sql!("logedit.sql"),
                params![msg.chat.id, msg.from.id, msg.id],
            )
            .await
            .unwrap();

            info!("[{}] user {} edited message {}", msg.chat, msg.from, msg.id,)
        }
        _ => warn!("Update event {:?} not handled!", update),
    }
}

async fn log_message(telegram: &Telegram, msg: &Message, context: &Context) {
    match msg.data {
        MessageData::Text(ref text) => {
            let conn = context.db_pool.get().await.unwrap();
            let stmt = conn
                .prepare_typed(
                    include_sql!("logmessage.sql"),
                    &[Type::INT8, Type::INT8, Type::INT8, Type::TEXT, Type::INT8],
                )
                .await
                .unwrap();
            conn.execute(
                &stmt,
                params![msg.id, msg.chat.id, msg.from.id, text, msg.date],
            )
            .await
            .unwrap();
        }
        MessageData::Sticker(ref sticker) => {
            let mut redis = context.redis_pool.get().await;
            let set_name: &str = sticker.set_name.as_ref().map(|s| s.as_str()).unwrap_or("");
            let emoji: &str = sticker.emoji.as_ref().map(|s| s.as_str()).unwrap_or("");
            let hash =
                calculate_sticker_hash(telegram, &mut redis, &sticker.file_id, set_name, emoji)
                    .await;
            if hash.is_err() {
                return;
            } //this error gets logged elsewhere anyway

            let conn = context.db_pool.get().await.unwrap();
            let stmt = conn
                .prepare_typed(
                    include_sql!("logsticker.sql"),
                    &[
                        Type::INT8,
                        Type::INT8,
                        Type::INT8,
                        Type::TEXT,
                        Type::TEXT,
                        Type::TEXT,
                        Type::INT8,
                        Type::BYTEA,
                    ],
                )
                .await
                .unwrap();
            conn.execute(
                &stmt,
                params![
                    msg.from.id,
                    msg.chat.id,
                    msg.id,
                    sticker.file_id,
                    sticker.emoji,
                    sticker.set_name,
                    msg.date,
                    hash.unwrap(),
                ],
            )
            .await
            .unwrap();
        }
        _ => (), //other message types are not logged yet
    }
}

async fn handle_text_reply(
    text: &str,
    telegram: &Telegram,
    context: &Context,
    msg: &Message,
    other_message: &Message,
) {
    let mut redis = context.redis_pool.get().await;
    //Check if the message is a reply to a reply command
    let key = format!("tg.replycommand.{}.{}", msg.chat.id, other_message.id);
    match redis.get(&key).await {
        Ok(Some(command)) => {
            let command: commands::ReplyCommand = rmp_serde::from_slice(&command).unwrap();
            let userid = get_user_id(msg.chat.id, &text, &context.db_pool).await;
            if let Some(userid) = userid {
                match command.action {
                    commands::ReplyAction::Quote => {
                        let _ = crate::commands::quote(
                            userid,
                            msg.chat.id,
                            command.command_message_id,
                            telegram,
                            context,
                        )
                        .await
                        .map_err(|e| error!("failed to quote from reply message: {}", e));
                    }
                    commands::ReplyAction::Simulate => {
                        let _ = crate::commands::simulate(
                            userid,
                            msg.chat.id,
                            context.config.markov.chain_order,
                            command.command_message_id,
                            telegram,
                            context,
                        )
                        .await
                        .map_err(|e| error!("failed to simulate from reply message: {}", e));
                    }
                    commands::ReplyAction::AddDisasterPoint => {
                        let _ = crate::commands::disaster::add_point(
                            userid,
                            msg.from.id,
                            msg.chat.id,
                            command.command_message_id,
                            msg.date,
                            telegram,
                            context,
                        )
                        .await
                        .map_err(|e| {
                            error!("failed to add a disaster point from reply message: {}", e)
                        });
                    }
                }
                futures::future::join(
                    telegram.delete_message(msg.chat.id, msg.id),
                    telegram.delete_message(msg.chat.id, other_message.id),
                )
                .await;
            }
        }
        Ok(None) => (),
        Err(e) => error!("redis error getting reply command: {:?}", e),
    }
}

async fn handle_message(msg: &Message, telegram: &Telegram, context: &Context) {
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
                handle_command(&msg, text, &telegram, &context).await;
                should_log = false;
            }
        }
        MessageData::Reply(ref data, ref other_message) => {
            //Replying to the bot
            if other_message.from.id == telegram.bot_user().id {
                should_log = false;
                //Only support text-based reply commands for now
                if let MessageData::Text(ref text) = **data {
                    handle_text_reply(text, &telegram, context, msg, other_message).await;
                } else {
                    return;
                };
            }
        }
        _ => (),
    }

    if should_log {
        //Replies should be logged as normal messages for now
        if let MessageData::Reply(ref data, _) = msg.data {
            let message = msg.with_data(data);
            log_message(&telegram, &message, &context).await;
        } else {
            log_message(&telegram, msg, &context).await;
        }
    }

    //Take a snapshot of the user's data
    let conn = context.db_pool.get().await.unwrap();
    conn.execute(
        include_sql!("updateuserdata.sql"),
        params![
            msg.from.id,
            msg.chat.id,
            msg.from.first_name,
            msg.from.last_name,
            msg.from.username
        ],
    )
    .await
    .unwrap();

    info!("[{}] <{}>: {}", msg.chat, msg.from, msg.data);
}
