use crate::include_sql;
use crate::telegram::{user::User, Telegram};
use chrono::Duration;
use deadpool_postgres::Pool;
use md5::{Digest, Md5};

#[macro_export]
macro_rules! include_sql {
    ($s:tt) => {
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/sql/", $s))
    };
}

#[macro_export]
macro_rules! params {
    [$($x:expr),+ $(,)?] => {
        &[$(&$x),+]
    };
}
//Will get the user from cache if it is cached, otherwise request the data
pub async fn get_user(
    chat_id: i64,
    user_id: i64,
    telegram: &Telegram,
    config: &crate::Config,
    redis: &mut darkredis::Connection,
) -> User {
    let user_path = format!("tg.user.{}.{}", chat_id, user_id);
    match redis.get(&user_path).await.unwrap() {
        Some(u) => rmp_serde::from_slice(&u).unwrap(),
        None => {
            debug!("Getting user from telegram");
            let user = telegram.get_chat_member(chat_id, user_id).await.unwrap();
            let serialized = rmp_serde::to_vec(&user).unwrap();
            redis
                .set_and_expire_seconds(&user_path, &serialized, config.cache.username as u32)
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
pub async fn get_user_id(chat_id: i64, name: &str, pool: &Pool) -> Option<i64> {
    let conn = pool.get().await.unwrap();
    conn.query_opt(
        include_sql!("getuseridfromname.sql"),
        params![chat_id, name],
    )
    .await
    .unwrap()
    .map(|r| r.get(0))
}

pub fn seconds_to_hours(seconds: i32) -> f64 {
    f64::from(seconds) / (60.0 * 60.0)
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

pub async fn calculate_sticker_hash(
    telegram: &Telegram,
    mut redis: &mut darkredis::Connection,
    file_id: &str,
    set_name: &str,
    emoji: &str,
) -> Result<Vec<u8>, ()> {
    let command = darkredis::Command::new("HGET")
        .arg(b"tg.sticker.hashes")
        .arg(&file_id);

    match redis
        .run_command(command)
        .await
        .map_err(|e| error!("Couldn't get data from hash key: {}", e))?
        .optional_string()
    {
        Some(s) => Ok(s),
        None => {
            let mut hasher = Md5::new();
            //Logging this failure is not needed since download_file will log failed downloads anyway
            let sticker_file = telegram
                .download_file(&mut redis, file_id)
                .await
                .map_err(|_| ())?;
            hasher.input(&sticker_file);
            hasher.input(set_name);
            hasher.input(emoji);
            let result = hasher.result();
            let res = result.as_slice();
            let command = darkredis::Command::new("HSET")
                .arg(b"tg.sticker.hashes")
                .arg(&file_id)
                .arg(&res);

            redis
                .run_command(command)
                .await
                .map_err(|e| error!("Couldn't set sticker hash: {}", e))?;

            Ok(result.as_slice().to_vec())
        }
    }
}

//Align a each line after a symbol
pub fn align_text_after(symbol: char, text: String) -> String {
    let mut left_len = 0; //Length needed on left side
    let mut right_len = 0; //length needed on right

    let mut pairs = Vec::new();
    let lines = text.split('\n');
    for l in lines {
        if l.is_empty() {
            continue;
        }
        let found_index = l.find(symbol).unwrap() + 1; // Ensure symbol goes to lhs
        let (lhs, rhs) = l.split_at(found_index);
        if left_len < found_index {
            left_len = found_index;
        }
        if right_len < rhs.len() {
            right_len = rhs.len();
        }
        pairs.push((lhs, rhs));
    }

    let mut output = String::new();
    for (lhs, rhs) in pairs {
        output += &format!(
            "{:lwidth$}{:>rwidth$}\n",
            lhs,
            rhs,
            lwidth = left_len,
            rwidth = right_len
        );
    }
    output
}
