use crate::io_uring_util::{Accept, Close, Connect, Fd, Read, Write};
use crate::runtime::{Error, Result};
use libc;
use log::info;
use std::net::TcpListener;

/// Listening socket that can be used to accept connections
pub struct ListenSocket {
    fd: Fd,
}

impl ListenSocket {
    /// Accept a new connection from the socket
    pub async fn accept(&self) -> Result<Socket> {
        let (fd, address, len) = Accept::new(&self.fd).await?;
        Ok(Socket { fd })
    }

    /// Close the listener
    pub async fn close(self) -> Result<()> {
        Close::new(self.fd).await
    }
}

/// Listen to the given tcp address
///
/// # Example
///
/// ```
/// listen("127.0.0.1:1234").await?
/// ```
pub async fn listen<A: std::net::ToSocketAddrs>(address: A) -> Result<ListenSocket> {
    let listener = TcpListener::bind(address)?;
    //info!("Listening on {}", address);
    Ok(ListenSocket {
        fd: Fd {
            fd: std::os::unix::io::IntoRawFd::into_raw_fd(listener),
        },
    })
}

/// Regular tcp socket
pub struct Socket {
    fd: Fd,
}

impl Socket {
    /// Write bytes to socket
    pub async fn write(&self, data: &[u8]) -> Result<()> {
        let mut start = 0;
        while start != data.len() {
            //TODO Handle EINTR and EAGAIN
            let written = Write::new(&self.fd, &data[start..], 0).await?;
            if written == 0 {
                return Err(Error::Eof);
            }
            start += written;
        }
        Ok(())
    }

    pub async fn write_item<T: Copy>(&self, item: &T) -> Result<()> {
        unsafe {
            self.write(std::slice::from_raw_parts(
                item as *const T as *const u8,
                std::mem::size_of::<T>(),
            ))
        }
        .await
    }

    /// Read data from socket into data, return number of bytes read
    pub async fn read(&self, data: &mut [u8]) -> Result<usize> {
        Read::new(&self.fd, data, 0).await
    }

    pub async fn read_all(&self, data: &mut [u8]) -> Result<()> {
        let mut start = 0;
        while start != data.len() {
            let read = Read::new(&self.fd, &mut data[start..], 0).await?;
            if read == 0 {
                return Err(Error::Eof);
            }
            start += read;
        }
        Ok(())
    }

    pub async unsafe fn read_item<T: Copy>(&self) -> Result<T> {
        let mut item: T = std::mem::zeroed();
        self.read_all(std::slice::from_raw_parts_mut(
            &mut item as *mut T as *mut u8,
            std::mem::size_of::<T>(),
        ))
        .await?;
        Ok(item)
    }

    /// Close this socket for reading and writing
    pub async fn close(self) -> Result<()> {
        Close::new(self.fd).await
    }
}

struct SocketAddrV4Copy {
    inner: libc::sockaddr_in,
}

struct SocketAddrV6Copy {
    inner: libc::sockaddr_in6,
}

/**
 * Connect to a remove service
 */
pub async fn connect<A: std::net::ToSocketAddrs>(address: A) -> Result<Socket> {
    let iter = address.to_socket_addrs()?;
    for addr in iter {
        let (domain, addr, addr_size) = match &addr {
            std::net::SocketAddr::V4(addr) => (
                libc::AF_INET,
                &unsafe { &*(addr as *const std::net::SocketAddrV4 as *const SocketAddrV4Copy) }
                    .inner as *const libc::sockaddr_in as *const libc::c_void,
                std::mem::size_of::<libc::sockaddr_in>(),
            ),
            std::net::SocketAddr::V6(addr) => (
                libc::AF_INET6,
                &unsafe { &*(addr as *const std::net::SocketAddrV6 as *const SocketAddrV6Copy) }
                    .inner as *const libc::sockaddr_in6 as *const libc::c_void,
                std::mem::size_of::<libc::sockaddr_in6>(),
            ),
        };

        let fd = unsafe { libc::socket(domain, libc::SOCK_STREAM, 0) };
        if fd == -1 {
            return Err(Error::from(std::io::Error::last_os_error()));
        }
        let fd = Fd { fd };
        Connect::new(&fd, addr, addr_size).await?;
        return Ok(Socket { fd });
    }
    Err(Error::Internal("Unable to connect"))
}
