use crate::runtime::Result;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct Delay {
    first: bool,
}

impl Future for Delay {
    type Output = Result<()>;
    fn poll(mut self: Pin<&mut Self>, context: &mut Context) -> Poll<Self::Output> {
        if self.first {
            self.first = false;
            context.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }
}

/*
 * Delay current tasks until higher priority tasks have been run
 */
pub fn delay() -> Delay {
    Delay { first: true }
}
