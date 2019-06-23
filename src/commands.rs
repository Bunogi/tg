use crate::db::AsyncSqlConnection;
use crate::include_sql;
use crate::redis::RedisConnection;
use crate::telegram::{chat::ChatType, message::Message, Telegram};
use crate::util::get_user;
use chrono::offset::TimeZone;

//Resolves commands written like /command@foobot which telegram does automatically. Cannot support '@' in command names.
fn get_command<'a>(input: &'a str, botname: &'a str) -> Option<&'a str> {
    if let Some(at) = input.find("@") {
        if &input[at..] == botname {
            Some(&input[..at])
        } else {
            None
        }
    } else {
        None
    }
}

pub async fn leaderboards<'a>(
    chatid: i64,
    context: Telegram,
    redis: RedisConnection,
    db: AsyncSqlConnection,
) {
    let conn = db.get().await;
    let messages = match conn
        .prepare_cached(include_sql!("getmessages.sql"))
        .unwrap()
        .query_map(params![chatid], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<(isize, isize)>, rusqlite::Error>>()
    {
        Ok(v) => v,
        Err(e) => {
            error!("Couldn't get messages in leaderboard command: {:?}", e);
            return;
        }
    };

    let (total_msgs, since): (isize, isize) = match conn.query_row(
        include_sql!("getmessagesdata.sql"),
        params![chatid],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ) {
        Ok((m, s)) => (m, s),
        Err(e) => {
            error!("Couldn't get message data in leaderboard command: {:?}", e);
            return;
        }
    };

    let edits = match conn
        .prepare_cached(include_sql!("geteditpercentage.sql"))
        .unwrap()
        .query_map(params![chatid], |row| {
            let user_id = row.get(0)?;
            let edit_percentage = row.get(1)?;
            let total_edits = row.get(2)?;
            Ok((user_id, edit_percentage, total_edits))
        })
        .unwrap()
        .collect::<Result<Vec<(isize, f64, isize)>, rusqlite::Error>>()
    {
        Ok(v) => v,
        Err(e) => {
            error!(
                "Couldn't get edit percentage in leaderboard command: {:?}",
                e
            );
            return;
        }
    };

    let since = chrono::Local.timestamp(since as i64, 0);

    //Message counts
    let mut reply = format!("{} messages since {}\n", total_msgs, since);
    let mut messages = messages.into_iter();
    if let Some((user, count)) = messages.next() {
        //store appendage here because otherwise this future doesn't implement Sync for
        //whatever reason
        let appendage = format!(
            "{} is the most annoying having sent {} messages!\n",
            get_user(chatid, user as i64, context.clone(), redis.clone()).await,
            count
        );

        reply += &appendage;
    }

    for m in messages {
        let appendage = format!(
            "{}: {} messages\n",
            get_user(chatid, m.0 as i64, context.clone(), redis.clone()).await,
            m.1
        );
        reply += &appendage;
    }

    // Edits
    let mut edits = edits.into_iter();
    if let Some((user, percentage, count)) = edits.next() {
        let appendage = format!(
            "{} is the biggest disaster, having edited {}% of their messages({} edits total)!\n",
            get_user(chatid, user as i64, context.clone(), redis.clone()).await,
            percentage,
            count
        );
        reply += &appendage;
    }
    for (user, percentage, count) in edits {
        let appendage = format!(
            "{}: {}% ({})\n",
            get_user(chatid, user as i64, context.clone(), redis.clone()).await,
            percentage,
            count
        );

        reply += &appendage;
    }
    context.send_message_silent(chatid, reply).await.unwrap();
}

pub async fn stickerlog(
    chatid: i64,
    context: Telegram,
    redis: RedisConnection,
    db: AsyncSqlConnection,
) {
    let conn = db.get().await;
    let (total_stickers, packs, since): (isize, isize, isize) = conn
        .query_row(
            include_sql!("getstickerstats.sql"),
            params![chatid],
            |row| {
                let total_stickers = row.get(0)?;
                let packs = row.get(1)?;
                let earliest = row.get(2)?;
                Ok((total_stickers, packs, earliest))
            },
        )
        .unwrap();

    context
        .send_message_silent(
            chatid,
            format!(
                "{} sent stickers from {} packs since {}",
                total_stickers,
                packs,
                chrono::Local.timestamp(since as i64, 0)
            ),
        )
        .await
        .unwrap();
}

pub async fn handle_command<'a>(
    msg: &'a Message,
    msg_text: &'a str,
    context: Telegram,
    redis: RedisConnection,
    db: AsyncSqlConnection,
) {
    let split: Vec<String> = msg_text.split_whitespace().map(|s| s.into()).collect();
    let root = if let ChatType::Private = msg.chat.kind {
        split[0].as_str()
    } else {
        let command = get_command(&split[0], context.bot_mention());
        if command.is_none() {
            return;
        }
        command.unwrap()
    };
    match root {
        "/leaderboards" => {
            leaderboards(msg.chat.id, context.clone(), redis.clone(), db.clone()).await
        }
        "/stickerlog" => stickerlog(msg.chat.id, context.clone(), redis.clone(), db.clone()).await,
        _ => {
            match msg.chat.kind {
                //Only nag at the user for wrong command if in a private chat
                ChatType::Private => {
                    context
                        .send_message_silent(msg.chat.id, "No such command".to_string())
                        .await
                        .unwrap();
                }
                _ => (),
            }
        }
    };
}
