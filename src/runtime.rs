use crate::sys::{
    __io_uring_get_cqe, io_uring, io_uring_cqe, io_uring_get_sqe, io_uring_queue_exit,
    io_uring_queue_init, io_uring_sqe, io_uring_submit, IORING_OP_ASYNC_CANCEL,
};

use log::info;
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::Poll;

fn io_uring_cancel(task: &TaskRef) -> Result<()> {
    unsafe {
        let mut sqep =
            io_uring_get_sqe_submit(&mut *task.as_ref().reactor.as_ref().ring.borrow_mut())?;
        let mut sqe = sqep.as_mut();

        sqe.opcode = IORING_OP_ASYNC_CANCEL as u8;
        sqe.flags = 0;
        sqe.ioprio = 0;
        sqe.fd = -1;
        sqe.__bindgen_anon_1.off = 0;
        sqe.__bindgen_anon_2.addr = Rc::as_ptr(task) as usize as u64;
        sqe.len = 0;
        sqe.__bindgen_anon_3.rw_flags = 0;
        sqe.user_data = 0;
        sqe.__bindgen_anon_4.__pad2[0] = 0;
        sqe.__bindgen_anon_4.__pad2[1] = 0;
        sqe.__bindgen_anon_4.__pad2[2] = 0;
    }
    Ok(())
}

#[cfg(feature = "verbs")]
use crate::verbs_util;

#[derive(Copy, Clone)]
pub enum Priority {
    High = 0,
    Normal = 1,
    Low = 2,
}

struct TaskQueue {
    qs: [std::collections::VecDeque<TaskRef>; 3],
}

impl TaskQueue {
    fn new() -> Self {
        Self {
            qs: [
                std::collections::VecDeque::new(),
                std::collections::VecDeque::new(),
                std::collections::VecDeque::new(),
            ],
        }
    }

    fn push(&mut self, task: TaskRef) {
        self.qs[task.priority as usize].push_back(task)
    }

    fn pop(&mut self) -> Option<TaskRef> {
        for q in &mut self.qs {
            if let Some(v) = q.pop_front() {
                return Some(v);
            }
        }
        return None;
    }
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Cancel,
    Timeout,
    Eof,
    Internal(&'static str),
    NulError(std::ffi::NulError),
    Boxed(Box<dyn std::error::Error>),
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Error::Io(error)
    }
}

impl From<std::ffi::NulError> for Error {
    fn from(error: std::ffi::NulError) -> Self {
        Error::NulError(error)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "IO: {}", e),
            Error::Cancel => write!(f, "Cancel"),
            Error::Timeout => write!(f, "Timeout"),
            Error::Eof => write!(f, "Eof"),
            Error::Internal(s) => write!(f, "Internal error: {}", s),
            Error::NulError(e) => write!(f, "NulError: {}", e),
            Error::Boxed(e) => write!(f, "{}", e.as_ref()),
        }
    }
}

impl std::error::Error for Error {
    fn description(&self) -> &str {
        "invalid utf-8: corrupt contents"
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug)]
pub(super) enum TaskState {
    Inital,
    Cancled,
    Timeouted,
    UringWaiting,
    UringCanceling,
    UringTimeouting,
    UringDone(i32),
}

pub(super) struct TaskContent {
    future: RefCell<Pin<Box<dyn Future<Output = Result<()>> + 'static>>>,
    priority: Priority,
    pub(super) reactor: ReactorRef,
    pub(super) state: Cell<TaskState>,
}

pub(super) type TaskRef = Rc<TaskContent>;

impl TaskContent {
    fn wake(self: TaskRef) {
        let s = self.clone();
        let r = self.as_ref().reactor.as_ref();
        r.ready.borrow_mut().push(s);
    }

    fn new<F: Future<Output = Result<()>> + 'static>(
        future: F,
        priority: Priority,
        reactor: ReactorRef,
    ) -> Self {
        TaskContent {
            future: RefCell::new(Box::pin(future)),
            priority,
            reactor,
            state: Cell::new(TaskState::Inital),
        }
    }
}

#[derive(Clone)]
pub struct Task {
    content: TaskRef,
}

impl Task {
    pub fn cancel(&self) -> Result<()> {
        let s = match self.content.state.get() {
            TaskState::Inital => TaskState::Cancled,
            TaskState::UringWaiting => {
                io_uring_cancel(&self.content)?;
                TaskState::UringCanceling
            }
            TaskState::UringDone(_) => TaskState::Cancled,
            v => v,
        };
        self.content.state.set(s);
        Ok(())
    }

    pub fn timeout(&self) -> Result<()> {
        let s = match self.content.state.get() {
            TaskState::Inital => TaskState::Timeouted,
            TaskState::UringWaiting => {
                io_uring_cancel(&self.content)?;
                TaskState::UringTimeouting
            }
            TaskState::UringDone(_) => TaskState::Timeouted,
            v => v,
        };
        self.content.state.set(s);
        Ok(())
    }

    pub async fn wait(&self) {}
}

unsafe fn waker_clone(data: *const ()) -> std::task::RawWaker {
    let task = TaskRef::from_raw(data as *const TaskContent);
    let t2 = task.clone();
    Rc::into_raw(t2); //incref
    std::task::RawWaker::new(Rc::into_raw(task) as *const (), &WAKER_VTABLE)
}

unsafe fn waker_wake(data: *const ()) {
    let task = TaskRef::from_raw(data as *const TaskContent);
    task.wake();
}

unsafe fn waker_wake_by_ref(data: *const ()) {
    let task = TaskRef::from_raw(data as *const TaskContent);
    TaskContent::wake(task.clone());
    Rc::into_raw(task);
}

unsafe fn waker_drop(data: *const ()) {
    TaskRef::from_raw(data as *const TaskContent);
}

const WAKER_VTABLE: std::task::RawWakerVTable =
    std::task::RawWakerVTable::new(waker_clone, waker_wake, waker_wake_by_ref, waker_drop);

struct UnsafeRawWaker {
    data: *const (),
    _vtable: &'static std::task::RawWakerVTable,
}

#[repr(transparent)]
struct UnsafeWaker {
    waker: UnsafeRawWaker,
}

pub(super) fn waker_task(waker: std::task::Waker) -> TaskRef {
    let a = unsafe {
        TaskRef::from_raw(
            (&waker as *const std::task::Waker as *const UnsafeWaker)
                .as_ref()
                .unwrap()
                .waker
                .data as *const TaskContent,
        )
    };
    std::mem::forget(waker);
    a
}

enum TimeEventType {
    Timeout,
    Wake,
}

struct TimeEvent {
    when: u64, //Unix time in millisecond
    task: TaskRef,
    event_type: TimeEventType,
}

impl PartialEq for TimeEvent {
    fn eq(&self, o: &Self) -> bool {
        return self.when == o.when;
    }
}

impl Eq for TimeEvent {}
impl PartialOrd for TimeEvent {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        return Some(std::cmp::Ord::cmp(&self.when, &o.when));
    }
}

pub struct Reactor {
    ready: RefCell<TaskQueue>,
    pub(super) ring: RefCell<io_uring>,
    #[cfg(feature = "verbs")]
    pub device: RefCell<verbs_util::Device>,
    #[cfg(feature = "verbs")]
    waiting_for_verbs_buffer: RefCell<TaskQueue>,
}

pub(super) type ReactorRef = Rc<Reactor>;

impl Reactor {
    #[cfg(feature = "verbs")]
    pub(super) fn wait_verbs_buffer(&self, t: TaskRef) {
        self.waiting_for_verbs_buffer.borrow_mut().push(t)
    }

    #[cfg(feature = "verbs")]
    pub(super) fn get_verbs_buffer(&self) -> Option<verbs_util::Buffer> {
        self.device.borrow_mut().free_buffers.pop()
    }

    #[cfg(feature = "verbs")]
    pub(super) fn put_verbs_buffer(&self, buffer: verbs_util::Buffer) {
        self.device.borrow_mut().free_buffers.push(buffer)
    }

    pub fn new(_size: u32) -> Result<ReactorRef> {
        #[cfg(feature = "verbs")]
        let mut device = verbs_util::Device::new(None, _size)?;

        let mut r = Rc::new(Reactor {
            ready: RefCell::new(TaskQueue::new()),
            ring: unsafe { std::mem::zeroed() },
            #[cfg(feature = "verbs")]
            device: RefCell::new(device),
            #[cfg(feature = "verbs")]
            waiting_for_verbs_buffer: RefCell::new(TaskQueue::new()),
        });

        unsafe {
            let ret =
                io_uring_queue_init(128, &mut *Rc::get_mut(&mut r).unwrap().ring.borrow_mut(), 0);
            if ret < 0 {
                return Err(Error::from(std::io::Error::last_os_error()));
            }
        }
        Ok(r)
    }

    pub fn spawn<F: Future<Output = Result<()>> + 'static>(
        self: &ReactorRef,
        priority: Priority,
        future: F,
    ) -> Task {
        let task = TaskRef::new(TaskContent::new(future, priority, self.clone()));
        self.ready.borrow_mut().push(task.clone());
        Task { content: task }
    }

    pub fn run(self: &ReactorRef) -> Result<()> {
        loop {
            // TODO (jakobt) possible post verbs recieve here
            // Poll verbs here
            #[cfg(feature = "verbs")]
            {
                self.device.borrow_mut().process();

                // Wake up a task waiting for free verbs buffers
                if !self.device.borrow().free_buffers.is_empty() {
                    if let Some(v) = self.waiting_for_verbs_buffer.borrow_mut().pop() {
                        self.ready.borrow_mut().push(v)
                    }
                }
            }

            // Run ready tasks
            let task = self.ready.borrow_mut().pop();
            if let Some(task) = task {
                let raw = std::task::RawWaker::new(
                    Rc::into_raw(task.clone()) as *const (),
                    &WAKER_VTABLE,
                );
                let waker = unsafe { std::task::Waker::from_raw(raw) };
                let mut context = std::task::Context::from_waker(&waker);
                match task
                    .as_ref()
                    .future
                    .borrow_mut()
                    .as_mut()
                    .poll(&mut context)
                {
                    Poll::Pending => {}
                    Poll::Ready(Ok(())) => {
                        println!("Task finished successfully");
                    }
                    Poll::Ready(Err(e)) => {
                        println!("TaskFailed {}", e);
                    }
                }
                continue;
            }

            //TODO we should pool the queu and the verbs queues for a bit before handing over to the os for a wait

            unsafe {
                // TODO we should handle all entries here
                let mut ring = self.ring.borrow_mut();
                io_uring_submit(&mut *ring);

                let mut cqe: *mut io_uring_cqe = std::ptr::null_mut();
                info!("Wait for event");

                let ret = __io_uring_get_cqe(&mut *ring, &mut cqe, 0, 1, std::ptr::null_mut());
                if ret < 0 {
                    return Err(Error::from(std::io::Error::last_os_error()));
                }
                info!("Got event");
                let cqe = cqe
                    .as_mut()
                    .ok_or(Error::Internal("Got null cqe pointer"))?;

                let task = TaskRef::from_raw(cqe.user_data as *const TaskContent);

                match task.as_ref().state.get() {
                    TaskState::UringWaiting => {
                        task.as_ref().state.set(TaskState::UringDone(cqe.res))
                    }
                    TaskState::UringCanceling => task.as_ref().state.set(TaskState::Cancled),
                    TaskState::UringTimeouting => task.as_ref().state.set(TaskState::Timeouted),
                    v => panic!("Unexpected task state on uring result {:?}", v),
                }

                self.ready.borrow_mut().push(task);
                std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
                *ring.cq.khead.as_mut().unwrap() += 1;
                std::sync::atomic::fence(std::sync::atomic::Ordering::Release);
            }
        }
    }
}

impl Drop for Reactor {
    fn drop(&mut self) {
        unsafe {
            io_uring_queue_exit(self.ring.get_mut());
        }
    }
}

pub(super) fn io_uring_get_sqe_submit(
    ring: *mut io_uring,
) -> Result<std::ptr::NonNull<io_uring_sqe>> {
    loop {
        let sqe = unsafe { io_uring_get_sqe(ring) };
        if let Some(p) = std::ptr::NonNull::new(sqe) {
            return Ok(p);
        }
        // If we could not allocate an seq
        if unsafe { io_uring_submit(ring) < 0 } {
            return Err(Error::from(std::io::Error::last_os_error()));
        }
    }
}
