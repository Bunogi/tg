use crate::db::AsyncSqlConnection;
use crate::include_sql;
use crate::redis::RedisConnection;
use crate::telegram::{message::Message, Telegram};
use crate::util::get_user;
use chrono::{offset::TimeZone, DateTime};

pub async fn handle_command<'a>(
    msg: &'a Message,
    msg_text: &'a str,
    context: Telegram,
    redis: RedisConnection,
    db: AsyncSqlConnection,
) {
    let split: Vec<String> = msg_text.split_whitespace().map(|s| s.into()).collect();
    match split[0].as_str() {
        "/show" => {
            //Apparently the preferred way to do blocking calls in async code is to put it in an async
            //block and then await it, so do that. SQLite doesn't have async primitives so good async
            //support would be hard anyway.
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

                let (total_msgs, since): (isize, isize) = conn
                    .query_row(
                        include_sql!("getmessagesdata.sql"),
                        params![msg.chat.id],
                        |row| Ok((row.get(0).unwrap(), row.get(1).unwrap())),
                    )
                    .unwrap();

                let since = chrono::Local.timestamp(since as i64, 0);

                let mut reply = format!("{} messages since {}\n", total_msgs, since);
                for m in messages {
                    debug!("message: {}", m.messages);
                    let appendage = format!(
                        "{}: {} messages\n",
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
                .unwrap()
        }
        _ => context
            .send_message_silent(msg.chat.id, "No such command".to_string())
            .await
            .unwrap(),
    };
}
