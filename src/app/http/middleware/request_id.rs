//! An example of your own middleware — `app/Http/Middleware`.

use std::sync::atomic::{AtomicU64, Ordering};

use rainier_framework::http::{Request, Response};
use rainier_framework::middleware::{Middleware, Next};
use rainier_framework::prelude::*;

/// The id assigned to this request, readable by any handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestId(pub String);

/// Gives every request an id and echoes it back in `X-Request-Id`.
///
/// Honours an incoming `X-Request-Id`, so a request that crossed a proxy or
/// another service keeps one id end to end — which is the whole point of
/// having one.
#[derive(Debug, Default)]
pub struct RequestIdMiddleware {
    counter: AtomicU64,
}

impl RequestIdMiddleware {
    /// A fresh generator.
    pub fn new() -> Self {
        Self::default()
    }

    fn next_id(&self) -> String {
        // Not a UUID — that would be a dependency for a value whose only
        // requirement is being distinct within this process's logs.
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        format!("{:x}-{n:x}", std::process::id())
    }
}

#[async_trait]
impl Middleware for RequestIdMiddleware {
    async fn handle(&self, mut request: Request, next: Next) -> Response {
        let id = request
            .header("x-request-id")
            .filter(|incoming| is_safe(incoming))
            .map(str::to_string)
            .unwrap_or_else(|| self.next_id());

        request.extensions_mut().insert(RequestId(id.clone()));
        next.run(request).await.with_header("x-request-id", &id)
    }

    fn name(&self) -> &'static str {
        "RequestId"
    }
}

/// An id echoed into a response header must not carry anything that could
/// break out of it, and an unbounded one is a cheap way to bloat every log
/// line.
fn is_safe(candidate: &str) -> bool {
    !candidate.is_empty()
        && candidate.len() <= 64
        && candidate.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::middleware::{Middleware, Pipeline};
    use std::sync::Arc;

    /// One shared generator, as the kernel registers it — a fresh instance per
    /// request would restart the counter and hand out the same id twice.
    async fn run(generator: Arc<RequestIdMiddleware>, request: Request) -> Response {
        Pipeline::new()
            .through_arc(generator as Arc<dyn Middleware>)
            .then(|request: Request| async move {
                let id = request.extension::<RequestId>().map(|id| id.0.clone());
                Response::text(id.unwrap_or_default())
            })
            .run(request)
            .await
    }

    fn generator() -> Arc<RequestIdMiddleware> {
        Arc::new(RequestIdMiddleware::new())
    }

    async fn body_of(response: Response) -> String {
        String::from_utf8(response.into_http().into_body().collect().await.unwrap().to_vec())
            .unwrap()
    }

    #[tokio::test]
    async fn an_id_is_generated_and_echoed() {
        let response = run(generator(), Request::builder().build()).await;
        let header = response.header("x-request-id").unwrap().to_string();

        assert!(!header.is_empty());
        assert_eq!(body_of(response).await, header, "the handler sees the same id");
    }

    #[tokio::test]
    async fn an_incoming_id_is_kept() {
        let request = Request::builder().header("x-request-id", "abc-123").build();
        assert_eq!(run(generator(), request).await.header("x-request-id"), Some("abc-123"));
    }

    #[tokio::test]
    async fn a_hostile_incoming_id_is_replaced() {
        let generator = generator();
        for hostile in ["a b", "x\r\nSet-Cookie: y", &"x".repeat(200), ""] {
            let request = Request::builder().header("x-request-id", hostile).build();
            let response = run(Arc::clone(&generator), request).await;
            assert_ne!(response.header("x-request-id"), Some(hostile));
        }
    }

    #[tokio::test]
    async fn ids_are_distinct_across_requests() {
        // One generator serving both, as the kernel does.
        let generator = generator();
        let a = run(Arc::clone(&generator), Request::builder().build()).await;
        let b = run(Arc::clone(&generator), Request::builder().build()).await;

        assert_ne!(a.header("x-request-id"), b.header("x-request-id"));
    }
}
