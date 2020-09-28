use crate::io_uring_util::{Close, Fd, OpenAt, Read, Write};
use crate::runtime::{Error, Result};
use libc;
use std::path::Path;

/// Options and flags which can be used to configure how a file is opened.
///
/// This builder exposes the ability to configure how a [`File`] is opened and
/// what operations are permitted on the open file. The [`File::open`] and
/// [`File::create`] methods are aliases for commonly used options using this
/// builder.
///
/// [`File`]: struct.File.html
/// [`File::open`]: struct.File.html#method.open
/// [`File::create`]: struct.File.html#method.create
///
/// Generally speaking, when using `OpenOptions`, you'll first call [`new`],
/// then chain calls to methods to set each option, then call [`open`],
/// passing the path of the file you're trying to open. This will give you a
/// [`io::Result`][result] with a [`File`][file] inside that you can further
/// operate on.
///
/// [`new`]: struct.OpenOptions.html#method.new
/// [`open`]: struct.OpenOptions.html#method.open
/// [result]: ../io/type.Result.html
/// [file]: struct.File.html
///
/// # Examples
///
/// Opening a file to read:
///
/// ```no_run
/// use agrippa::fs::OpenOptions;
///
/// let file = OpenOptions::new().read(true).open("foo.txt").await?;
/// ```
///
/// Opening a file for both reading and writing, as well as creating it if it
/// doesn't exist:
///
/// ```no_run
/// use agrippa::fs::OpenOptions;
///
/// let file = OpenOptions::new()
///             .read(true)
///             .write(true)
///             .create(true)
///             .open("foo.txt").await??
/// ```
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
    /// Creates a new set of options ready for configuration.
    /// All options are initially set to false, except `close_on_exec`
    /// `user_read`, `user_write`, and `user_execute`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use agrippa::fs::OpenOptions;
    ///
    /// let mut options = OpenOptions::new();
    /// let file = options.read(true).open("foo.txt").await?;
    /// ```
    pub fn new() -> Self {
        OpenOptions {
            read: false,
            write: false,
            truncate: false,
            append: false,
            create: false,
            exclusive: false,
            close_on_exec: true,
            direct: false,
            no_atime: false,
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

    /// Sets the option for read access.
    ///
    /// This option, when true, will indicate that the file should be
    /// `read`-able if opened.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use agrippa::fs::OpenOptions;
    ///
    /// let file = OpenOptions::new().read(true).open("foo.txt").await?;
    /// ```
    pub fn read(&mut self, read: bool) -> &mut Self {
        self.read = read;
        self
    }

    /// Sets the option for write access.
    ///
    /// This option, when true, will indicate that the file should be
    /// `write`-able if opened.
    ///
    /// If the file already exists, any write calls on it will overwrite its
    /// contents, without truncating it.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use agrippa::fs::OpenOptions;
    ///
    /// let file = OpenOptions::new().write(true).open("foo.txt").await?;
    /// ```
    pub fn write(&mut self, write: bool) -> &mut Self {
        self.write = write;
        self
    }

    /// Sets the option for truncating a previous file.
    ///
    /// If a file is successfully opened with this option set it will truncate
    /// the file to 0 length if it already exists.
    ///
    /// The file must be opened with write access for truncate to work.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use agrippa::fs::OpenOptions;
    ///
    /// let file = OpenOptions::new().write(true).truncate(true).open("foo.txt").await;
    /// ```
    pub fn truncate(&mut self, truncate: bool) -> &mut Self {
        self.truncate = truncate;
        self
    }

    /// Sets the option for the append mode.
    ///
    /// This option, when true, means that writes will append to a file instead
    /// of overwriting previous contents.
    /// Note that setting `.write(true).append(true)` has the same effect as
    /// setting only `.append(true)`.
    ///
    /// For most filesystems, the operating system guarantees that all writes are
    /// atomic: no writes get mangled because another process writes at the same
    /// time.
    ///
    ///
    /// ## Note
    ///
    /// This function doesn't create the file if it doesn't exist. Use the [`create`]
    /// method to do so.
    ///
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use agrippa::fs::OpenOptions;
    ///
    /// let file = OpenOptions::new().append(true).open("foo.txt").await?;
    /// ```
    pub fn append(&mut self, append: bool) -> &mut Self {
        self.append = append;
        self.write = true;
        self
    }

    /// Sets the option to create a new file, or open it if it already exists.
    ///
    /// In order for the file to be created, [`write`] or [`append`] access must
    /// be used.
    ///
    /// [`write`]: #method.write
    /// [`append`]: #method.append
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use agrippa::fs::OpenOptions;
    ///
    /// let file = OpenOptions::new().write(true).create(true).open("foo.txt").await?;
    /// ```
    pub fn create(&mut self, create: bool) -> &mut Self {
        self.create = create;
        self
    }

    /// Ensure that this call creates the file: if this flag is specified in conjunction
    /// with `create(true)`, and pathname already exists, then `open()` fails with the error EEXIST.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use agrippa::fs::OpenOptions;
    ///
    /// let file = OpenOptions::new().write(true).create(true).exclusive(true).open("foo.txt").await?;
    /// ```
    pub fn exclusive(&mut self, exclusive: bool) -> &mut Self {
        self.exclusive = exclusive;
        self
    }

    /// Enable the close-on-exec flag for the new file descriptor.
    /// Specifying this flag permits a program to avoid additional fcntl(2) F_SETFD operations
    /// to set the FD_CLOEXEC flag.
    ///
    /// # Note
    /// This flag is set by default
    ///
    /// # Note
    /// The use of this flag is essential in some multithreaded programs,
    /// because using a separate fcntl(2) F_SETFD operation to set the FD_CLOEXEC flag does not suffice
    /// to avoid race conditions where one thread opens  a  file  descriptor  and  attempts  to set its
    /// close-on-exec flag using fcntl(2) at the same time as another thread does a fork(2) plus execve(2).
    /// Depending on the order of execution, the race may lead to the file descriptor returned by open()
    /// being unintentionally leaked to the program executed by the child process created by fork(2).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use agrippa::fs::OpenOptions;
    ///
    /// let file = OpenOptions::new().read(true).close_on_exec(false).open("foo.txt").await?;
    /// ```
    pub fn close_on_exec(&mut self, close_on_exec: bool) -> &mut Self {
        self.close_on_exec = close_on_exec;
        self
    }

    /// Try to minimize cache effects of the I/O to and from this file.  In general this will degrade performance,
    /// but it is useful in special situations, such as when applications do their own caching.
    /// File I/O is done directly to/from user-space buffers.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use agrippa::fs::OpenOptions;
    ///
    /// let file = OpenOptions::new().read(true).direct(false).open("foo.txt").await?;
    /// ```
    pub fn direct(&mut self, direct: bool) -> &mut Self {
        self.direct = direct;
        self
    }

    /// Do not update the file last access time (st_atime in the inode) when the file is read(2).
    ///
    /// This flag can be employed only if one of the following conditions is true:
    /// * The effective UID of the process matches the owner UID of the file.
    /// * The calling process has the CAP_FOWNER capability in its user namespace and the owner UID of
    ///   the file has a mapping in the namespace.
    ///
    /// This flag is intended for use by indexing or backup programs, where its use can significantly reduce
    /// the amount of disk activity.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use agrippa::fs::OpenOptions;
    ///
    /// let file = OpenOptions::new().read(true).no_atime(true).open("foo.txt").await?;
    /// ```
    pub fn no_atime(&mut self, no_atime: bool) -> &mut Self {
        self.no_atime = no_atime;
        self
    }

    /// If pathname is a symbolic link,then the open fails, with the error ELOOP.
    /// Symbolic links in earlier components of the pathname will still be followed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use agrippa::fs::OpenOptions;
    ///
    /// let file = OpenOptions::new().read(true).no_follow(true).open("foo.txt").await?;
    /// ```
    pub fn no_follow(&mut self, no_follow: bool) -> &mut Self {
        self.no_follow = no_follow;
        self
    }

    /// Create an unnamed temporary regular file. The pathname argument specifies a directory;
    /// an unnamed inode will be created in that directory's filesystem.
    /// Anything written to the resulting file will be lost when the last file descriptor is closed,
    /// unless the file is given a name.
    ///
    /// This flag must be specified with `write(true)`. If `exclusive(true)` is not specified, then
    /// linkat(2) can be used to link the temporary file into the filesystem, making it permanent
    /// , linking from /proc/self/{fd}.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use agrippa::fs::OpenOptions;
    ///
    /// let file = OpenOptions::new().write(true).temp_file(true).open("foo.txt").await?;
    /// ```
    pub fn temp_file(&mut self, temp_file: bool) -> &mut Self {
        self.temp_file = temp_file;
        self
    }

    /// Set user has read permission mode flag (0o400)
    pub fn user_read(&mut self, user_read: bool) -> &mut Self {
        self.user_read = user_read;
        self
    }

    /// Set user has write permission mode flag (0o200)
    pub fn user_write(&mut self, user_write: bool) -> &mut Self {
        self.user_write = user_write;
        self
    }

    /// Set user has execute permission mode flag (0o100)
    pub fn user_execute(&mut self, user_execute: bool) -> &mut Self {
        self.user_execute = user_execute;
        self
    }

    /// Set group has read permission mode flag (0o040)
    pub fn group_read(&mut self, group_read: bool) -> &mut Self {
        self.group_read = group_read;
        self
    }

    /// Set group has write permission mode flag (0o020)
    pub fn group_write(&mut self, group_write: bool) -> &mut Self {
        self.group_write = group_write;
        self
    }

    /// Set group has execute permission mode flag (0o010)
    pub fn group_execute(&mut self, group_execute: bool) -> &mut Self {
        self.group_execute = group_execute;
        self
    }

    /// Set other has read permission mode flag (0o004)
    pub fn other_read(&mut self, other_read: bool) -> &mut Self {
        self.other_read = other_read;
        self
    }

    /// Set other has write permission mode flag (0o002)
    pub fn other_write(&mut self, other_write: bool) -> &mut Self {
        self.other_write = other_write;
        self
    }

    /// Set other has execute permission mode flag (0o001)
    pub fn other_execute(&mut self, other_execute: bool) -> &mut Self {
        self.other_execute = other_execute;
        self
    }

    /// Set mode setuid flag. If this flag this file
    /// will be executed as the owning user istead of the calling
    /// user
    pub fn set_user_id(&mut self, set_user_id: bool) -> &mut Self {
        self.set_user_id = set_user_id;
        self
    }

    /// Set mode setgid flag. If this flag this file
    /// will be executed as the owning group istead of the group of
    /// calling user
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

    /// Opens a file at `path` with the options specified by `self`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use agrippa::fs::OpenOptions;
    ///
    /// let file = OpenOptions::new().read(true).open("foo.txt").await?;
    /// ```
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
    /// Attempts to open a file in read-only mode.
    ///
    /// See the [`OpenOptions::open`] method for more details.
    ///
    /// [`OpenOptions::open`]: struct.OpenOptions.html#method.open
    ///
    /// # Errors
    ///
    /// This function will return an error if `path` does not already exist.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use agrippa::fs::File;
    ///
    /// let mut file = File::open("foo.txt").await?;
    /// ```
    pub async fn open<P: AsRef<Path>>(path: P) -> Result<File> {
        OpenOptions::new().open(path).await
    }

    /// Opens a file in write-only mode.
    ///
    /// This function will create a file if it does not exist,
    /// and will truncate it if it does.
    ///
    /// See the [`OpenOptions::open`] function for more details.
    ///
    /// [`OpenOptions::open`]: struct.OpenOptions.html#method.open
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use agrippa::fs::File;
    ///
    /// let mut file = File::create("foo.txt").await?;
    /// ```
    pub async fn create<P: AsRef<Path>>(path: P) -> Result<File> {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .await
    }

    //TODO openat openat2 statx fadvice madvice

    /// Close the file
    ///
    /// # Note
    ///
    /// The file is closed synchronsly if it is dropped without calling this method
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
