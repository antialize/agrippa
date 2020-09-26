use crate::runtime::{io_uring_get_sqe_submit, waker_task, Error, Result, TaskRef, NOT_DONE};
use crate::sys::{
    io_uring_sqe, IORING_OP_ACCEPT, IORING_OP_CLOSE, IORING_OP_CONNECT, IORING_OP_OPENAT,
    IORING_OP_READ, IORING_OP_WRITE,
};
use libc;
use log::debug;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::Poll;

unsafe fn prep_rw(
    op: u32,
    sqe: &mut io_uring_sqe,
    fd: i32,
    addr: *mut std::ffi::c_void,
    len: u32,
    offset: u64,
    task: TaskRef,
) {
    sqe.opcode = op as u8;
    sqe.flags = 0;
    sqe.ioprio = 0;
    sqe.fd = fd;
    sqe.__bindgen_anon_1.off = offset;
    sqe.__bindgen_anon_2.addr = addr as u64;
    sqe.len = len;
    sqe.__bindgen_anon_3.rw_flags = 0;
    sqe.user_data = Rc::into_raw(task) as usize as u64;
    sqe.__bindgen_anon_4.__pad2[0] = 0;
    sqe.__bindgen_anon_4.__pad2[1] = 0;
    sqe.__bindgen_anon_4.__pad2[2] = 0;
}
pub(super) struct Fd {
    pub(super) fd: i32,
}

impl Drop for Fd {
    fn drop(&mut self) {
        //TODO check for EAGAIN
        debug!("File closed synchronosly");
        unsafe {
            libc::close(self.fd);
        }
    }
}

impl Fd {
    unsafe fn as_raw(&self) -> i32 {
        self.fd
    }
    pub fn into_raw(self) -> i32 {
        let r = self.fd;
        std::mem::forget(self);
        r
    }
}

#[derive(Copy, Clone)]
enum IOUringFutureState {
    Initial,
    Sent,
    Done,
}

fn io_uring_poll_impl<F: FnMut(&mut io_uring_sqe, TaskRef) -> Result<()>>(
    state: IOUringFutureState,
    context: &mut std::task::Context,
    f: &mut F,
) -> (IOUringFutureState, Result<Option<i32>>) {
    let task = waker_task(context.waker().clone());
    match state {
        IOUringFutureState::Initial => {
            match io_uring_get_sqe_submit(&mut *task.as_ref().reactor.as_ref().ring.borrow_mut()) {
                Err(e) => (IOUringFutureState::Done, Err(e)),
                Ok(mut sqe) => {
                    if let Err(e) = f(unsafe { sqe.as_mut() }, task.clone()) {
                        (IOUringFutureState::Done, Err(e))
                    } else {
                        (IOUringFutureState::Sent, Ok(None))
                    }
                }
            }
        }
        IOUringFutureState::Sent => {
            let res = task.ring_result.get();
            if res == NOT_DONE {
                (IOUringFutureState::Sent, Ok(None))
            } else if res < 0 {
                (
                    IOUringFutureState::Done,
                    Err(Error::from(std::io::Error::from_raw_os_error(-res))),
                )
            } else {
                (IOUringFutureState::Done, Ok(Some(res)))
            }
        }
        IOUringFutureState::Done => (
            IOUringFutureState::Done,
            Err(Error::Internal("Done future polled")),
        ),
    }
}

pub(super) struct Accept<'a> {
    fd: &'a Fd,
    addr: libc::sockaddr_in,
    addr_len: u64,
    state: IOUringFutureState,
}

impl<'a> Future for Accept<'a> {
    type Output = Result<(Fd, libc::sockaddr_in, u64)>;
    fn poll(
        mut self: Pin<&mut Self>,
        context: &mut std::task::Context,
    ) -> std::task::Poll<Self::Output> {
        let (state, res) = io_uring_poll_impl(self.state, context, &mut |sqe, task| {
            println!("I WAS HERE");
            unsafe {
                prep_rw(
                    IORING_OP_ACCEPT,
                    sqe,
                    self.fd.as_raw(),
                    &mut self.addr as *mut libc::sockaddr_in as *mut core::ffi::c_void,
                    0,
                    &mut self.addr_len as *mut u64 as usize as u64,
                    task,
                )
            };
            Ok(())
        });
        self.state = state;
        match res {
            Err(e) => Poll::Ready(Err(e)),
            Ok(None) => Poll::Pending,
            Ok(Some(ret)) => Poll::Ready(Ok((Fd { fd: ret }, self.addr, self.addr_len))),
        }
    }
}

impl<'a> Accept<'a> {
    pub(super) fn new(fd: &'a Fd) -> Self {
        Self {
            fd,
            addr: unsafe { std::mem::zeroed() },
            addr_len: 0,
            state: IOUringFutureState::Initial,
        }
    }
}

impl<'a> Drop for Accept<'a> {
    fn drop(&mut self) {
        if let IOUringFutureState::Sent = self.state {
            panic!("io_uring future dropped while in progress");
        }
    }
}

pub(super) struct Close {
    fd: Option<Fd>,
    state: IOUringFutureState,
}

impl Future for Close {
    type Output = Result<()>;
    fn poll(
        mut self: Pin<&mut Self>,
        context: &mut std::task::Context,
    ) -> std::task::Poll<Self::Output> {
        let (state, res) = io_uring_poll_impl(self.state, context, &mut |sqe, task| {
            let mut fd = None;
            std::mem::swap(&mut fd, &mut self.fd);
            if let Some(fd) = fd {
                unsafe {
                    prep_rw(
                        IORING_OP_CLOSE,
                        sqe,
                        fd.into_raw(),
                        std::ptr::null_mut(),
                        0,
                        0,
                        task,
                    )
                };
                Ok(())
            } else {
                Err(Error::Internal("internal error fd was none"))
            }
        });
        self.state = state;
        match res {
            Err(e) => Poll::Ready(Err(e)),
            Ok(None) => Poll::Pending,
            Ok(Some(_)) => Poll::Ready(Ok(())),
        }
    }
}

impl Close {
    pub(super) fn new(fd: Fd) -> Self {
        Self {
            fd: Some(fd),
            state: IOUringFutureState::Initial,
        }
    }
}

impl Drop for Close {
    fn drop(&mut self) {
        if let IOUringFutureState::Sent = self.state {
            panic!("io_uring future dropped while in progress");
        }
    }
}

pub(super) struct Write<'a> {
    fd: &'a Fd,
    data: &'a [u8],
    offset: u64,
    state: IOUringFutureState,
}

impl<'a> Future for Write<'a> {
    type Output = Result<usize>;
    fn poll(
        mut self: Pin<&mut Self>,
        context: &mut std::task::Context,
    ) -> std::task::Poll<Self::Output> {
        let (state, res) = io_uring_poll_impl(self.state, context, &mut |sqe, task| {
            unsafe {
                prep_rw(
                    IORING_OP_WRITE,
                    sqe,
                    self.fd.as_raw(),
                    self.data.as_ptr() as *const core::ffi::c_void as *mut core::ffi::c_void,
                    self.data.len() as u32,
                    self.offset,
                    task,
                )
            };
            Ok(())
        });
        self.state = state;
        match res {
            Err(e) => Poll::Ready(Err(e)),
            Ok(None) => Poll::Pending,
            Ok(Some(len)) => Poll::Ready(Ok(len as usize)),
        }
    }
}

impl<'a> Write<'a> {
    pub(super) fn new(fd: &'a Fd, data: &'a [u8], offset: u64) -> Self {
        Self {
            fd,
            data,
            offset,
            state: IOUringFutureState::Initial,
        }
    }
}

impl<'a> Drop for Write<'a> {
    fn drop(&mut self) {
        if let IOUringFutureState::Sent = self.state {
            panic!("io_uring future dropped while in progress");
        }
    }
}

pub(super) struct Read<'a> {
    fd: &'a Fd,
    data: &'a mut [u8],
    offset: u64,
    state: IOUringFutureState,
}

impl<'a> Future for Read<'a> {
    type Output = Result<usize>;
    fn poll(
        mut self: Pin<&mut Self>,
        context: &mut std::task::Context,
    ) -> std::task::Poll<Self::Output> {
        let (state, res) = io_uring_poll_impl(self.state, context, &mut |sqe, task| {
            unsafe {
                prep_rw(
                    IORING_OP_READ,
                    sqe,
                    self.fd.as_raw(),
                    self.data.as_mut_ptr() as *mut core::ffi::c_void,
                    self.data.len() as u32,
                    self.offset,
                    task,
                )
            };
            Ok(())
        });
        self.state = state;
        match res {
            Err(e) => Poll::Ready(Err(e)),
            Ok(None) => Poll::Pending,
            Ok(Some(len)) => Poll::Ready(Ok(len as usize)),
        }
    }
}

impl<'a> Read<'a> {
    pub(super) fn new(fd: &'a Fd, data: &'a mut [u8], offset: u64) -> Self {
        Self {
            fd,
            data,
            offset,
            state: IOUringFutureState::Initial,
        }
    }
}

impl<'a> Drop for Read<'a> {
    fn drop(&mut self) {
        if let IOUringFutureState::Sent = self.state {
            panic!("io_uring future dropped while in progress");
        }
    }
}

pub(super) struct Connect<'a> {
    fd: &'a Fd,
    addr: *const libc::c_void,
    addr_size: usize,
    state: IOUringFutureState,
}

impl<'a> Future for Connect<'a> {
    type Output = Result<()>;
    fn poll(mut self: Pin<&mut Self>, context: &mut std::task::Context) -> Poll<Self::Output> {
        let (state, res) = io_uring_poll_impl(self.state, context, &mut |sqe, task| {
            unsafe {
                prep_rw(
                    IORING_OP_CONNECT,
                    sqe,
                    self.fd.as_raw(),
                    self.addr as *mut libc::c_void,
                    0,
                    self.addr_size as u64,
                    task,
                )
            };
            Ok(())
        });
        self.state = state;
        match res {
            Err(e) => Poll::Ready(Err(e)),
            Ok(None) => Poll::Pending,
            Ok(Some(_)) => Poll::Ready(Ok(())),
        }
    }
}

impl<'a> Connect<'a> {
    pub(super) fn new(fd: &'a Fd, addr: *const libc::c_void, addr_size: usize) -> Self {
        Self {
            fd,
            addr,
            addr_size,
            state: IOUringFutureState::Initial,
        }
    }
}

impl<'a> Drop for Connect<'a> {
    fn drop(&mut self) {
        if let IOUringFutureState::Sent = self.state {
            panic!("io_uring future dropped while in progress");
        }
    }
}

pub(super) struct OpenAt<'a> {
    path: &'a std::ffi::CStr,
    dirfd: Option<&'a Fd>,
    flags: u32,
    mode: u32,
    state: IOUringFutureState,
}

impl<'a> OpenAt<'a> {
    pub(super) fn new(
        path: &'a std::ffi::CStr,
        dirfd: Option<&'a Fd>,
        flags: u32,
        mode: u32,
    ) -> Self {
        Self {
            path,
            dirfd,
            flags,
            mode,
            state: IOUringFutureState::Initial,
        }
    }
}

impl<'a> Future for OpenAt<'a> {
    type Output = Result<Fd>;
    fn poll(mut self: Pin<&mut Self>, context: &mut std::task::Context) -> Poll<Self::Output> {
        let (state, res) = io_uring_poll_impl(self.state, context, &mut |sqe, task| {
            unsafe {
                prep_rw(
                    IORING_OP_OPENAT,
                    sqe,
                    self.dirfd.map(|v| v.as_raw()).unwrap_or(libc::AT_FDCWD),
                    self.path.as_ptr() as *mut libc::c_void,
                    self.mode,
                    0,
                    task,
                );
                sqe.__bindgen_anon_3.open_flags = self.flags;
            };
            Ok(())
        });
        self.state = state;
        match res {
            Err(e) => Poll::Ready(Err(e)),
            Ok(None) => Poll::Pending,
            Ok(Some(ret)) => Poll::Ready(Ok(Fd { fd: ret })),
        }
    }
}

impl<'a> Drop for OpenAt<'a> {
    fn drop(&mut self) {
        if let IOUringFutureState::Sent = self.state {
            panic!("io_uring future dropped while in progress");
        }
    }
}
