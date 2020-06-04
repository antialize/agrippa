use crate::sys::{
    _compat_ibv_port_attr, ib_uverbs_comp_event_desc, ibv_access_flags, ibv_ack_cq_events,
    ibv_alloc_pd, ibv_close_device, ibv_comp_channel, ibv_context, ibv_cq, ibv_create_comp_channel,
    ibv_create_cq, ibv_create_qp, ibv_create_srq, ibv_dealloc_pd, ibv_dereg_mr,
    ibv_destroy_comp_channel, ibv_destroy_cq, ibv_destroy_qp, ibv_destroy_srq, ibv_device,
    ibv_free_device_list, ibv_get_cq_event, ibv_get_device_list, ibv_get_device_name,
    ibv_modify_qp, ibv_mr, ibv_mtu_IBV_MTU_1024, ibv_open_device, ibv_pd, ibv_port_attr, ibv_qp,
    ibv_qp_attr,
    ibv_qp_attr_mask::{
        IBV_QP_ACCESS_FLAGS, IBV_QP_AV, IBV_QP_CAP, IBV_QP_DEST_QPN, IBV_QP_MAX_DEST_RD_ATOMIC,
        IBV_QP_MAX_QP_RD_ATOMIC, IBV_QP_MIN_RNR_TIMER, IBV_QP_PATH_MTU, IBV_QP_PKEY_INDEX,
        IBV_QP_PORT, IBV_QP_RETRY_CNT, IBV_QP_RNR_RETRY, IBV_QP_RQ_PSN, IBV_QP_SQ_PSN,
        IBV_QP_STATE, IBV_QP_TIMEOUT,
    },
    ibv_qp_init_attr, ibv_qp_state, ibv_qp_type, ibv_query_port, ibv_query_qp, ibv_recv_wr,
    ibv_reg_mr,
    ibv_send_flags::IBV_SEND_SIGNALED,
    ibv_send_wr, ibv_sge, ibv_srq, ibv_srq_init_attr, ibv_wc,
    ibv_wc_opcode::{IBV_WC_RECV, IBV_WC_SEND},
    ibv_wr_opcode::IBV_WR_SEND,
    IBV_LINK_LAYER_ETHERNET,
};

use crate::io_uring_util::{Fd, Read};
use crate::runtime::{Task, TaskRef};
use libc;
use libc::c_int;
use log::info;
use std::cell::RefCell;
use std::ffi::{c_void, CStr};
use std::ptr::null_mut;
use std::rc::Rc;

unsafe fn ibv_req_notify_cq(cq: *mut ibv_cq, solicited_only: c_int) -> c_int {
    let fp = (*(*cq).context).ops.req_notify_cq.unwrap();
    fp(cq, solicited_only)
}

unsafe fn ibv_post_send(
    qp: *mut ibv_qp,
    wr: *mut ibv_send_wr,
    bad_wr: *mut *mut ibv_send_wr,
) -> c_int {
    let fp = (*(*qp).context).ops.post_send.unwrap();
    fp(qp, wr, bad_wr)
}

unsafe fn ibv_post_recv(
    qp: *mut ibv_qp,
    wr: *mut ibv_recv_wr,
    bad_wr: *mut *mut ibv_recv_wr,
) -> c_int {
    let fp = (*(*qp).context).ops.post_recv.unwrap();
    fp(qp, wr, bad_wr)
}

unsafe fn ibv_post_srq_recv(
    srq: *mut ibv_srq,
    wr: *mut ibv_recv_wr,
    bad_wr: *mut *mut ibv_recv_wr,
) -> c_int {
    let fp = (*(*srq).context).ops.post_srq_recv.unwrap();
    fp(srq, wr, bad_wr)
}

unsafe fn ibv_poll_cq(cq: *mut ibv_cq, num_entries: c_int, wc: *mut ibv_wc) -> c_int {
    let fp = (*(*cq).context).ops.poll_cq.unwrap();
    fp(cq, num_entries, wc)
}

unsafe fn ibv_get_cq_event_my(
    channel: *const ibv_comp_channel,
    cq: *mut *mut ibv_cq,
    cq_context: *mut *mut c_void,
) -> c_int {
    let mut ev: ib_uverbs_comp_event_desc = std::mem::zeroed();
    if libc::read(
        (*channel).fd,
        &mut ev as *mut ib_uverbs_comp_event_desc as *mut c_void,
        std::mem::size_of::<ib_uverbs_comp_event_desc>(),
    ) != std::mem::size_of::<ib_uverbs_comp_event_desc>() as isize
    {
        return -1;
    }
    let x = ev.cq_handle as *mut ibv_cq;
    *cq = x;
    *cq_context = (*x).cq_context;
    let fp: Option<unsafe extern "C" fn(cq: *mut ibv_cq)> =
        std::mem::transmute((*(*x).context).ops._compat_cq_event);

    (fp.unwrap())(*cq);
    return 0;
}

pub struct Buffer {
    buf: *mut c_void,
    offset: usize,
    used: usize,
    capacity: usize,
    mr: *mut ibv_mr,
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            if !self.mr.is_null() {
                ibv_dereg_mr(self.mr);
                self.mr = null_mut();
            }
            if !self.buf.is_null() {
                libc::free(self.buf);
                self.buf = null_mut();
            }
            self.offset = 0;
            self.used = 0;
            self.capacity = 0;
        }
    }
}

impl Buffer {
    pub(super) fn new(d: &Device) -> std::io::Result<Buffer> {
        unsafe {
            let mut r = Buffer {
                buf: null_mut(),
                offset: 0,
                used: 0,
                capacity: d.size,
                mr: null_mut(),
            };
            r.buf = libc::memalign(4 * 1024, r.capacity);
            if r.buf.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            libc::memset(r.buf, 0, r.capacity);

            r.mr = ibv_reg_mr(
                d.pd,
                r.buf,
                r.capacity as u64,
                ibv_access_flags::IBV_ACCESS_LOCAL_WRITE as i32,
            );
            if r.mr.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            Ok(r)
        }
    }
}

// LATEST_SYMVER_FUNC(ibv_get_cq_event, 1_1, "IBVERBS_1.1",
// 		   int,
// 		   struct  *channel,
// 		   struct ibv_cq **cq, void **)
// {
// 	struct  ev;

// 	if (read(channel->fd, &ev, sizeof ev) != sizeof ev)
// 		return -1;

// 	*cq         = (struct ibv_cq *) (uintptr_t) ev.cq_handle;
// 	*cq_context = (*cq)->cq_context;

// 	get_ops((*cq)->context)->cq_event(*cq);

// 	return 0;
// }

#[repr(C, packed(1))]
#[derive(Copy, Clone, Debug)]
pub struct VerbsAddr {
    qpn: u32,
    psn: u32,
    gid: u128,
    lid: u16,
}

pub(super) struct QueuePair {
    qp: *mut ibv_qp,
    psn: u32,
    received: RefCell<std::collections::VecDeque<Buffer>>,
    waiting: RefCell<Option<TaskRef>>,
}

impl QueuePair {
    pub(super) fn local_address(&self, device: &Device) -> VerbsAddr {
        VerbsAddr {
            qpn: unsafe { (*self.qp).qp_num },
            psn: self.psn,
            lid: device.port_info.lid,
            gid: 0,
        }
    }

    pub(super) fn read(&self) -> Option<Buffer> {
        self.received.borrow_mut().pop_front()
    }

    pub(super) fn wait(&self, task: TaskRef) -> std::io::Result<()> {
        let mut w = self.waiting.borrow_mut();
        if w.is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Two concurent reads are not supported",
            ));
        }
        *w = Some(task);
        Ok(())
    }

    pub(super) unsafe fn send(&self, task: TaskRef, buffer: &Buffer) -> std::io::Result<()> {
        let mut list: ibv_sge = std::mem::zeroed();
        list.addr = buffer.buf as u64; //TODO add offset
        list.length = buffer.used as u32;
        list.lkey = (*buffer.mr).lkey;

        let mut wr: ibv_send_wr = std::mem::zeroed();
        wr.wr_id = Rc::into_raw(task) as usize as u64;
        wr.sg_list = &mut list;
        wr.num_sge = 1;
        wr.opcode = IBV_WR_SEND;
        wr.send_flags = IBV_SEND_SIGNALED;

        let mut bad_wr: *mut ibv_send_wr = std::ptr::null_mut();

        info!("Sending buffer");
        if ibv_post_send(self.qp, &mut wr, &mut bad_wr) != 0 {
            Rc::from_raw(wr.wr_id as *mut Task);
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub(super) fn connect(&mut self, device: &Device, remote: &VerbsAddr) -> std::io::Result<()> {
        unsafe {
            let mut attr: ibv_qp_attr = std::mem::zeroed();
            attr.qp_state = ibv_qp_state::IBV_QPS_RTR;
            attr.path_mtu = ibv_mtu_IBV_MTU_1024;
            attr.dest_qp_num = remote.qpn;
            attr.rq_psn = remote.psn;
            attr.max_dest_rd_atomic = 1;
            attr.min_rnr_timer = 12;
            attr.ah_attr.is_global = 0;
            attr.ah_attr.dlid = remote.lid;
            attr.ah_attr.sl = 0; //TODO WHAT IS THIS
            attr.ah_attr.src_path_bits = 0;
            attr.ah_attr.port_num = device.ib_port;

            // if (dest->gid.global.interface_id) {
            //   attr.ah_attr.is_global = 1;
            //   attr.ah_attr.grh.hop_limit = 1;
            //   attr.ah_attr.grh.dgid = dest->gid;
            //   attr.ah_attr.grh.sgid_index = sgid_idx;
            // }

            if ibv_modify_qp(
                self.qp,
                &mut attr,
                (IBV_QP_STATE
                    | IBV_QP_AV
                    | IBV_QP_PATH_MTU
                    | IBV_QP_DEST_QPN
                    | IBV_QP_RQ_PSN
                    | IBV_QP_MAX_DEST_RD_ATOMIC
                    | IBV_QP_MIN_RNR_TIMER) as i32,
            ) != 0
            {
                return Err(std::io::Error::last_os_error());
            }

            attr.qp_state = ibv_qp_state::IBV_QPS_RTS;
            attr.timeout = 14;
            attr.retry_cnt = 7;
            attr.rnr_retry = 7;
            attr.sq_psn = self.psn;
            attr.max_rd_atomic = 1;
            if ibv_modify_qp(
                self.qp,
                &mut attr,
                (IBV_QP_STATE
                    | IBV_QP_TIMEOUT
                    | IBV_QP_RETRY_CNT
                    | IBV_QP_RNR_RETRY
                    | IBV_QP_SQ_PSN
                    | IBV_QP_MAX_QP_RD_ATOMIC) as i32,
            ) != 0
            {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(())
    }

    pub fn new(c: &Device) -> std::io::Result<Self> {
        unsafe {
            let mut r = QueuePair {
                qp: std::ptr::null_mut(),
                psn: rand::random::<u32>() & 0xFFFFFF,
                received: RefCell::new(std::collections::VecDeque::new()),
                waiting: RefCell::new(None),
            };

            let mut init_attr: ibv_qp_init_attr = std::mem::zeroed();
            init_attr.send_cq = c.cq;
            init_attr.recv_cq = c.cq;
            init_attr.srq = c.srq;
            init_attr.cap.max_send_wr = 1;
            init_attr.cap.max_recv_wr = c.rx_depth;
            init_attr.cap.max_send_sge = 1;
            init_attr.cap.max_recv_sge = 1;
            init_attr.qp_type = ibv_qp_type::IBV_QPT_RC;

            r.qp = ibv_create_qp(c.pd, &mut init_attr);
            if r.qp.is_null() {
                return Err(std::io::Error::last_os_error());
            }

            let mut attr: ibv_qp_attr = std::mem::zeroed();
            attr.qp_state = ibv_qp_state::IBV_QPS_INIT;
            attr.pkey_index = 0;
            attr.port_num = c.ib_port;
            attr.qp_access_flags = 0;

            if ibv_modify_qp(
                r.qp,
                &mut attr,
                (IBV_QP_STATE | IBV_QP_PKEY_INDEX | IBV_QP_PORT | IBV_QP_ACCESS_FLAGS) as i32,
            ) != 0
            {
                return Err(std::io::Error::last_os_error());
            }

            Ok(r)
        }
    }
}

impl Drop for QueuePair {
    fn drop(&mut self) {
        unsafe {
            if !self.qp.is_null() {
                ibv_destroy_qp(self.qp);
                self.qp = null_mut();
            }
            self.psn = 0;
        }
    }
}

pub struct Device {
    rx_depth: u32,
    size: usize,
    device_list: *mut *mut ibv_device,
    context: *mut ibv_context,
    name: String,
    channel: *mut ibv_comp_channel,
    pd: *mut ibv_pd,
    ib_port: u8,
    cq: *mut ibv_cq,
    port_info: ibv_port_attr,
    srq: *mut ibv_srq,

    notify_enabled: bool,
    events_pending: usize,

    read_slot: Vec<Option<Buffer>>,
    empty_read_slots: Vec<usize>,
    pub(super) free_buffers: Vec<Buffer>,
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            if !self.srq.is_null() {
                ibv_destroy_srq(self.srq);
                self.srq = null_mut();
            }
            if !self.cq.is_null() {
                ibv_destroy_cq(self.cq);
                self.cq = null_mut();
            }
            if !self.pd.is_null() {
                ibv_dealloc_pd(self.pd);
                self.pd = null_mut();
            }
            if !self.channel.is_null() {
                ibv_destroy_comp_channel(self.channel);
                self.channel = null_mut();
            }
            if !self.context.is_null() {
                ibv_close_device(self.context);
                self.context = null_mut();
            }
            if !self.device_list.is_null() {
                ibv_free_device_list(self.device_list);
                self.device_list = null_mut();
            }
        }
    }
}

impl Device {
    /*pub fn addr(&self) -> VerbsAddr {
        VerbsAddr {
            qpn: self.qpn,
            psn: self.psn,
            lid: self.port_info.lid,
            gid: 0,
        }
    }*/

    // SEE https://www.ibm.com/support/knowledgecenter/ssw_aix_72/rdma/client_active_example.html

    // pub fn send_ping(&self) -> std::io::Result<()> {
    //     unsafe {
    //         let mut list: ibv_sge = std::mem::zeroed();
    //         list.addr = self.buf as u64;
    //         list.length = self.size as u32;
    //         list.lkey = (*self.mr).lkey;

    //         let mut wr: ibv_send_wr = std::mem::zeroed();
    //         wr.wr_id = 2;
    //         wr.sg_list = &mut list;
    //         wr.num_sge = 1;
    //         wr.opcode = IBV_WR_SEND;
    //         wr.send_flags = IBV_SEND_SIGNALED;

    //         let mut bad_wr: *mut ibv_send_wr = std::ptr::null_mut();

    //         std::thread::sleep(std::time::Duration::from_millis(1000));
    //         info!("send_ping 1 {:?}", std::io::Error::last_os_error());
    //         if ibv_post_send(self.qp, &mut wr, &mut bad_wr) != 0 {
    //             info!("send_ping 2");

    //             return Err(std::io::Error::last_os_error());
    //         }
    //         info!("send_ping 3");

    //         Ok(())
    //     }
    // }
    /*
    pub fn pp_post_recv(&self) -> std::io::Result<u32> {
        unsafe {
            let mut list: ibv_sge = std::mem::zeroed();
            list.addr = self.buf as u64;
            list.length = self.size as u32;
            list.lkey = (*self.mr).lkey;

            info!("QQ {} {} {}", list.addr, list.length, list.lkey);
            let mut wr: ibv_recv_wr = std::mem::zeroed();
            wr.wr_id = 1;
            wr.sg_list = &mut list;
            wr.num_sge = 1;

            let mut bad_wr: *mut ibv_recv_wr = std::ptr::null_mut();

            info!("recv_ping 1");

            let mut ans = 0;
            loop {
                if ans == self.rx_depth {
                    break;
                }
                if ibv_post_srq_recv(self.srq, &mut wr, &mut bad_wr) != 0 {
                    break;
                }
                ans += 1;
            }

            Ok(ans)
        }
    }*/

    pub fn process(&mut self) -> std::io::Result<bool> {
        let mut add_notify_read = false;

        unsafe {
            info!("PROCESS");
            while !self.empty_read_slots.is_empty() && !self.free_buffers.is_empty() {
                let slot = self.empty_read_slots.pop().unwrap();
                let buf = self.free_buffers.pop().unwrap();

                let mut list: ibv_sge = std::mem::zeroed();
                list.addr = buf.buf as u64;
                list.length = buf.capacity as u32;
                list.lkey = (*buf.mr).lkey;

                let mut wr: ibv_recv_wr = std::mem::zeroed();
                wr.wr_id = slot as u64;
                wr.sg_list = &mut list;
                wr.num_sge = 1;

                self.read_slot[slot] = Some(buf);

                let mut bad_wr: *mut ibv_recv_wr = std::ptr::null_mut();
                if ibv_post_srq_recv(self.srq, &mut wr, &mut bad_wr) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                info!("ASSIGN SLOT");
            }

            if !self.notify_enabled {
                if ibv_req_notify_cq(self.cq, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                add_notify_read = true;
                self.notify_enabled = true;
            }

            const CHUNK: usize = 16;
            let mut wc: [ibv_wc; CHUNK] = [std::mem::zeroed(); CHUNK];
            loop {
                let ne = ibv_poll_cq(self.cq, CHUNK as i32, wc.as_mut_ptr());
                if ne < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if ne == 0 {
                    break;
                }
                for w in &wc[0..ne as usize] {
                    match w.opcode {
                        IBV_WC_RECV => {}
                        IBV_WC_SEND => {
                            info!("Send finished {}", w.wr_id);

                            let task = TaskRef::from_raw(w.wr_id as *const Task);
                            task.as_ref().ring_result.set(w.status as i32);
                            //self.ready.borrow_mut().push(task);
                        }
                        other => info!("Unhandled verbs opcode {}", other),
                    }
                }
                info!("WE GOT AN EVENT");
                //TODO HANDLE THE ne EVENTS
            }
            Ok(add_notify_read)
        }
    }

    //         info!("recv_ping 2");
    //         //TODO use recv_cq when we have set it up
    //

    // pub async fn recv_ping2(&self) -> std::io::Result<()> {
    //     unsafe {
    //         //unsafe fn ibv_get_cq_event_my(channel: *const ibv_comp_channel, cq: *mut *mut ibv_cq, cq_context: *mut * mut c_void ) -> c_int{
    //         let mut ev: ib_uverbs_comp_event_desc = std::mem::zeroed();
    //         let fd = Fd {
    //             fd: (*self.channel).fd,
    //         };
    //         Read::new(
    //             &fd,
    //             std::slice::from_raw_parts_mut(
    //                 &mut ev as *mut ib_uverbs_comp_event_desc as *mut u8,
    //                 std::mem::size_of::<ib_uverbs_comp_event_desc>(),
    //             ),
    //         )
    //         .await
    //         .unwrap();
    //         fd.into_raw();

    //         // if libc::read((*channel).fd, &mut ev as *mut ib_uverbs_comp_event_desc as *mut c_void, std::mem::size_of::<ib_uverbs_comp_event_desc>()) != std::mem::size_of::<ib_uverbs_comp_event_desc>() as isize {
    //         //     return -1;
    //         // }

    //         info!("Read from channel socket using ibverbs {}", ev.cq_handle);

    //         let x = ev.cq_handle as *mut ibv_cq;
    //         //*cq = x;
    //         //*cq_context = (*x).cq_context;
    //         let fp: Option<unsafe extern "C" fn(cq: *mut ibv_cq)> =
    //             std::mem::transmute((*(*x).context).ops._compat_cq_event);
    //         (fp.unwrap())(x);

    //         if ibv_req_notify_cq(self.send_cq, 0) != 0 {
    //             return Err(std::io::Error::last_os_error());
    //         }

    //         let mut wc: [ibv_wc; 2] = [std::mem::zeroed(); 2];

    //         info!("recv_ping 2");
    //         //TODO use recv_cq when we have set it up
    //         let ne = ibv_poll_cq(self.send_cq, 2, wc.as_mut_ptr());
    //         info!("CNT {}", ne);
    //         if ne <= 0 {
    //             return Ok(());
    //         }
    //         let w = &wc[0];
    //         info!("QQ {} {} {} {}", w.wr_id, w.status, w.opcode, w.byte_len);

    //         return Ok(());
    //     }
    // }

    // pub fn recv_ping(&self) -> std::io::Result<()> {
    //     unsafe {
    //         // let mut list: ibv_sge = std::mem::zeroed();
    //         // list.addr = self.buf as u64;
    //         // list.length = self.size as u32;
    //         // list.lkey = (*self.mr).lkey;

    //         // let mut wr: ibv_recv_wr = std::mem::zeroed();
    //         // wr.wr_id = 2;
    //         // wr.sg_list = &mut list;
    //         // wr.num_sge = 1;

    //         // let mut bad_wr: *mut ibv_recv_wr = std::ptr::null_mut();

    //         // info!("recv_ping 1");
    //         // if ibv_post_recv(self.qp, &mut wr, &mut bad_wr) != 0 {
    //         //     return Err(std::io::Error::last_os_error());
    //         // }

    //         loop {
    //             let mut ev_cq: *mut ibv_cq = std::ptr::null_mut();
    //             let mut ev_ctx: *mut c_void = std::ptr::null_mut();

    //             if ibv_get_cq_event_my(self.channel, &mut ev_cq, &mut ev_ctx) != 0 {
    //                 return Err(std::io::Error::last_os_error());
    //             }

    //             info!("GOT EVENT");
    //             // if (use_event) {
    //             //     struct  *;
    //             //     void          *;

    //             //     if ((ctx->channel, &ev_cq, &ev_ctx)) {
    //             //         fprintf(stderr, "Failed to get cq_event\n");
    //             //         return 1;
    //             //     }

    //             //     ++num_cq_events;

    //             //     if (ev_cq != pp_cq(ctx)) {
    //             //         fprintf(stderr, "CQ event for unknown CQ %p\n", ev_cq);
    //             //         return 1;
    //             //     }

    //             //     if (ibv_req_notify_cq(pp_cq(ctx), 0)) {
    //             //         fprintf(stderr, "Couldn't request CQ notification\n");
    //             //         return 1;
    //             //     }
    //             // }

    //             ibv_ack_cq_events(self.send_cq, 1);

    //             if ibv_req_notify_cq(self.send_cq, 0) != 0 {
    //                 return Err(std::io::Error::last_os_error());
    //             }

    //             let mut wc: [ibv_wc; 2] = [std::mem::zeroed(); 2];

    //             info!("recv_ping 2");
    //             //TODO use recv_cq when we have set it up
    //             let ne = ibv_poll_cq(self.send_cq, 2, wc.as_mut_ptr());
    //             info!("CNT {}", ne);
    //             if ne > 0 {
    //                 break;
    //             }
    //             let w = &wc[0];
    //             info!("QQ {} {} {} {}", w.wr_id, w.status, w.opcode, w.byte_len)
    //         }
    //         /*do {
    //                         ne = ibv_poll_cq(pp_cq(ctx), 2, wc);
    //                         if (ne < 0) {
    //                             fprintf(stderr, "poll CQ failed %d\n", ne);
    //                             return 1;
    //                         }
    //                     } while (!use_event && ne < 1);
    //         */
    //         Ok(())
    //     }
    // }

    // pub fn connect(&self, remote: &VerbsAddr) -> std::io::Result<()> {
    //     unsafe {
    //         let mut attr: ibv_qp_attr = std::mem::zeroed();
    //         attr.qp_state = ibv_qp_state::IBV_QPS_RTR;
    //         attr.path_mtu = ibv_mtu_IBV_MTU_1024;
    //         attr.dest_qp_num = remote.qpn;
    //         attr.rq_psn = remote.psn;
    //         attr.max_dest_rd_atomic = 1;
    //         attr.min_rnr_timer = 12;
    //         attr.ah_attr.is_global = 0;
    //         attr.ah_attr.dlid = remote.lid;
    //         attr.ah_attr.sl = 0; //TODO WHAT IS THIS
    //         attr.ah_attr.src_path_bits = 0;
    //         attr.ah_attr.port_num = self.ib_port;

    //         // if (dest->gid.global.interface_id) {
    //         //   attr.ah_attr.is_global = 1;
    //         //   attr.ah_attr.grh.hop_limit = 1;
    //         //   attr.ah_attr.grh.dgid = dest->gid;
    //         //   attr.ah_attr.grh.sgid_index = sgid_idx;
    //         // }
    //         info!("connect1 {}", std::io::Error::last_os_error());
    //         if ibv_modify_qp(
    //             self.qp,
    //             &mut attr,
    //             (IBV_QP_STATE
    //                 | IBV_QP_AV
    //                 | IBV_QP_PATH_MTU
    //                 | IBV_QP_DEST_QPN
    //                 | IBV_QP_RQ_PSN
    //                 | IBV_QP_MAX_DEST_RD_ATOMIC
    //                 | IBV_QP_MIN_RNR_TIMER) as i32,
    //         ) != 0
    //         {
    //             info!("Modify qp failed {}", std::io::Error::last_os_error());
    //             return Err(std::io::Error::last_os_error());
    //         }
    //         info!("connect2");

    //         attr.qp_state = ibv_qp_state::IBV_QPS_RTS;
    //         attr.timeout = 14;
    //         attr.retry_cnt = 7;
    //         attr.rnr_retry = 7;
    //         attr.sq_psn = self.psn;
    //         attr.max_rd_atomic = 1;
    //         if ibv_modify_qp(
    //             self.qp,
    //             &mut attr,
    //             (IBV_QP_STATE
    //                 | IBV_QP_TIMEOUT
    //                 | IBV_QP_RETRY_CNT
    //                 | IBV_QP_RNR_RETRY
    //                 | IBV_QP_SQ_PSN
    //                 | IBV_QP_MAX_QP_RD_ATOMIC) as i32,
    //         ) != 0
    //         {
    //             return Err(std::io::Error::last_os_error());
    //         }

    //         info!("connect3");

    //         Ok(())
    //     }
    // }

    pub(super) fn new(name: Option<&str>, size: u32) -> std::io::Result<Self> {
        unsafe {
            let mut c = Device {
                //buf: null_mut(),
                size: size as usize, //: 4096, //1024 * 1024 * 10,
                device_list: null_mut(),
                context: null_mut(),
                name: String::new(),
                channel: null_mut(),
                pd: null_mut(),
                //mr: null_mut(),
                ib_port: 1,
                cq: null_mut(),
                port_info: std::mem::zeroed(),
                srq: null_mut(),
                rx_depth: 30,
                events_pending: 0,
                notify_enabled: false,
                empty_read_slots: Vec::new(),
                free_buffers: Vec::new(),
                read_slot: Vec::new(),
            };

            for n in 0..c.rx_depth {
                c.read_slot.push(None);
                c.empty_read_slots.push(n as usize);
            }

            //c.buf = libc::memalign(4 * 1024, c.size);
            //libc::memset(c.buf, 0, c.size);

            c.device_list = ibv_get_device_list(null_mut());
            if c.device_list.is_null() {
                return Err(std::io::Error::last_os_error());
            }

            for n in 0.. {
                let dev = *c.device_list.offset(n);
                if dev.is_null() {
                    break;
                }
                let name_ptr = ibv_get_device_name(dev);
                if name_ptr.is_null() {
                    continue;
                }
                let dev_name = match CStr::from_ptr(name_ptr).to_str() {
                    Ok(name) => name,
                    Err(_) => continue,
                };
                if let Some(name) = name {
                    if name != dev_name {
                        continue;
                    }
                }
                c.context = ibv_open_device(dev);
                if c.context.is_null() {
                    return Err(std::io::Error::last_os_error());
                }
                c.name = dev_name.to_string();
                break;
            }
            if c.context.is_null() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "No device found",
                ));
            }

            c.channel = ibv_create_comp_channel(c.context);
            if c.channel.is_null() {
                return Err(std::io::Error::last_os_error());
            }

            c.pd = ibv_alloc_pd(c.context);
            if c.pd.is_null() {
                return Err(std::io::Error::last_os_error());
            }

            for n in 0..(c.rx_depth * 2) {
                c.free_buffers.push(Buffer::new(&c)?)
            }

            // for n in 0..c.rx_depth * 2 {
            //     info!("Allocate buffer {}", n);
            //     c.free_buffers.push(
            //         Buffer::new(&c)?
            //     )
            // }

            /* c.mr = ibv_reg_mr(
                c.pd,
                c.buf,
                c.size as u64,
                ibv_access_flags::IBV_ACCESS_LOCAL_WRITE as i32,
            );
            if c.mr.is_null() {
                return Err(std::io::Error::last_os_error());
            }*/

            c.cq = ibv_create_cq(c.context, (c.rx_depth + 1) as i32, null_mut(), c.channel, 0);
            if c.cq.is_null() {
                return Err(std::io::Error::last_os_error());
            }

            let mut attr: ibv_srq_init_attr = std::mem::zeroed();
            attr.attr.max_wr = c.rx_depth;
            attr.attr.max_sge = 1;

            c.srq = ibv_create_srq(c.pd, &mut attr);
            if c.srq.is_null() {
                return Err(std::io::Error::last_os_error());
            }

            // let mut init_attr: ibv_qp_init_attr = std::mem::zeroed();
            // init_attr.send_cq = c.send_cq;
            // init_attr.recv_cq = c.send_cq;
            // init_attr.srq = c.srq;
            // init_attr.cap.max_send_wr = 1;
            // init_attr.cap.max_recv_wr = rx_depth as u32;
            // init_attr.cap.max_send_sge = 1;
            // init_attr.cap.max_recv_sge = 1;
            // init_attr.qp_type = ibv_qp_type::IBV_QPT_RC;

            // c.qp = ibv_create_qp(c.pd, &mut init_attr);
            // if c.qp.is_null() {
            //     return Err(std::io::Error::last_os_error());
            // }

            // c.qpn = (*c.qp).qp_num;

            // //let mut attr: ibv_qp_attr = std::mem::zeroed();

            // //ibv_query_qp(c.qp, &mut attr, IBV_QP_CAP as i32, &mut init_attr);
            // //info!("O HI THERE {}", init_attr.cap.max_inline_data);
            // //     ibv_query_qp(ctx->qp, &attr, IBV_QP_CAP, &init_attr);
            // //     if (_data >= size && !use_dm)
            // //         ctx->send_flags |= IBV_SEND_INLINE;
            // // }

            // let mut attr: ibv_qp_attr = std::mem::zeroed();
            // attr.qp_state = ibv_qp_state::IBV_QPS_INIT;
            // attr.pkey_index = 0;
            // attr.port_num = c.ib_port;
            // attr.qp_access_flags = 0;

            // if ibv_modify_qp(
            //     c.qp,
            //     &mut attr,
            //     (IBV_QP_STATE | IBV_QP_PKEY_INDEX | IBV_QP_PORT | IBV_QP_ACCESS_FLAGS) as i32,
            // ) != 0
            // {
            //     return Err(std::io::Error::last_os_error());
            // }

            /*let cnt = c.pp_post_recv()?;
            info!("HI THERE {}", cnt);

            if ibv_req_notify_cq(c.cq, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }*/

            if ibv_query_port(
                c.context,
                c.ib_port,
                &mut c.port_info as *mut ibv_port_attr as *mut _compat_ibv_port_attr,
            ) != 0
            {
                return Err(std::io::Error::last_os_error());
            }

            if c.port_info.link_layer != IBV_LINK_LAYER_ETHERNET as u8 && c.port_info.lid == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Could not get local LID",
                ));
            }

            //my_dest.qpn = ctx->qp->qp_num;
            //my_dest.psn = lrand48() & 0xffffff;
            // libc::inet_ntop

            //inet_ntop(AF_INET6, &my_dest.gid, gid, sizeof gid);
            // info!(
            //     "Local adderss: {:04x}:{:06x}:{:06x}:{:032x}",
            //     c.port_info.lid, c.qpn, c.psn, 0
            // );

            info!("WE GOT HERE");
            Ok(c)
        }
    }
}
