use crate::redis::RedisConnection;
use crate::telegram::{user::User, Telegram};
use std::time::{SystemTime, UNIX_EPOCH};

//Will get the user from cache if it is cached, otherwise request the data
pub async fn get_user(
    chat_id: i64,
    user_id: i64,
    context: Telegram,
    mut redis: RedisConnection,
) -> User {
    let user_path = format!("chat.\"{}\".\"{}\"", chat_id, user_id);
    match redis.get(&user_path).await.unwrap() {
        Some(u) => u,
        None => {
            debug!("Getting user from tg");
            let user = context.get_chat_member(chat_id, user_id).await.unwrap();
            let serialized = serde_json::to_string(&user).unwrap();
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

pub fn get_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
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
