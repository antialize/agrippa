use crate::runtime::{io_uring_get_sqe_submit, waker_task, Error, Result, TaskRef, TaskState};
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

pub(super) trait IOUringMethod: std::marker::Unpin {
    type Output;
    unsafe fn call(&mut self, sqe: &mut io_uring_sqe, task: TaskRef) -> Result<()>;
    fn result(&self, ret: i32) -> Result<Self::Output>;
}

#[derive(Copy, Clone)]
enum IOUringFutureState {
    Initial,
    Sent,
    Done,
}

pub(super) struct IOUringFeature<M: IOUringMethod> {
    state: IOUringFutureState,
    method: M,
}

impl<M: IOUringMethod> IOUringFeature<M> {
    fn new(method: M) -> Self {
        Self {
            state: IOUringFutureState::Initial,
            method,
        }
    }
}

impl<M: IOUringMethod> Drop for IOUringFeature<M> {
    fn drop(&mut self) {
        if let IOUringFutureState::Sent = self.state {
            panic!("io_uring future dropped while in progress");
        }
    }
}

impl<M: IOUringMethod> Future for IOUringFeature<M> {
    type Output = Result<M::Output>;
    fn poll(
        mut self: Pin<&mut Self>,
        context: &mut std::task::Context,
    ) -> std::task::Poll<Self::Output> {
        let task = waker_task(context.waker().clone());
        if let IOUringFutureState::Done = self.state {
            return Poll::Ready(Err(Error::Internal("Done future polled")));
        }
        let (ts, s, r) = match task.state.get() {
            TaskState::Inital => {
                match io_uring_get_sqe_submit(
                    &mut *task.as_ref().reactor.as_ref().ring.borrow_mut(),
                ) {
                    Err(e) => (
                        TaskState::Inital,
                        IOUringFutureState::Done,
                        Poll::Ready(Err(e)),
                    ),
                    Ok(mut sqe) => unsafe {
                        if let Err(e) = self.method.call(sqe.as_mut(), task.clone()) {
                            (
                                TaskState::Inital,
                                IOUringFutureState::Done,
                                Poll::Ready(Err(e)),
                            )
                        } else {
                            (
                                TaskState::UringWaiting,
                                IOUringFutureState::Sent,
                                Poll::Pending,
                            )
                        }
                    },
                }
            }
            TaskState::Cancled => (
                TaskState::Inital,
                IOUringFutureState::Done,
                Poll::Ready(Err(Error::Cancel)),
            ),
            TaskState::Timeouted => (
                TaskState::Inital,
                IOUringFutureState::Done,
                Poll::Ready(Err(Error::Timeout)),
            ),
            TaskState::UringWaiting => (
                TaskState::UringWaiting,
                IOUringFutureState::Sent,
                Poll::Pending,
            ),
            TaskState::UringDone(res) if res < 0 => (
                TaskState::Inital,
                IOUringFutureState::Done,
                Poll::Ready(Err(Error::from(std::io::Error::from_raw_os_error(-res)))),
            ),
            TaskState::UringDone(res) => (
                TaskState::Inital,
                IOUringFutureState::Done,
                Poll::Ready(self.method.result(res)),
            ),
            TaskState::UringCanceling => (
                TaskState::UringCanceling,
                IOUringFutureState::Sent,
                Poll::Pending,
            ),
            TaskState::UringTimeouting => (
                TaskState::UringTimeouting,
                IOUringFutureState::Sent,
                Poll::Pending,
            ),
        };
        task.state.set(ts);
        self.state = s;
        return r;
    }
}

pub(super) struct Accept<'a> {
    fd: &'a Fd,
    addr: libc::sockaddr_in,
    addr_len: u64,
}
impl<'a> IOUringMethod for Accept<'a> {
    type Output = (Fd, libc::sockaddr_in, u64);
    unsafe fn call(&mut self, sqe: &mut io_uring_sqe, task: TaskRef) -> Result<()> {
        prep_rw(
            IORING_OP_ACCEPT,
            sqe,
            self.fd.as_raw(),
            &mut self.addr as *mut libc::sockaddr_in as *mut core::ffi::c_void,
            0,
            &mut self.addr_len as *mut u64 as usize as u64,
            task,
        );
        Ok(())
    }
    fn result(&self, ret: i32) -> Result<Self::Output> {
        Ok((Fd { fd: ret }, self.addr, self.addr_len))
    }
}
impl<'a> Accept<'a> {
    pub(super) fn new(fd: &'a Fd) -> IOUringFeature<Self> {
        IOUringFeature::new(Self {
            fd,
            addr: unsafe { std::mem::zeroed() },
            addr_len: 0,
        })
    }
}

pub(super) struct Close {
    fd: Option<Fd>,
}
impl IOUringMethod for Close {
    type Output = ();
    unsafe fn call(&mut self, sqe: &mut io_uring_sqe, task: TaskRef) -> Result<()> {
        let mut fd = None;
        std::mem::swap(&mut fd, &mut self.fd);
        if let Some(fd) = fd {
            prep_rw(
                IORING_OP_CLOSE,
                sqe,
                fd.into_raw(),
                std::ptr::null_mut(),
                0,
                0,
                task,
            );
            Ok(())
        } else {
            Err(Error::Internal("internal error fd was none"))
        }
    }
    fn result(&self, _: i32) -> Result<Self::Output> {
        Ok(())
    }
}
impl Close {
    pub(super) fn new(fd: Fd) -> IOUringFeature<Self> {
        IOUringFeature::new(Self { fd: Some(fd) })
    }
}

pub(super) struct Write<'a> {
    fd: &'a Fd,
    data: &'a [u8],
    offset: u64,
}
impl<'a> IOUringMethod for Write<'a> {
    type Output = usize;
    unsafe fn call(&mut self, sqe: &mut io_uring_sqe, task: TaskRef) -> Result<()> {
        prep_rw(
            IORING_OP_WRITE,
            sqe,
            self.fd.as_raw(),
            self.data.as_ptr() as *const core::ffi::c_void as *mut core::ffi::c_void,
            self.data.len() as u32,
            self.offset,
            task,
        );
        Ok(())
    }
    fn result(&self, ret: i32) -> Result<Self::Output> {
        Ok(ret as usize)
    }
}
impl<'a> Write<'a> {
    pub(super) fn new(fd: &'a Fd, data: &'a [u8], offset: u64) -> IOUringFeature<Self> {
        IOUringFeature::new(Self { fd, data, offset })
    }
}

pub(super) struct Read<'a> {
    fd: &'a Fd,
    data: &'a mut [u8],
    offset: u64,
}
impl<'a> IOUringMethod for Read<'a> {
    type Output = usize;
    unsafe fn call(&mut self, sqe: &mut io_uring_sqe, task: TaskRef) -> Result<()> {
        prep_rw(
            IORING_OP_READ,
            sqe,
            self.fd.as_raw(),
            self.data.as_mut_ptr() as *mut core::ffi::c_void,
            self.data.len() as u32,
            self.offset,
            task,
        );
        Ok(())
    }
    fn result(&self, ret: i32) -> Result<Self::Output> {
        Ok(ret as usize)
    }
}
impl<'a> Read<'a> {
    pub(super) fn new(fd: &'a Fd, data: &'a mut [u8], offset: u64) -> IOUringFeature<Self> {
        IOUringFeature::new(Self { fd, data, offset })
    }
}

pub(super) struct Connect<'a> {
    fd: &'a Fd,
    addr: *const libc::c_void,
    addr_size: usize,
}

impl<'a> IOUringMethod for Connect<'a> {
    type Output = ();
    unsafe fn call(&mut self, sqe: &mut io_uring_sqe, task: TaskRef) -> Result<()> {
        prep_rw(
            IORING_OP_CONNECT,
            sqe,
            self.fd.as_raw(),
            self.addr as *mut libc::c_void,
            0,
            self.addr_size as u64,
            task,
        );
        Ok(())
    }
    fn result(&self, _: i32) -> Result<Self::Output> {
        Ok(())
    }
}

impl<'a> Connect<'a> {
    pub(super) fn new(
        fd: &'a Fd,
        addr: *const libc::c_void,
        addr_size: usize,
    ) -> IOUringFeature<Self> {
        IOUringFeature::new(Self {
            fd,
            addr,
            addr_size,
        })
    }
}

pub(super) struct OpenAt<'a> {
    path: &'a std::ffi::CStr,
    dirfd: Option<&'a Fd>,
    flags: u32,
    mode: u32,
}

impl<'a> IOUringMethod for OpenAt<'a> {
    type Output = Fd;
    unsafe fn call(&mut self, sqe: &mut io_uring_sqe, task: TaskRef) -> Result<()> {
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
        Ok(())
    }
    fn result(&self, ret: i32) -> Result<Self::Output> {
        Ok(Fd { fd: ret })
    }
}
impl<'a> OpenAt<'a> {
    pub(super) fn new(
        path: &'a std::ffi::CStr,
        dirfd: Option<&'a Fd>,
        flags: u32,
        mode: u32,
    ) -> IOUringFeature<Self> {
        IOUringFeature::new(Self {
            path,
            dirfd,
            flags,
            mode,
        })
    }
}
