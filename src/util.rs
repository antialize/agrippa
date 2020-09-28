use crate::runtime::{waker_task, Error, Priority, Result, Task};
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

struct SpawnTaskFuture<F: Future<Output = Result<()>> + 'static> {
    future: Option<F>,
    priority: Priority,
}

impl<F: Future<Output = Result<()>> + 'static> Future for SpawnTaskFuture<F> {
    type Output = Result<Task>;
    fn poll(mut self: Pin<&mut Self>, context: &mut Context) -> Poll<Self::Output> {
        let p = self.priority;
        if let Some(f) = unsafe { self.get_unchecked_mut() }.future.take() {
            let task = waker_task(context.waker().clone());
            Poll::Ready(Ok(task.reactor.spawn(p, f)))
        } else {
            Poll::Ready(Err(Error::Internal("Poll called on done future")))
        }
    }
}

/// Spawn a new task with the given priority. The task will be spawned
/// in reactor of the task that awaits the resulting future.
pub async fn spawn_task<F: Future<Output = Result<()>> + 'static>(
    priority: Priority,
    future: F,
) -> Result<Task> {
    SpawnTaskFuture {
        future: Some(future),
        priority,
    }
    .await
}
