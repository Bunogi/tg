use crate::db::SqlPool;
use crate::include_sql;
use crate::redis::RedisConnection;
use crate::telegram::{user::User, Telegram};
use chrono::Duration;
use rusqlite::OptionalExtension;

//Will get the user from cache if it is cached, otherwise request the data
pub async fn get_user(
    chat_id: i64,
    user_id: i64,
    context: Telegram,
    mut redis: RedisConnection,
) -> User {
    let user_path = format!("tg.user.{}.{}", chat_id, user_id);
    match redis.get_bytes(&user_path).await.unwrap() {
        Some(u) => rmp_serde::from_slice(&u).unwrap(),
        None => {
            debug!("Getting user from tg");
            let user = context.get_chat_member(chat_id, user_id).await.unwrap();
            let serialized = rmp_serde::to_vec(&user).unwrap();
            redis
                .set_with_expiry(
                    &user_path,
                    &serialized,
                    std::time::Duration::from_millis(1000 * 3600),
                )
                .await
                .unwrap();
            user
        }
    }
}

pub fn parse_time(input: &[String]) -> Option<Duration> {
    //n year(s)/month(s)/week(s)/day(s)
    if input.len() != 2 {
        None
    } else {
        if input[0].contains('-') {
            return None;
        }

        let num = input[0].parse::<i64>().ok()?;
        let name = if input[1].ends_with('s') {
            &input[1][..input[1].len() - 1]
        } else {
            input[1].as_str()
        };
        match name.to_lowercase().as_str() {
            "year" => Some(Duration::days(num * 365)),
            "month" => Some(Duration::weeks(num * 4)),
            "week" => Some(Duration::weeks(num)),
            "day" => Some(Duration::days(num)),
            "hour" => Some(Duration::hours(num)),
            "minute" => Some(Duration::minutes(num)),
            "second" => Some(Duration::seconds(num)),
            _ => None,
        }
    }
}

//Returns the last known user id matching name in chat_id
//If multiple users match, it will pick one at complete random due to how SQLite works
pub async fn get_user_id(chat_id: i64, name: &str, pool: SqlPool) -> Option<i64> {
    let conn = pool.get().await;
    conn.query_row(
        include_sql!("getuseridfromname.sql"),
        params![chat_id as isize, name],
        |row| Ok(row.get(0)?),
    )
    .optional()
    .unwrap()
}

pub unsafe fn rgba_to_cairo(mut ptr: *mut u8, len: usize) {
    assert_eq!(len % 4, 0);
    for _ in 0..len / 4 {
        //Convert to bgra
        let r = *ptr.offset(0);
        let b = *ptr.offset(2);
        *ptr.offset(0) = b;
        *ptr.offset(2) = r;

        //Precalculate alpha
        let a = f64::from(*ptr.offset(3)) / 255.0;
        *ptr.offset(0) = (f64::from(*ptr.offset(0)) * a).round() as u8;
        *ptr.offset(1) = (f64::from(*ptr.offset(1)) * a).round() as u8;
        *ptr.offset(2) = (f64::from(*ptr.offset(2)) * a).round() as u8;

        ptr = ptr.offset(4);
    }
}
