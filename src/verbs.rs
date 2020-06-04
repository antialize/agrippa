use crate::runtime::{waker_task, Error, ReactorRef, Result, TaskRef, NOT_DONE};
use crate::verbs_util::QueuePair;
pub use crate::verbs_util::{Buffer, VerbsAddr};
use std::future::Future;
use std::pin::Pin;
use std::task::Poll;

pub struct Recv<'a> {
    qp: &'a QueuePair,
}

impl<'a> Future for Recv<'a> {
    type Output = Result<Buffer>;
    fn poll(mut self: Pin<&mut Self>, context: &mut std::task::Context) -> Poll<Self::Output> {
        if let Some(buffer) = self.qp.read() {
            Poll::Ready(Ok(buffer))
        } else if let Err(e) = self.qp.wait(waker_task(context.waker().clone())) {
            Poll::Ready(Err(Error::Io(e)))
        } else {
            Poll::Pending
        }
    }
}

#[derive(Copy, Clone)]
enum SendState {
    Initial,
    Sent,
    Done,
}

pub struct Send<'a> {
    qp: &'a QueuePair,
    buffer: Option<Buffer>,
    state: SendState,
}

impl<'a> Future for Send<'a> {
    type Output = Result<()>;
    fn poll(mut self: Pin<&mut Self>, context: &mut std::task::Context) -> Poll<Self::Output> {
        let task = waker_task(context.waker().clone());
        match self.state {
            SendState::Initial => {
                task.ring_result.set(NOT_DONE);
                match unsafe { self.qp.send(task, self.buffer.as_ref().unwrap()) } {
                    Err(e) => {
                        self.state = SendState::Done;
                        put_buffer(self.buffer.take().unwrap());
                        Poll::Ready(Err(Error::Io(e)))
                    }
                    Ok(e) => {
                        self.state = SendState::Sent;
                        Poll::Pending
                    }
                }
            }
            SendState::Sent => {
                let res = task.ring_result.get();
                if res == NOT_DONE {
                    Poll::Pending
                } else if res != 0 {
                    self.state = SendState::Done;
                    put_buffer(self.buffer.take().unwrap());
                    Poll::Ready(Err(Error::Internal("verbs error"))) //TODO (jakobt) this should be some kind of verbs error
                } else {
                    self.state = SendState::Done;
                    put_buffer(self.buffer.take().unwrap());
                    Poll::Ready(Ok(()))
                }
            }
            SendState::Done => Poll::Ready(Err(Error::Internal("Poll called on done future"))),
        }
    }
}

pub struct Connection {
    qp: QueuePair,
}

impl Connection {
    pub fn send(&self, buffer: Buffer) -> Send {
        Send {
            qp: &self.qp,
            buffer: Some(buffer),
            state: SendState::Initial,
        }
    }

    pub fn recv(&self) -> Recv {
        Recv { qp: &self.qp }
    }
}

pub struct ConnectionBuilder {
    reactor: ReactorRef,
    qp: QueuePair,
}

impl ConnectionBuilder {
    /**
     * Connect to the given remote address, for this to work
     * the remote host must also create a ConnectionBuilder
     * and pass our local_address, to his connect method
     */
    pub fn connect(self, remote_address: &VerbsAddr) -> Result<Connection> {
        let mut qp = self.qp;
        qp.connect(&mut self.reactor.device.borrow_mut(), remote_address)?;
        Ok(Connection { qp })
    }

    /**
     * Return our local address, this is the address that must be passed to connect
     * on the remote host
     */
    pub fn local_address(&self) -> VerbsAddr {
        self.qp.local_address(&self.reactor.device.borrow())
    }
}

pub struct Connect {}
impl Future for Connect {
    type Output = Result<ConnectionBuilder>;
    fn poll(mut self: Pin<&mut Self>, context: &mut std::task::Context) -> Poll<Self::Output> {
        let mut task = waker_task(context.waker().clone());
        let reactor = task.as_ref().reactor.clone();
        let ans = QueuePair::new(&mut reactor.device.borrow_mut());
        match ans {
            Ok(qp) => Poll::Ready(Ok(ConnectionBuilder { reactor, qp })),
            Err(e) => Poll::Ready(Err(Error::Io(e))),
        }
    }
}

/**
 * Establish a verbs connection to a remote host
 */
pub fn connect() -> Connect {
    Connect {}
}

pub struct GetBuffer {}
impl Future for GetBuffer {
    type Output = Result<Buffer>;
    fn poll(mut self: Pin<&mut Self>, context: &mut std::task::Context) -> Poll<Self::Output> {
        let mut task = waker_task(context.waker().clone());
        let t2 = task.clone();
        match task.as_ref().reactor.get_verbs_buffer() {
            Some(b) => Poll::Ready(Ok(b)),
            None => {
                task.as_ref().reactor.wait_verbs_buffer(t2);
                Poll::Pending
            }
        }
    }
}

pub fn get_buffer() -> GetBuffer {
    GetBuffer {}
}

pub struct PutBuffer {
    buffer: Option<Buffer>,
}
impl Future for PutBuffer {
    type Output = Result<()>;
    fn poll(mut self: Pin<&mut Self>, context: &mut std::task::Context) -> Poll<Self::Output> {
        let mut task = waker_task(context.waker().clone());
        task.as_ref()
            .reactor
            .put_verbs_buffer(self.buffer.take().unwrap());
        Poll::Ready(Ok(()))
    }
}

pub fn put_buffer(buffer: Buffer) -> PutBuffer {
    PutBuffer {
        buffer: Some(buffer),
    }
}
