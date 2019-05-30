pub mod chat;
pub mod update;
pub mod user;

use reqwest::{r#async::Client, Url};
use update::UpdateStream;

pub struct Telegram {
    client: Client,
    base_url: String,
}

impl Telegram {
    pub fn new(token: String) -> Self {
        Self {
            client: Client::new(),
            base_url: format!("https://api.telegram.org/bot{}", token),
        }
    }

    pub fn updates<'a>(&'a self) -> UpdateStream<'a> {
        let url = Url::parse(&format!("{}/getUpdates", self.base_url)).unwrap();
        UpdateStream::new(&self.client, url)
    }
}
