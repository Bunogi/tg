use super::ApiResponse;
use super::ApiUpdate;
use super::Update;
use futures::{
    compat::Future01CompatExt,
    future::Future,
    prelude::*,
    task::{Context, Poll},
};
use futures01::{Future as Future01, Stream as Stream01};
use reqwest::{r#async::Client, Url};
use std::collections::VecDeque;
use std::pin::Pin;

pub struct UpdateStream {
    token: String,
    client: Client,
    poll_future: Pin<Box<Future<Output = Result<ApiResponse, ()>> + Send>>,
    poll_running: bool,
    cached_updates: VecDeque<ApiUpdate>,
    offset: u64,
}

impl UpdateStream {
    pub fn new(token: String) -> Self {
        let client = Client::new();
        let poll_future = Self::get_poll_future(&client, &token, 0);
        Self {
            offset: 0,
            token,
            client,
            poll_future,
            poll_running: false,
            cached_updates: VecDeque::new(),
        }
    }

    fn get_poll_future(
        client: &Client,
        token: &str,
        offset: u64,
    ) -> Pin<Box<Future<Output = Result<ApiResponse, ()>> + Send>> {
        let json = serde_json::json!({"offset": offset, "timeout": 6000});
        client
            .get(Url::parse(&format!("https://api.telegram.org/bot{}/getUpdates", token)).unwrap())
            .json(&json)
            .send()
            .and_then(|response| response.into_body().concat2())
            .map(|f| serde_json::from_slice(&f).unwrap())
            .map_err(|_| ())
            .compat()
            .boxed()
    }
}

impl Stream for UpdateStream {
    type Item = Update;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        if !self.cached_updates.is_empty() {
            let popped = self.cached_updates.pop_front().unwrap();
            return Poll::Ready(Some(popped.into_update()));
        }

        if !self.poll_running {
            self.poll_future = Self::get_poll_future(&self.client, &self.token, self.offset + 1);
            self.poll_running = true;
        }

        match Future::poll(self.poll_future.as_mut(), cx) {
            Poll::Ready(Ok(t)) => {
                self.cached_updates = t.updates;
                self.poll_running = false;
                if let Some(max) = self
                    .cached_updates
                    .iter()
                    .max_by(|x, y| x.update_id.cmp(&y.update_id))
                {
                    self.offset = max.update_id;
                }
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Poll::Ready(Err(e)) => {
                warn!("Got an error: {:?}. Shuttng down update stream", e);
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
