use super::{super::ApiUpdate, Update};
use futures::{
    compat::Future01CompatExt,
    future::Future,
    prelude::*,
    task::{Context, Poll},
};
use futures01::{Future as Future01, Stream as Stream01};
use reqwest::{r#async::Client, Url};
use serde::Deserialize;
use std::collections::VecDeque;
use std::pin::Pin;

#[derive(Debug, Deserialize)]
struct ApiResponse {
    ok: bool,
    #[serde(rename = "result")]
    updates: VecDeque<ApiUpdate>,
}

pub struct UpdateStream<'a> {
    update_url: Url,
    client: &'a Client,
    poll_future: Pin<Box<dyn Future<Output = Result<ApiResponse, ()>> + Send>>,
    poll_running: bool,
    cached_updates: VecDeque<ApiUpdate>,
    offset: u64,
}

impl<'a> UpdateStream<'a> {
    pub fn new(client: &'a Client, update_url: Url) -> Self {
        let poll_future = Self::get_poll_future(&client, &update_url, 0);
        Self {
            offset: 0,
            update_url,
            client,
            poll_future,
            poll_running: false,
            cached_updates: VecDeque::new(),
        }
    }

    //TODO return proper error type
    fn get_poll_future(
        client: &Client,
        url: &Url,
        offset: u64,
    ) -> Pin<Box<dyn Future<Output = Result<ApiResponse, ()>> + Send>> {
        let json = serde_json::json!({"offset": offset, "timeout": 6000});
        client
            .get(url.clone())
            .json(&json)
            .send()
            .and_then(|response| response.into_body().concat2())
            .map(|f| serde_json::from_slice(&f).unwrap())
            .map_err(|_| ())
            .compat()
            .boxed()
    }
}

impl Stream for UpdateStream<'_> {
    type Item = Update;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        if !self.cached_updates.is_empty() {
            let popped = self.cached_updates.pop_front().unwrap();
            return Poll::Ready(Some(popped.into()));
        }

        if !self.poll_running {
            self.poll_future =
                Self::get_poll_future(&self.client, &self.update_url, self.offset + 1);
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
