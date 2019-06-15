use crate::db::AsyncSqlConnection;
use crate::include_sql;
use crate::redis::RedisConnection;
use crate::telegram::{message::Message, Telegram};
use crate::util::get_user;

pub async fn handle_command(
    msg: &Message,
    command: Vec<String>,
    context: Telegram,
    redis: RedisConnection,
    db: AsyncSqlConnection,
) {
    if command.get(1) == Some(&"show".to_string()) {
        let reply = async {
            struct LoggedMessage {
                userid: isize,
                messages: isize,
            }

            let conn = db.get().await;
            let messages = {
                let mut stmt = conn
                    .prepare_cached(include_sql!("getmessages.sql"))
                    .unwrap();

                stmt.query_map(params![msg.chat.id], |row| {
                    Ok(LoggedMessage {
                        userid: row.get(0).unwrap(),
                        messages: row.get(1).unwrap(),
                    })
                })
                .unwrap()
                .collect::<Result<Vec<LoggedMessage>, rusqlite::Error>>()
                .unwrap()
            };

            let mut reply = String::new();
            for m in messages {
                let appendage = format!(
                    "{}: {} messages",
                    get_user(msg.chat.id, m.userid as i64, context.clone(), redis.clone())
                        .await
                        .to_string(),
                    m.messages
                );
                reply += &appendage;
            }
            reply
        };

        context
            .send_message_silent(msg.chat.id, reply.await)
            .await
            .unwrap();
    } else {
        context
            .send_message_silent(msg.chat.id, "No such command".to_string())
            .await
            .unwrap();
    }
}
