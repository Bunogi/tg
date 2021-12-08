use super::{super::ApiUpdate, Update};
use futures::{
    future::Future,
    prelude::*,
    task::{Context, Poll},
};
use reqwest::{Client, Url};
use serde::Deserialize;
use std::pin::Pin;
use std::{collections::VecDeque, convert::TryInto};

#[derive(Debug, Deserialize)]
struct ApiResponse {
    ok: bool,
    #[serde(rename = "result")]
    updates: VecDeque<ApiUpdate>,
}

pub struct UpdateStream<'a> {
    update_url: Url,
    client: &'a Client,
    poll_future: Pin<Box<dyn Future<Output = Result<ApiResponse, ()>> + Send + 'a>>,
    poll_running: bool,
    cached_updates: VecDeque<ApiUpdate>,
    offset: u64,
}

impl<'a> UpdateStream<'a> {
    pub fn new(client: &'a Client, update_url: Url) -> Self {
        let poll_future = Self::get_poll_future(client, update_url.clone(), 0).boxed();

        Self {
            offset: 1,
            update_url,
            client,
            poll_future,
            poll_running: true,
            cached_updates: VecDeque::new(),
        }
    }

    //TODO return proper error type
    async fn get_poll_future(client: &'a Client, url: Url, offset: u64) -> Result<ApiResponse, ()> {
        let json = serde_json::json!({"offset": offset, "timeout": 6000});
        let response = client.get(url).json(&json).send().await.unwrap();

        response.json::<ApiResponse>().map_err(|_| ()).await
    }
}

impl Stream for UpdateStream<'_> {
    type Item = Update;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        while !self.cached_updates.is_empty() {
            let popped = self.cached_updates.pop_front().unwrap().try_into();

            if let Ok(inner) = popped {
                return Poll::Ready(Some(inner));
            } else {
                warn!("Ignored APIUpdate {:?}", popped);
            }
        }

        if !self.poll_running {
            self.poll_future =
                Self::get_poll_future(self.client, self.update_url.clone(), self.offset + 1)
                    .boxed();
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
                warn!(
                    "Updatestream got an error: {:?}. Shutting down update stream",
                    e
                );
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
