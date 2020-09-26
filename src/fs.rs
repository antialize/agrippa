use crate::io_uring_util::{Close, Fd, OpenAt, Read, Write};
use crate::runtime::{Error, Result};
use libc;
use std::path::Path;

pub struct OpenOptions {
    read: bool,
    write: bool,
    truncate: bool,
    append: bool,
    create: bool,
    exclusive: bool,
    close_on_exec: bool,
    direct: bool,
    no_atime: bool,
    no_follow: bool,
    temp_file: bool,
    user_read: bool,
    user_write: bool,
    user_execute: bool,
    group_read: bool,
    group_write: bool,
    group_execute: bool,
    other_read: bool,
    other_write: bool,
    other_execute: bool,
    set_user_id: bool,
    set_group_id: bool,
}
impl OpenOptions {
    pub fn new() -> Self {
        OpenOptions {
            read: true,
            write: false,
            truncate: false,
            append: false,
            create: false,
            exclusive: false,
            close_on_exec: true,
            direct: false,
            no_atime: true,
            no_follow: false,
            temp_file: false,
            user_read: true,
            user_write: true,
            user_execute: false,
            group_read: false,
            group_write: false,
            group_execute: false,
            other_read: false,
            other_write: false,
            other_execute: false,
            set_user_id: false,
            set_group_id: false,
        }
    }
    pub fn read(&mut self, read: bool) -> &mut Self {
        self.read = read;
        self
    }
    pub fn write(&mut self, write: bool) -> &mut Self {
        self.write = write;
        self
    }
    pub fn truncate(&mut self, truncate: bool) -> &mut Self {
        self.truncate = truncate;
        self
    }
    pub fn append(&mut self, append: bool) -> &mut Self {
        self.append = append;
        self
    }
    pub fn create(&mut self, create: bool) -> &mut Self {
        self.create = create;
        self
    }
    pub fn exclusive(&mut self, exclusive: bool) -> &mut Self {
        self.exclusive = exclusive;
        self
    }
    pub fn close_on_exec(&mut self, close_on_exec: bool) -> &mut Self {
        self.close_on_exec = close_on_exec;
        self
    }
    pub fn direct(&mut self, direct: bool) -> &mut Self {
        self.direct = direct;
        self
    }
    pub fn no_atime(&mut self, no_atime: bool) -> &mut Self {
        self.no_atime = no_atime;
        self
    }
    pub fn no_follow(&mut self, no_follow: bool) -> &mut Self {
        self.no_follow = no_follow;
        self
    }
    pub fn temp_file(&mut self, temp_file: bool) -> &mut Self {
        self.temp_file = temp_file;
        self
    }
    pub fn user_read(&mut self, user_read: bool) -> &mut Self {
        self.user_read = user_read;
        self
    }
    pub fn user_write(&mut self, user_write: bool) -> &mut Self {
        self.user_write = user_write;
        self
    }
    pub fn user_execute(&mut self, user_execute: bool) -> &mut Self {
        self.user_execute = user_execute;
        self
    }
    pub fn group_read(&mut self, group_read: bool) -> &mut Self {
        self.group_read = group_read;
        self
    }
    pub fn group_write(&mut self, group_write: bool) -> &mut Self {
        self.group_write = group_write;
        self
    }
    pub fn group_execute(&mut self, group_execute: bool) -> &mut Self {
        self.group_execute = group_execute;
        self
    }
    pub fn other_read(&mut self, other_read: bool) -> &mut Self {
        self.other_read = other_read;
        self
    }
    pub fn other_write(&mut self, other_write: bool) -> &mut Self {
        self.other_write = other_write;
        self
    }
    pub fn other_execute(&mut self, other_execute: bool) -> &mut Self {
        self.other_execute = other_execute;
        self
    }
    pub fn set_user_id(&mut self, set_user_id: bool) -> &mut Self {
        self.set_user_id = set_user_id;
        self
    }
    pub fn set_group_id(&mut self, set_group_id: bool) -> &mut Self {
        self.set_group_id = set_group_id;
        self
    }

    fn flags(&self) -> u32 {
        let mut flags = 0;
        if self.read && self.write {
            flags |= libc::O_RDWR;
        } else if self.read {
            flags |= libc::O_RDONLY;
        } else if self.write {
            flags |= libc::O_WRONLY;
        }
        if self.truncate {
            flags |= libc::O_TRUNC;
        }
        if self.append {
            flags |= libc::O_APPEND;
        }
        if self.create {
            flags |= libc::O_CREAT;
        }
        if self.exclusive {
            flags |= libc::O_EXCL;
        }
        if self.close_on_exec {
            flags |= libc::O_CLOEXEC;
        }
        if self.direct {
            flags |= libc::O_DIRECT;
        }
        if self.no_atime {
            flags |= libc::O_NOATIME;
        }
        if self.no_follow {
            flags |= libc::O_NOFOLLOW;
        }
        if self.temp_file {
            flags |= libc::O_TMPFILE;
        }
        flags as u32
    }

    fn mode(&self) -> u32 {
        let mut mode = 0;
        if self.user_read {
            mode |= libc::S_IRUSR;
        }
        if self.user_write {
            mode |= libc::S_IWUSR;
        }
        if self.user_execute {
            mode |= libc::S_IXUSR;
        }
        if self.group_read {
            mode |= libc::S_IRGRP;
        }
        if self.group_write {
            mode |= libc::S_IWGRP;
        }
        if self.group_execute {
            mode |= libc::S_IXGRP;
        }
        if self.other_read {
            mode |= libc::S_IROTH;
        }
        if self.other_write {
            mode |= libc::S_IWOTH;
        }
        if self.other_execute {
            mode |= libc::S_IXOTH;
        }
        if self.set_user_id {
            mode |= libc::S_ISUID;
        }
        if self.set_group_id {
            mode |= libc::S_ISGID;
        }
        mode
    }

    pub async fn open<P: AsRef<Path>>(&self, path: P) -> Result<File> {
        use std::os::unix::ffi::OsStrExt;

        let path = std::ffi::CString::new(path.as_ref().as_os_str().as_bytes())?;
        let fd = OpenAt::new(path.as_ref(), None, self.flags(), self.mode()).await?;
        return Ok(File { fd });
    }
}

pub struct File {
    fd: Fd,
}

impl File {
    pub async fn open<P: AsRef<Path>>(path: P) -> Result<File> {
        OpenOptions::new().open(path).await
    }

    pub async fn create<P: AsRef<Path>>(path: P) -> Result<File> {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .await
    }

    //TODO openat openat2 statx fadvice madvice

    pub async fn close(self) -> Result<()> {
        let Self { fd } = self;
        Close::new(fd).await?;
        Ok(())
    }

    pub async fn write(&self, data: &[u8], offset: u64) -> Result<()> {
        let mut start = 0;

        while start != data.len() {
            //TODO Handle EINTR and EAGAIN
            let written = Write::new(&self.fd, &data[start..], offset + start as u64).await?;
            if written == 0 {
                return Err(Error::Eof);
            }
            start += written;
        }
        Ok(())
    }

    pub async fn read(&self, data: &mut [u8], offset: u64) -> Result<usize> {
        Read::new(&self.fd, data, offset).await
    }

    pub async fn read_all(&self) -> Result<Vec<u8>> {
        let mut data: Vec<u8> = Vec::new();
        data.resize(128 * 1024, 0);
        let mut start = 0;
        loop {
            let read = Read::new(&self.fd, &mut data[start..], start as u64).await?;
            start += read;
            if start != data.len() {
                data.resize(start, 0);
                return Ok(data);
            }
            data.resize(data.len() * 2, 0);
        }
    }
}

// impl Drop for File {
//     fn drop(&mut self) {
//         debug!("File closed synchronosly");
//         libc::close(self.fd);
//     }
// }
