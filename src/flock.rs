//! Copy paste of Cargo's src/util/flock.rs with modifications to not depend on
//! other Cargo stuff

use std::fs::{File, OpenOptions};
use std::path::{Display, Path, PathBuf};
use std::{fs, io};

use self::sys::*;

#[derive(PartialEq)]
enum State {
    Exclusive,
    Shared,
}

pub struct FileLock {
    file: File,
    path: PathBuf,
}

impl FileLock {
    pub fn parent(&self) -> &Path {
        self.path.parent().unwrap()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn remove_siblings(&self) -> io::Result<()> {
        let path = self.path();
        for entry in path.parent().unwrap().read_dir()? {
            let entry = entry?;
            if Some(&entry.file_name()[..]) == path.file_name() {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                fs::remove_dir_all(entry.path())?;
            } else {
                fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }
}

pub struct Filesystem {
    path: PathBuf,
    quiet: bool,
}

impl Filesystem {
    pub fn new(path: PathBuf, quiet: bool) -> Filesystem {
        Filesystem { path: path, quiet: quiet }
    }

    pub fn join<T>(&self, other: T) -> Filesystem
    where
        T: AsRef<Path>,
    {
        Filesystem::new(self.path.join(other), self.quiet)
    }

    pub fn open_ro<P>(&self, path: P, msg: &str) -> io::Result<FileLock>
    where
        P: AsRef<Path>,
    {
        self.open(
            path.as_ref(),
            OpenOptions::new().read(true),
            State::Shared,
            msg,
        )
    }

    pub fn open_rw<P>(&self, path: P, msg: &str) -> io::Result<FileLock>
    where
        P: AsRef<Path>,
    {
        self.open(
            path.as_ref(),
            OpenOptions::new().read(true).write(true).create(true),
            State::Exclusive,
            msg,
        )
    }

    fn open(
        &self,
        path: &Path,
        opts: &OpenOptions,
        state: State,
        msg: &str,
    ) -> io::Result<FileLock> {
        let path = self.path.join(path);

        let f = opts.open(&path).or_else(|e| {
            if e.kind() == io::ErrorKind::NotFound && state == State::Exclusive {
                create_dir_all(path.parent().unwrap())?;
                opts.open(&path)
            } else {
                Err(e)
            }
        })?;

        match state {
            State::Exclusive => {
                acquire(msg, &path, self.quiet, &|| try_lock_exclusive(&f), &|| {
                    lock_exclusive(&f)
                })?;
            }
            State::Shared => {
                acquire(msg, &path, self.quiet, &|| try_lock_shared(&f), &|| lock_shared(&f))?;
            }
        }

        Ok(FileLock {
            file: f,
            path: path,
        })
    }

    pub fn display(&self) -> Display {
        self.path.display()
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        unlock(&self.file).ok();
    }
}

fn acquire(
    msg: &str,
    path: &Path,
    quiet: bool,
    lock_try: &dyn Fn() -> io::Result<()>,
    lock_block: &dyn Fn() -> io::Result<()>,
) -> io::Result<()> {
    #[cfg(all(target_os = "linux", not(target_env = "musl")))]
    fn is_on_nfs_mount(path: &Path) -> bool {
        use std::ffi::CString;
        use std::mem;
        use std::os::unix::prelude::*;

        let path = match CString::new(path.as_os_str().as_bytes()) {
            Ok(path) => path,
            Err(_) => return false,
        };

        unsafe {
            let mut buf: ::libc::statfs = mem::zeroed();
            let r = ::libc::statfs(path.as_ptr(), &mut buf);

            r == 0 && buf.f_type as u32 == ::libc::NFS_SUPER_MAGIC as u32
        }
    }

    #[cfg(any(not(target_os = "linux"), target_env = "musl"))]
    fn is_on_nfs_mount(_path: &Path) -> bool {
        false
    }

    if is_on_nfs_mount(path) {
        return Ok(());
    }

    match lock_try() {
        Ok(()) => return Ok(()),

        // In addition to ignoring NFS which is commonly not working we also
        // just ignore locking on filesystems that look like they don't
        // implement file locking.
        Err(e) if error_unsupported(&e) => return Ok(()),

        Err(e) => {
            if !error_contended(&e) {
                return Err(e);
            }
        }
    }

    if !quiet {
        eprintln!(
            "{:>12} waiting for file lock on {}",
            "Blocking",
            msg
        )
    }

    lock_block()
}

#[cfg(unix)]
mod sys {
    use std::fs::File;
    use std::io::{Error, Result};
    use std::os::unix::io::AsRawFd;

    pub(super) fn lock_shared(file: &File) -> Result<()> {
        flock(file, libc::LOCK_SH)
    }

    pub(super) fn lock_exclusive(file: &File) -> Result<()> {
        flock(file, libc::LOCK_EX)
    }

    pub(super) fn try_lock_shared(file: &File) -> Result<()> {
        flock(file, libc::LOCK_SH | libc::LOCK_NB)
    }

    pub(super) fn try_lock_exclusive(file: &File) -> Result<()> {
        flock(file, libc::LOCK_EX | libc::LOCK_NB)
    }

    pub(super) fn unlock(file: &File) -> Result<()> {
        flock(file, libc::LOCK_UN)
    }

    pub(super) fn error_contended(err: &Error) -> bool {
        err.raw_os_error().map_or(false, |x| x == libc::EWOULDBLOCK)
    }

    pub(super) fn error_unsupported(err: &Error) -> bool {
        match err.raw_os_error() {
            Some(libc::ENOTSUP) => true,
            #[cfg(target_os = "linux")]
            Some(libc::ENOSYS) => true,
            _ => false,
        }
    }

    #[cfg(not(target_os = "solaris"))]
    fn flock(file: &File, flag: libc::c_int) -> Result<()> {
        let ret = unsafe { libc::flock(file.as_raw_fd(), flag) };
        if ret < 0 {
            Err(Error::last_os_error())
        } else {
            Ok(())
        }
    }

    #[cfg(target_os = "solaris")]
    fn flock(file: &File, flag: libc::c_int) -> Result<()> {
        // Solaris lacks flock(), so simply succeed with a no-op
        Ok(())
    }
}

#[cfg(windows)]
mod sys {
    use std::fs::File;
    use std::io::{Error, Result};
    use std::mem;
    use std::os::windows::io::AsRawHandle;

    use winapi::shared::minwindef::DWORD;
    use winapi::shared::winerror::{ERROR_INVALID_FUNCTION, ERROR_LOCK_VIOLATION};
    use winapi::um::fileapi::{LockFileEx, UnlockFile};
    use winapi::um::minwinbase::{LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY};

    pub(super) fn lock_shared(file: &File) -> Result<()> {
        lock_file(file, 0)
    }

    pub(super) fn lock_exclusive(file: &File) -> Result<()> {
        lock_file(file, LOCKFILE_EXCLUSIVE_LOCK)
    }

    pub(super) fn try_lock_shared(file: &File) -> Result<()> {
        lock_file(file, LOCKFILE_FAIL_IMMEDIATELY)
    }

    pub(super) fn try_lock_exclusive(file: &File) -> Result<()> {
        lock_file(file, LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY)
    }

    pub(super) fn error_contended(err: &Error) -> bool {
        err.raw_os_error()
            .map_or(false, |x| x == ERROR_LOCK_VIOLATION as i32)
    }

    pub(super) fn error_unsupported(err: &Error) -> bool {
        err.raw_os_error()
            .map_or(false, |x| x == ERROR_INVALID_FUNCTION as i32)
    }

    pub(super) fn unlock(file: &File) -> Result<()> {
        unsafe {
            let ret = UnlockFile(file.as_raw_handle(), 0, 0, !0, !0);
            if ret == 0 {
                Err(Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }

    fn lock_file(file: &File, flags: DWORD) -> Result<()> {
        unsafe {
            let mut overlapped = mem::zeroed();
            let ret = LockFileEx(file.as_raw_handle(), flags, 0, !0, !0, &mut overlapped);
            if ret == 0 {
                Err(Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }
}

fn create_dir_all(path: &Path) -> io::Result<()> {
    match create_dir(path) {
        Ok(()) => Ok(()),
        Err(e) => {
            if e.kind() == io::ErrorKind::NotFound {
                if let Some(p) = path.parent() {
                    return create_dir_all(p).and_then(|()| create_dir(path));
                }
            }
            Err(e)
        }
    }
}

fn create_dir(path: &Path) -> io::Result<()> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(ref e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}
