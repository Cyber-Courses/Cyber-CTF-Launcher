//! A panic in a future, caught where it is polled: tokio ends a task that panics without a word
//! to whoever waits on what it would have done, so what must report every end (a deploy worker's
//! job, a test scenario) catches it and says it instead.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::task::{Context, Poll};

/// A future whose panic, while it is polled, is its output instead.
pub struct CatchUnwind<F>(pub Pin<Box<F>>);

impl<F: Future> Future for CatchUnwind<F> {
    type Output = std::thread::Result<F::Output>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let inner = self.0.as_mut();
        match catch_unwind(AssertUnwindSafe(|| inner.poll(cx))) {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(done)) => Poll::Ready(Ok(done)),
            Err(panic) => Poll::Ready(Err(panic)),
        }
    }
}

/// What a panic said.
pub fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic.downcast_ref::<&str>().map(ToString::to_string).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_else(|| "no message".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_panic_while_polled_is_the_output_with_its_message() {
        assert_eq!(CatchUnwind(Box::pin(async { 7 })).await.ok(), Some(7));
        let panicked = CatchUnwind(Box::pin(async {
            tokio::task::yield_now().await;
            panic!("step {} blew up", 2)
        }))
        .await
        .unwrap_err();
        assert_eq!(panic_message(&*panicked), "step 2 blew up");
        let literal = CatchUnwind(Box::pin(async { panic!("plain") })).await.unwrap_err();
        assert_eq!(panic_message(&*literal), "plain");
        assert_eq!(panic_message(&42_u8), "no message");
    }
}
