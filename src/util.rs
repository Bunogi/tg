use crate::redis::RedisConnection;
use crate::telegram::{user::User, Telegram};

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
            let user = context.get_chat_member(chat_id, user_id).await.unwrap();
            redis
                .set_with_expiry(
                    &user_path,
                    &user,
                    std::time::Duration::from_millis(1000 * 3600),
                )
                .await
                .unwrap();
            user
        }
    }
}
