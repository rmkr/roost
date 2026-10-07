use super::{Entry, FileIdentity, absolute, valid_name, validate_token};
use crate::{Error, Result};
use rustix::fs::{self, AtFlags, Mode, OFlags};
use rustix::termios::{LocalModes, OptionalActions, SpecialCodeIndex};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

pub struct Directory {
    pub path: PathBuf,
    file: File,
}

fn native_error(operation: &str, path: &Path, error: rustix::io::Errno) -> Error {
    if error == rustix::io::Errno::LOOP || error == rustix::io::Errno::NOTDIR {
        Error::new(
            "unsafe_path",
            format!(
                "{operation}: redirected or non-directory path {}",
                path.display()
            ),
        )
    } else if error == rustix::io::Errno::NOENT {
        Error::new(
            "not_found",
            format!("{operation}: {} does not exist", path.display()),
        )
    } else if error == rustix::io::Errno::EXIST {
        Error::new(
            "collision",
            format!("{operation}: {} already exists", path.display()),
        )
    } else {
        Error::io(operation, path, error.into())
    }
}

fn identity(metadata: &std::fs::Metadata) -> FileIdentity {
    FileIdentity::Unix {
        device: metadata.dev().to_string(),
        inode: metadata.ino().to_string(),
    }
}

pub fn file_identity(file: &File) -> Result<FileIdentity> {
    Ok(identity(&file.metadata().map_err(|_| {
        Error::new("io", "Cannot inspect opened file")
    })?))
}

/// Opens a user-managed file (outside any managed directory) for reading,
/// refusing anything but a regular file. The open never blocks, so a FIFO swapped
/// in cannot hang it; a final symlink is followed only when `follow`.
pub(crate) fn open_user_file(path: &Path, follow: bool) -> Result<File> {
    let mut flags = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK | OFlags::NOCTTY;
    if !follow {
        flags |= OFlags::NOFOLLOW;
    }
    let file = fs::open(path, flags, Mode::empty())
        .map(File::from)
        .map_err(|e| native_error("open file", path, e))?;
    let metadata = file
        .metadata()
        .map_err(|e| Error::io("inspect opened file", path, e))?;
    if !metadata.is_file() {
        return Err(Error::new(
            "unsafe_path",
            format!("{} is not a regular file", path.display()),
        ));
    }
    Ok(file)
}

/// An opened file's permission bits (`0o777`).
pub(crate) fn file_mode(file: &File) -> Result<u32> {
    Ok(file
        .metadata()
        .map_err(|_| Error::new("io", "Cannot inspect opened file mode"))?
        .mode()
        & 0o777)
}

/// Sets an opened file's permission bits (`0o777`).
pub(crate) fn set_file_mode(file: &File, mode: u32) -> Result<()> {
    fs::fchmod(file, Mode::from_raw_mode((mode & 0o777) as _))
        .map_err(|e| Error::new("io", format!("Cannot set file mode: {e}")))
}

pub(crate) fn copy_mode(file: &File) -> Result<u32> {
    let metadata = file
        .metadata()
        .map_err(|_| Error::new("io", "Cannot inspect copy source mode"))?;
    Ok(0o600 | (metadata.mode() & 0o100))
}

fn check_file(file: &File, path: &Path, private: bool, directory: bool, write: bool) -> Result<()> {
    let metadata = file
        .metadata()
        .map_err(|e| Error::io("inspect opened object", path, e))?;
    if (directory && !metadata.is_dir())
        || (!directory && (!metadata.is_file() || metadata.nlink() != 1))
    {
        return Err(Error::new(
            "unsafe_path",
            format!("Unsafe object type or link count: {}", path.display()),
        ));
    }
    if private {
        let owner = rustix::process::geteuid().as_raw();
        let mode = metadata.mode();
        let required = if directory {
            0o700
        } else if write {
            0o600
        } else {
            0o400
        };
        if metadata.uid() != owner || mode & 0o077 != 0 || mode & required != required {
            return Err(Error::new(
                "ownership",
                format!("Unsafe owner or permissions: {}", path.display()),
            ));
        }
    }
    Ok(())
}

/// One passwd record: the facts the private-group rule needs.
#[derive(Clone, Debug)]
pub(crate) struct Account {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
}

/// The passwd and group databases as the namespace check sees them. Every
/// method answers `None` when the database cannot be read, which the check
/// treats as "not private" (fail closed).
pub(crate) trait AccountDatabase {
    /// The effective user's passwd record.
    fn current_user(&self) -> Option<Account>;
    /// The supplementary member names of group `gid`.
    fn group_members(&self, gid: u32) -> Option<Vec<String>>;
    /// Every account in the passwd database, completely enumerated.
    fn accounts(&self) -> Option<Vec<Account>>;
}

/// The effective user's private group: their primary group, provided it has
/// no supplementary members other than the user and no other account uses it
/// as its primary group. `None` when there is no such group or any fact is
/// unreadable.
pub(crate) fn private_group(db: &dyn AccountDatabase) -> Option<u32> {
    let user = db.current_user()?;
    let members = db.group_members(user.gid)?;
    if members.iter().any(|member| *member != user.name) {
        return None;
    }
    let accounts = db.accounts()?;
    if accounts
        .iter()
        .any(|other| other.gid == user.gid && other.uid != user.uid)
    {
        return None;
    }
    Some(user.gid)
}

/// Whether a directory with this owner, group and mode protects the names in
/// it. It must be owned by the effective user or root. A sticky directory is
/// accepted as before; otherwise it may not be world-writable, and it may be
/// group-writable only when its group is the user's private group (looked up
/// lazily, only for that case).
fn namespace_protected(
    owner: u32,
    gid: u32,
    mode: u32,
    euid: u32,
    private_gid: impl FnOnce() -> Option<u32>,
) -> bool {
    if owner != 0 && owner != euid {
        return false;
    }
    if mode & 0o1000 != 0 {
        return true;
    }
    if mode & 0o002 != 0 {
        return false;
    }
    mode & 0o020 == 0 || private_gid() == Some(gid)
}

/// Whether a directory belongs to the effective user and only the user can
/// write it: never world-writable or sticky, and group-writable only when its
/// group is the user's private group. Used for startup directories.
fn user_private_directory(
    owner: u32,
    gid: u32,
    mode: u32,
    euid: u32,
    private_gid: impl FnOnce() -> Option<u32>,
) -> bool {
    owner == euid && mode & 0o1002 == 0 && (mode & 0o020 == 0 || private_gid() == Some(gid))
}

/// The real passwd/group databases through the C library (NSS included).
struct SystemAccounts;

/// Runs a reentrant libc lookup, growing its string buffer on ERANGE.
fn with_buffer<T>(mut lookup: impl FnMut(&mut [u8]) -> std::result::Result<T, i32>) -> Option<T> {
    let mut buffer = vec![0_u8; 16384];
    loop {
        match lookup(&mut buffer) {
            Err(libc::ERANGE) if buffer.len() < 1048576 => {
                let size = buffer.len() * 2;
                buffer.resize(size, 0);
            }
            Err(_) => return None,
            Ok(value) => return Some(value),
        }
    }
}

fn passwd_account(record: &libc::passwd) -> Option<Account> {
    // pw_name points into the caller's buffer, which outlives this call.
    let name = unsafe { std::ffi::CStr::from_ptr(record.pw_name) };
    Some(Account {
        name: name.to_str().ok()?.to_owned(),
        uid: record.pw_uid,
        gid: record.pw_gid,
    })
}

/// Reads the effective user's passwd record through `getpwuid_r`.
fn current_passwd<T>(read: impl Fn(&libc::passwd) -> T) -> Option<T> {
    with_buffer(|buffer| {
        let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut result = std::ptr::null_mut();
        // getpwuid_r writes into caller-owned storage.
        let code = unsafe {
            libc::getpwuid_r(
                libc::geteuid(),
                record.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        if code != 0 {
            return Err(code);
        }
        if result.is_null() {
            return Err(libc::ENOENT);
        }
        Ok(read(unsafe { record.assume_init_ref() }))
    })
}

impl AccountDatabase for SystemAccounts {
    fn current_user(&self) -> Option<Account> {
        current_passwd(passwd_account).flatten()
    }

    fn group_members(&self, gid: u32) -> Option<Vec<String>> {
        with_buffer(|buffer| {
            let mut record = std::mem::MaybeUninit::<libc::group>::uninit();
            let mut result = std::ptr::null_mut();
            // getgrgid_r writes into caller-owned storage.
            let code = unsafe {
                libc::getgrgid_r(
                    gid,
                    record.as_mut_ptr(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                    &mut result,
                )
            };
            if code != 0 {
                return Err(code);
            }
            if result.is_null() {
                return Err(libc::ENOENT);
            }
            let record = unsafe { record.assume_init_ref() };
            let mut names = Vec::new();
            let mut cursor = record.gr_mem;
            // gr_mem is a NULL-terminated array of C strings inside `buffer`.
            while !cursor.is_null() && !unsafe { *cursor }.is_null() {
                let name = unsafe { std::ffi::CStr::from_ptr(*cursor) };
                names.push(name.to_str().map_err(|_| libc::EINVAL)?.to_owned());
                cursor = unsafe { cursor.add(1) };
            }
            Ok(names)
        })
    }

    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    fn accounts(&self) -> Option<Vec<Account>> {
        // setpwent/getpwent_r share process-wide iteration state.
        static ENUMERATION: Mutex<()> = Mutex::new(());
        let _guard = ENUMERATION.lock().ok()?;
        unsafe { libc::setpwent() };
        let mut buffer = vec![0_u8; 16384];
        let mut accounts = Vec::new();
        let complete = loop {
            let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
            let mut result = std::ptr::null_mut();
            // getpwent_r writes into caller-owned storage.
            let code = unsafe {
                libc::getpwent_r(
                    record.as_mut_ptr(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                    &mut result,
                )
            };
            if code == libc::ENOENT {
                break true;
            }
            if code == libc::ERANGE && buffer.len() < 1048576 {
                let size = buffer.len() * 2;
                buffer.resize(size, 0);
                continue;
            }
            if code != 0 || result.is_null() {
                break false;
            }
            match passwd_account(unsafe { record.assume_init_ref() }) {
                Some(account) => accounts.push(account),
                None => break false,
            }
        };
        unsafe { libc::endpwent() };
        complete.then_some(accounts)
    }

    /// Without a reentrant enumeration the database is treated as unreadable.
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    fn accounts(&self) -> Option<Vec<Account>> {
        None
    }
}

/// The effective user's private group from the system databases, computed
/// once per process.
fn system_private_group() -> Option<u32> {
    static PRIVATE: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
    *PRIVATE.get_or_init(|| private_group(&SystemAccounts))
}

fn check_namespace(file: &File, path: &Path) -> Result<()> {
    let metadata = file
        .metadata()
        .map_err(|e| Error::io("inspect parent namespace", path, e))?;
    let uid = rustix::process::geteuid().as_raw();
    if !namespace_protected(
        metadata.uid(),
        metadata.gid(),
        metadata.mode(),
        uid,
        system_private_group,
    ) {
        return Err(Error::new(
            "ownership",
            format!("Parent namespace is not protected: {}", path.display()),
        ));
    }
    Ok(())
}

impl Directory {
    pub fn open(path: &Path, private: bool) -> Result<Self> {
        Self::open_checked(path, private, private)
    }

    fn open_checked(path: &Path, private: bool, protected_parents: bool) -> Result<Self> {
        let path = absolute(path)?;
        let initial = fs::open(
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| native_error("open filesystem root", Path::new("/"), e))?;
        let mut file = File::from(initial);
        let mut walked = PathBuf::from("/");
        for component in path.components() {
            if let Component::Normal(name) = component {
                if protected_parents {
                    check_namespace(&file, &walked)?;
                }
                walked.push(name);
                file = fs::openat(
                    &file,
                    name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map(File::from)
                .map_err(|e| native_error("open directory", &walked, e))?;
            }
        }
        check_file(&file, &path, private, true, true)?;
        Ok(Self { path, file })
    }

    pub(crate) fn verify_namespace(&self) -> Result<()> {
        let current = Self::open_checked(&self.path, false, true)?;
        check_namespace(&current.file, &current.path)?;
        if current.identity()? != self.identity()? {
            return Err(Error::new("ownership", "Directory namespace changed"));
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn create(path: &Path) -> Result<Self> {
        let path = absolute(path)?;
        let parent = path
            .parent()
            .ok_or_else(|| Error::new("unsafe_path", "Cannot create filesystem root"))?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::new("unsafe_path", "Invalid directory name"))?;
        Self::open(parent, false)?.create_dir(name)
    }

    pub fn identity(&self) -> Result<FileIdentity> {
        file_identity(&self.file)
    }

    /// Tightens a group- or world-writable directory that the caller owns to 0700
    /// through this already-open handle (fchmod, never a path). Only for directories
    /// inside Roost-owned profiles, such as `skills/` that Claude created under umask
    /// 002; borrowed data is never passed here. Another owner is ownership.
    pub(crate) fn restrict_to_owner(&self) -> Result<()> {
        let metadata = self
            .file
            .metadata()
            .map_err(|e| Error::io("inspect directory", &self.path, e))?;
        if metadata.uid() != rustix::process::geteuid().as_raw() {
            return Err(Error::new(
                "ownership",
                format!("Not owned by the current user: {}", self.path.display()),
            ));
        }
        if metadata.mode() & 0o022 != 0 {
            fs::fchmod(&self.file, Mode::from_raw_mode(0o700))
                .map_err(|e| native_error("restrict directory", &self.path, e))?;
        }
        Ok(())
    }

    pub fn entries(&self) -> Result<Vec<String>> {
        let directory = fs::Dir::read_from(&self.file)
            .map_err(|e| native_error("read directory", &self.path, e))?;
        let mut names = Vec::new();
        for entry in directory {
            let entry = entry.map_err(|e| native_error("read directory entry", &self.path, e))?;
            let name = entry
                .file_name()
                .to_str()
                .map_err(|_| Error::new("unsafe_path", "Directory entry is not Unicode"))?;
            if name == "." || name == ".." {
                continue;
            }
            valid_name(name)?;
            names.push(name.to_owned());
        }
        names.sort();
        Ok(names)
    }

    pub fn entry(&self, name: &str) -> Result<Option<Entry>> {
        valid_name(name)?;
        let metadata = match fs::statat(&self.file, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(value) => value,
            Err(error) if error == rustix::io::Errno::NOENT => return Ok(None),
            Err(error) => return Err(native_error("inspect entry", &self.path.join(name), error)),
        };
        let kind = fs::FileType::from_raw_mode(metadata.st_mode);
        Ok(Some(Entry {
            identity: FileIdentity::Unix {
                device: metadata.st_dev.to_string(),
                inode: metadata.st_ino.to_string(),
            },
            is_dir: kind == fs::FileType::Directory,
            is_file: kind == fs::FileType::RegularFile,
            is_link: kind == fs::FileType::Symlink,
            nlink: metadata.st_nlink as u64,
        }))
    }

    pub fn child(&self, name: &str, private: bool) -> Result<Self> {
        valid_name(name)?;
        let path = self.path.join(name);
        let file = fs::openat(
            &self.file,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map(File::from)
        .map_err(|e| native_error("open child directory", &path, e))?;
        check_file(&file, &path, private, true, true)?;
        Ok(Self { path, file })
    }

    pub fn create_dir(&self, name: &str) -> Result<Self> {
        valid_name(name)?;
        self.verify_namespace()?;
        fs::mkdirat(&self.file, name, Mode::from_raw_mode(0o700))
            .map_err(|e| native_error("create private directory", &self.path.join(name), e))?;
        self.child(name, true)
    }

    /// Creates `name` as a symbolic link to the absolute `target`, relative to this
    /// verified handle. Never replaces an existing entry; returns the link's own identity.
    pub fn symlink(&self, target: &Path, name: &str) -> Result<FileIdentity> {
        valid_name(name)?;
        let text = target
            .to_str()
            .ok_or_else(|| Error::new("unsafe_path", "Link target is not Unicode"))?;
        if !target.is_absolute() || text.contains(['\r', '\n', '\0']) {
            return Err(Error::new(
                "unsafe_path",
                "Link target must be an absolute path without line breaks",
            ));
        }
        self.verify_namespace()?;
        let path = self.path.join(name);
        fs::symlinkat(target, &self.file, name)
            .map_err(|e| native_error("create link", &path, e))?;
        match self.entry(name)? {
            Some(entry) if entry.is_link => Ok(entry.identity),
            _ => Err(Error::new(
                "unsafe_path",
                format!("Link changed after creation: {}", path.display()),
            )),
        }
    }

    /// Reads the text of the link `name` without following it. `None` when absent;
    /// any other object type is unsafe_path.
    pub fn read_link(&self, name: &str) -> Result<Option<PathBuf>> {
        valid_name(name)?;
        let path = self.path.join(name);
        match fs::readlinkat(&self.file, name, Vec::new()) {
            Ok(text) => {
                use std::os::unix::ffi::OsStringExt;
                Ok(Some(PathBuf::from(std::ffi::OsString::from_vec(
                    text.into_bytes(),
                ))))
            }
            Err(error) if error == rustix::io::Errno::NOENT => Ok(None),
            Err(error) if error == rustix::io::Errno::INVAL => Err(Error::new(
                "unsafe_path",
                format!("Not a link: {}", path.display()),
            )),
            Err(error) => Err(native_error("read link", &path, error)),
        }
    }

    pub fn open_file(&self, name: &str, private: bool, write: bool) -> Result<File> {
        valid_name(name)?;
        let path = self.path.join(name);
        let access = if write { OFlags::RDWR } else { OFlags::RDONLY };
        let file = fs::openat(
            &self.file,
            name,
            access | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map(File::from)
        .map_err(|e| native_error("open regular file", &path, e))?;
        check_file(&file, &path, private, false, write)?;
        Ok(file)
    }

    pub fn read(&self, name: &str, private: bool, limit: usize) -> Result<Option<Vec<u8>>> {
        if self.entry(name)?.is_none() {
            return Ok(None);
        }
        let file = self.open_file(name, private, false)?;
        let mut bytes = Vec::new();
        file.take((limit as u64).saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|e| Error::io("read file", &self.path.join(name), e))?;
        if bytes.len() > limit {
            return Err(Error::new(
                "unsafe_path",
                format!(
                    "File exceeds supported size: {}",
                    self.path.join(name).display()
                ),
            ));
        }
        Ok(Some(bytes))
    }

    pub(crate) fn create_file(&self, name: &str, mode: u32) -> Result<File> {
        valid_name(name)?;
        if mode & !0o700 != 0 || mode & 0o600 != 0o600 {
            return Err(Error::new(
                "ownership",
                "New file mode must permit private owner read/write",
            ));
        }
        let path = self.path.join(name);
        let file = fs::openat(
            &self.file,
            name,
            OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(mode as _),
        )
        .map(File::from)
        .map_err(|e| native_error("create private file", &path, e))?;
        check_file(&file, &path, true, false, true)?;
        if mode & 0o100 != 0
            && file
                .metadata()
                .map_err(|e| Error::io("inspect launcher mode", &path, e))?
                .mode()
                & 0o100
                == 0
        {
            return Err(Error::new(
                "ownership",
                "Launcher owner execute permission was masked",
            ));
        }
        Ok(file)
    }

    /// Never checks cancellation: a journal write must finish once its staged
    /// object exists; callers honor cancellation at step boundaries.
    pub fn write_new(&self, name: &str, bytes: &[u8], mode: u32) -> Result<FileIdentity> {
        let mut file = self.create_file(name, mode)?;
        file.write_all(bytes)
            .map_err(|e| Error::io("write private file", &self.path.join(name), e))?;
        file.sync_all()
            .map_err(|e| Error::io("flush private file", &self.path.join(name), e))?;
        file_identity(&file)
    }

    pub fn rename(&self, name: &str, destination: &Directory, new_name: &str) -> Result<()> {
        valid_name(name)?;
        valid_name(new_name)?;
        fs::renameat(&self.file, name, &destination.file, new_name)
            .map_err(|e| native_error("rename entry", &destination.path.join(new_name), e))
    }

    pub fn remove(&self, name: &str, directory: bool) -> Result<()> {
        valid_name(name)?;
        fs::unlinkat(
            &self.file,
            name,
            if directory {
                AtFlags::REMOVEDIR
            } else {
                AtFlags::empty()
            },
        )
        .map_err(|e| native_error("remove entry", &self.path.join(name), e))
    }

    pub fn sync(&self) -> Result<()> {
        self.file
            .sync_all()
            .map_err(|e| Error::io("flush directory", &self.path, e))
    }

    pub fn purge_children(&self, retain: &str) -> Result<()> {
        valid_name(retain)?;
        for name in self.entries()? {
            if name == retain {
                continue;
            }
            if cancelled() {
                return Err(Error::cancelled());
            }
            if Self::open(&self.path, true)?.identity()? != self.identity()? {
                return Err(Error::new(
                    "ownership",
                    "Purge parent changed; partial deletion may remain",
                ));
            }
            let Some(entry) = self.entry(&name)? else {
                continue;
            };
            if entry.is_dir {
                let child = self.child(&name, false)?;
                if child.identity()? != entry.identity {
                    return Err(Error::new(
                        "ownership",
                        "Purge directory changed; partial deletion may remain",
                    ));
                }
                if self.entry(&name)?.map(|entry| entry.identity) != Some(entry.identity) {
                    return Err(Error::new(
                        "ownership",
                        "Purge directory replaced; partial deletion may remain",
                    ));
                }
                // Std supplies contained-link race protection. Parent revalidation
                // above supplies ownership within the agreed quiet-writer boundary.
                std::fs::remove_dir_all(&child.path).map_err(|e| {
                    Error::io(
                        "purge child directory; partial deletion may remain",
                        &child.path,
                        e,
                    )
                })?;
            } else {
                self.remove(&name, false)?;
            }
        }
        self.sync()
    }
}

pub fn home() -> Result<PathBuf> {
    if let Some(value) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        return absolute(Path::new(&value));
    }
    // pw_dir points into the lookup's buffer, which outlives the closure.
    let dir = current_passwd(|r| {
        unsafe { std::ffi::CStr::from_ptr(r.pw_dir) }
            .to_str()
            .map(str::to_owned)
    })
    .ok_or_else(|| Error::new("io", "Cannot determine user home directory"))?
    .map_err(|_| Error::new("unsafe_path", "User home is not Unicode"))?;
    absolute(Path::new(&dir))
}

static CANCELLED: AtomicBool = AtomicBool::new(false);
static SIGNALS: Mutex<Vec<(libc::c_int, libc::sigaction)>> = Mutex::new(Vec::new());
extern "C" fn cancel_handler(_: libc::c_int) {
    CANCELLED.store(true, Ordering::Relaxed);
}
pub fn cancelled() -> bool {
    CANCELLED.load(Ordering::Relaxed)
}

pub fn install_cancel_handler() -> Result<()> {
    let mut previous = SIGNALS
        .lock()
        .map_err(|_| Error::new("io", "Signal-state lock failed"))?;
    if !previous.is_empty() {
        return Ok(());
    }
    for signal in [libc::SIGINT, libc::SIGTERM] {
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = cancel_handler as *const () as usize;
        unsafe {
            libc::sigemptyset(&mut action.sa_mask);
        }
        let mut old = std::mem::MaybeUninit::uninit();
        if unsafe { libc::sigaction(signal, &action, old.as_mut_ptr()) } != 0 {
            for (signal, old) in previous.drain(..) {
                unsafe {
                    libc::sigaction(signal, &old, std::ptr::null_mut());
                }
            }
            return Err(Error::new("io", "Cannot install cancellation handler"));
        }
        previous.push((signal, unsafe { old.assume_init() }));
    }
    Ok(())
}

pub fn restore_for_exec() -> Result<()> {
    if cancelled() {
        return Err(Error::cancelled());
    }
    let mut previous = SIGNALS
        .lock()
        .map_err(|_| Error::new("io", "Signal-state lock failed"))?;
    for (signal, old) in previous.drain(..) {
        if unsafe { libc::sigaction(signal, &old, std::ptr::null_mut()) } != 0 {
            return Err(Error::new("io", "Cannot restore signal handler"));
        }
    }
    Ok(())
}

/// The terminal's original mode, restored on drop.
struct TerminalMode<'a> {
    fd: BorrowedFd<'a>,
    original: rustix::termios::Termios,
}
impl Drop for TerminalMode<'_> {
    fn drop(&mut self) {
        let _ = rustix::termios::tcsetattr(self.fd, OptionalActions::Now, &self.original);
    }
}

/// Switches `fd` to byte-at-a-time input without echo (and without `extra` local
/// modes), keeping ISIG so Ctrl-C still raises SIGINT for the cancel handler.
/// `failure` is the error message when the mode cannot be set.
fn raw_mode<'a>(
    fd: BorrowedFd<'a>,
    extra: LocalModes,
    failure: &'static str,
) -> Result<TerminalMode<'a>> {
    let original = rustix::termios::tcgetattr(fd)
        .map_err(|_| Error::new("io", "Cannot inspect terminal mode"))?;
    let mut raw = original.clone();
    raw.local_modes &= !(LocalModes::ECHO | LocalModes::ICANON | extra);
    raw.special_codes[SpecialCodeIndex::VMIN] = 1;
    raw.special_codes[SpecialCodeIndex::VTIME] = 0;
    rustix::termios::tcsetattr(fd, OptionalActions::Now, &raw)
        .map_err(|_| Error::new("io", failure))?;
    Ok(TerminalMode { fd, original })
}

fn terminal() -> Result<File> {
    std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty")
        .map_err(|_| Error::new("usage", "An interactive terminal is required; use --stdin for tokens or --yes for confirmation"))
}

fn bounded_input(descriptor: libc::c_int, terminal: bool, limit: usize) -> Result<Vec<u8>> {
    // Oversized terminal input discards the rest of the pending line, so the
    // shell does not run it after Roost exits.
    let overflow = || {
        if terminal {
            let fd = unsafe { BorrowedFd::borrow_raw(descriptor) };
            let _ = rustix::termios::tcflush(fd, rustix::termios::QueueSelector::IFlush);
        }
        Error::new("invalid_token", "Input exceeds supported size")
    };
    let mut bytes = Vec::new();
    loop {
        if cancelled() {
            return Err(Error::cancelled());
        }
        let Some(chunk) = read_ready(descriptor, 100, limit + 1 - bytes.len())? else {
            continue;
        };
        if chunk.is_empty() {
            break;
        }
        if terminal {
            for byte in &chunk {
                if *byte == 4 {
                    return Ok(bytes);
                }
                if *byte == b'\n' || *byte == b'\r' {
                    bytes.push(*byte);
                    if bytes.len() > limit {
                        return Err(overflow());
                    }
                    return Ok(bytes);
                }
                if *byte == 8 || *byte == 127 {
                    bytes.pop();
                } else {
                    bytes.push(*byte);
                }
            }
        } else {
            bytes.extend_from_slice(&chunk);
        }
        if bytes.len() > limit {
            return Err(overflow());
        }
        if terminal && bytes.contains(&b'\n') {
            break;
        }
    }
    if cancelled() {
        return Err(Error::cancelled());
    }
    Ok(bytes)
}

pub fn token_input(stdin: bool) -> Result<String> {
    if stdin {
        return validate_token(&bounded_input(libc::STDIN_FILENO, false, 65536)?);
    }
    let tty = terminal()?;
    // Noncanonical input avoids the kernel's much smaller canonical-line ceiling.
    let guard = raw_mode(
        tty.as_fd(),
        LocalModes::ECHONL,
        "Cannot disable terminal echo",
    )?;
    (&tty)
        .write_all(b"Token: ")
        .map_err(|_| Error::new("io", "Cannot write terminal prompt"))?;
    let bytes = bounded_input(tty.as_raw_fd(), true, 65536);
    drop(guard);
    let _ = (&tty).write_all(b"\n");
    let bytes = bytes?;
    if bytes.is_empty() {
        return Err(Error::cancelled());
    }
    validate_token(&bytes)
}

/// Column count of the terminal on stdout; None when stdout is not a terminal or
/// reports no size.
pub fn stdout_width() -> Option<usize> {
    terminal_width(std::io::stdout())
}

/// Column count of the terminal on stderr, where the pickers draw, less their
/// two-column `> ` prefix; None like `stdout_width`.
pub fn picker_width() -> Option<usize> {
    terminal_width(std::io::stderr())
        .map(|width| width.saturating_sub(2))
        .filter(|&width| width > 0)
}

fn terminal_width(stream: impl std::io::IsTerminal + std::os::fd::AsFd) -> Option<usize> {
    if !stream.is_terminal() {
        return None;
    }
    rustix::termios::tcgetwinsize(&stream)
        .ok()
        .map(|size| usize::from(size.ws_col))
        .filter(|&width| width > 0)
}

pub fn confirm(scope: &str, yes: bool) -> Result<()> {
    eprintln!("{scope}\nStop writers for this scope before continuing.");
    if cancelled() {
        return Err(Error::cancelled());
    }
    if yes {
        return Ok(());
    }
    let mut tty = terminal()?;
    tty.write_all(b"Continue? [y/N] ")
        .map_err(|_| Error::new("io", "Cannot write confirmation prompt"))?;
    // An oversized answer is not yes.
    let bytes = match bounded_input(tty.as_raw_fd(), true, 128) {
        Err(error) if error.code == "invalid_token" => return Err(Error::cancelled()),
        bytes => bytes?,
    };
    let response = std::str::from_utf8(&bytes).unwrap_or("").trim();
    if response.eq_ignore_ascii_case("y") || response.eq_ignore_ascii_case("yes") {
        Ok(())
    } else {
        Err(Error::cancelled())
    }
}

/// Shows `prompt` on the terminal and reads one line (at most 4 KiB) from it.
/// No terminal is usage; Ctrl-C or EOF before any input is cancelled.
pub fn prompt_line(prompt: &str) -> Result<String> {
    let mut tty = terminal()?;
    tty.write_all(prompt.as_bytes())
        .map_err(|_| Error::new("io", "Cannot write terminal prompt"))?;
    let bytes = bounded_input(tty.as_raw_fd(), true, 4096)?;
    if bytes.is_empty() {
        return Err(Error::cancelled());
    }
    String::from_utf8(bytes)
        .map(|text| text.trim().to_owned())
        .map_err(|_| Error::new("usage", "Terminal input is not valid UTF-8"))
}

/// One key read by the picker.
#[derive(Debug, PartialEq, Eq)]
enum Key {
    Up,
    Down,
    Choose,
    Cancel,
    /// An ASCII letter or digit with no built-in meaning.
    Letter(char),
    Other,
}

/// Decodes one keypress from the front of `bytes`, returning it and its length.
/// A lone ESC at the end of the buffer is ambiguous; `None` asks for more input.
fn decode_key(bytes: &[u8]) -> Option<(Key, usize)> {
    Some(match bytes {
        [] => return None,
        [0x1b] => return None,
        [0x1b, b'[' | b'O', b'A', ..] => (Key::Up, 3),
        [0x1b, b'[' | b'O', b'B', ..] => (Key::Down, 3),
        [0x1b, b'[' | b'O'] => return None,
        [0x1b, b'[' | b'O', rest @ ..] => {
            // Skip any other CSI/SS3 sequence through its final byte.
            let end = rest
                .iter()
                .position(|b| (0x40..=0x7e).contains(b))
                .map_or(bytes.len(), |i| i + 3);
            (Key::Other, end)
        }
        [0x1b, ..] => (Key::Cancel, 1),
        [b'k', ..] => (Key::Up, 1),
        [b'j', ..] => (Key::Down, 1),
        [b'\r' | b'\n', ..] => (Key::Choose, 1),
        [b'q' | 3 | 4, ..] => (Key::Cancel, 1),
        [b, ..] if b.is_ascii_alphanumeric() => (Key::Letter(char::from(*b)), 1),
        [_, ..] => (Key::Other, 1),
    })
}

/// Waits up to `timeout` milliseconds for at most `max` (up to 512) bytes of input
/// on `descriptor`; `None` on timeout or interruption, an empty vector at EOF.
fn read_ready(
    descriptor: libc::c_int,
    timeout: libc::c_int,
    max: usize,
) -> Result<Option<Vec<u8>>> {
    let mut poll = libc::pollfd {
        fd: descriptor,
        events: libc::POLLIN,
        revents: 0,
    };
    let ready = unsafe { libc::poll(&mut poll, 1, timeout) };
    if ready < 0 {
        if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            return Ok(None);
        }
        return Err(Error::new("io", "Cannot wait for input"));
    }
    if ready == 0 {
        return Ok(None);
    }
    let mut buffer = [0_u8; 512];
    let count = unsafe { libc::read(descriptor, buffer.as_mut_ptr().cast(), max.min(512)) };
    if count < 0 {
        if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            return Ok(None);
        }
        return Err(Error::new("io", "Cannot read input"));
    }
    Ok(Some(buffer[..count as usize].to_vec()))
}

/// One picker frame: the prompt, the header and every row, the `current` row in
/// reverse video. Rows may carry `ls` styles; reverse video is re-applied after
/// each embedded reset so the highlight spans the whole row. Later frames first
/// move the cursor back over the previous one.
fn picker_frame(
    title: &str,
    hint: &str,
    header: &str,
    rows: &[String],
    current: usize,
    first: bool,
) -> String {
    let mut text = String::new();
    if !first {
        text.push_str(&format!("\x1b[{}A", rows.len() + 2));
    }
    text.push_str(&format!("\r\x1b[2K{title} ({hint})\n"));
    text.push_str(&format!("\r\x1b[2K  {header}\n"));
    for (index, row) in rows.iter().enumerate() {
        if index == current {
            let row = row.replace("\x1b[0m", "\x1b[0m\x1b[7m");
            text.push_str(&format!("\r\x1b[2K\x1b[7m> {row}\x1b[0m\n"));
        } else {
            text.push_str(&format!("\r\x1b[2K  {row}\n"));
        }
    }
    text
}

/// Single-choice picker on stderr reading keys from the terminal on stdin, with
/// `hint` after the title. Up/Down or k/j move, Enter chooses; Ctrl-C, Esc, `q` or
/// EOF cancel (exit 130). Pressing one of the `actions` keys ends the picker like a
/// choice, with the highlighted row, so the caller can act and show it again. The
/// terminal mode is restored and the list erased on every exit.
pub fn pick_with(
    title: &str,
    hint: &str,
    header: &str,
    rows: &[String],
    initial: usize,
    actions: &[char],
) -> Result<super::Picked> {
    if rows.is_empty() {
        return Err(Error::new("not_found", "Nothing to choose from"));
    }
    if cancelled() {
        return Err(Error::cancelled());
    }
    let descriptor = libc::STDIN_FILENO;
    let stdin = std::io::stdin();
    let guard = raw_mode(
        stdin.as_fd(),
        LocalModes::empty(),
        "Cannot switch terminal mode",
    )?;
    let mut out = std::io::stderr();
    let height = rows.len() + 2;
    let draw =
        |current: usize, first: bool| picker_frame(title, hint, header, rows, current, first);
    let mut current = initial.min(rows.len() - 1);
    let _ = write!(out, "\x1b[?25l{}", draw(current, true));
    let _ = out.flush();
    let mut pending: Vec<u8> = Vec::new();
    let result = loop {
        if cancelled() {
            break Err(Error::cancelled());
        }
        // A lone ESC waits briefly for the rest of an arrow-key sequence.
        let timeout = if pending.is_empty() { 100 } else { 50 };
        match read_ready(descriptor, timeout, 64) {
            Err(e) => break Err(e),
            Ok(Some(bytes)) if bytes.is_empty() => break Err(Error::cancelled()),
            Ok(Some(bytes)) => pending.extend(bytes),
            Ok(None) if pending.is_empty() => continue,
            // Timed out after a partial escape: a bare Esc.
            Ok(None) => break Err(Error::cancelled()),
        }
        let mut chosen = None;
        while let Some((key, used)) = decode_key(&pending) {
            pending.drain(..used);
            match key {
                Key::Up => current = current.saturating_sub(1),
                Key::Down => current = (current + 1).min(rows.len() - 1),
                Key::Choose => chosen = Some(Ok(super::Picked::Chosen(current))),
                Key::Cancel => chosen = Some(Err(Error::cancelled())),
                Key::Letter(letter) if actions.contains(&letter) => {
                    chosen = Some(Ok(super::Picked::Action(current)));
                }
                Key::Letter(_) | Key::Other => (),
            }
            if chosen.is_some() {
                break;
            }
        }
        if let Some(result) = chosen {
            break result;
        }
        let _ = write!(out, "{}", draw(current, false));
        let _ = out.flush();
    };
    let _ = write!(out, "\x1b[{height}A\r\x1b[J\x1b[?25h");
    let _ = out.flush();
    drop(guard);
    if cancelled() {
        return Err(Error::cancelled());
    }
    result
}

mod path_setup;
pub use path_setup::setup_path;

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn user_private_directory_accepts_only_the_private_group_as_writer() {
        let private = || Some(1003);
        assert!(user_private_directory(1001, 1003, 0o40755, 1001, private));
        assert!(user_private_directory(1001, 1003, 0o40775, 1001, private));
        assert!(!user_private_directory(1001, 1004, 0o40775, 1001, private));
        assert!(!user_private_directory(1001, 1003, 0o40777, 1001, private));
        assert!(!user_private_directory(1001, 1003, 0o41777, 1001, private));
        assert!(!user_private_directory(0, 1003, 0o40755, 1001, private));
        assert!(!user_private_directory(1001, 1003, 0o40775, 1001, || None));
    }

    /// Injected passwd/group facts for the private-group namespace rule.
    struct FakeAccounts {
        current: Option<Account>,
        members: Option<Vec<&'static str>>,
        accounts: Option<Vec<Account>>,
    }
    fn account(name: &str, uid: u32, gid: u32) -> Account {
        Account {
            name: name.to_owned(),
            uid,
            gid,
        }
    }
    impl AccountDatabase for FakeAccounts {
        fn current_user(&self) -> Option<Account> {
            self.current.clone()
        }
        fn group_members(&self, gid: u32) -> Option<Vec<String>> {
            assert_eq!(gid, 1000);
            self.members
                .as_ref()
                .map(|m| m.iter().map(|n| (*n).to_owned()).collect())
        }
        fn accounts(&self) -> Option<Vec<Account>> {
            self.accounts.clone()
        }
    }
    fn private_ubuntu_user() -> FakeAccounts {
        FakeAccounts {
            current: Some(account("alice", 1000, 1000)),
            members: Some(vec![]),
            accounts: Some(vec![
                account("root", 0, 0),
                account("alice", 1000, 1000),
                account("bob", 1001, 1001),
            ]),
        }
    }

    #[test]
    fn group_writable_namespace_owned_by_user_with_private_group_is_protected() {
        let db = private_ubuntu_user();
        assert!(namespace_protected(1000, 1000, 0o40775, 1000, || {
            private_group(&db)
        }));
    }

    #[test]
    fn private_group_may_list_only_the_user_as_a_supplementary_member() {
        let mut db = private_ubuntu_user();
        db.members = Some(vec!["alice"]);
        assert_eq!(private_group(&db), Some(1000));
        db.members = Some(vec!["alice", "bob"]);
        assert_eq!(private_group(&db), None);
        assert!(!namespace_protected(1000, 1000, 0o40775, 1000, || {
            private_group(&db)
        }));
    }

    #[test]
    fn group_shared_as_another_accounts_primary_group_is_not_private() {
        let mut db = private_ubuntu_user();
        db.accounts
            .as_mut()
            .unwrap()
            .push(account("carol", 1002, 1000));
        assert_eq!(private_group(&db), None);
        // A second name for the user's own uid is still the user.
        let mut db = private_ubuntu_user();
        db.accounts
            .as_mut()
            .unwrap()
            .push(account("alice-alias", 1000, 1000));
        assert_eq!(private_group(&db), Some(1000));
    }

    #[test]
    fn unreadable_account_databases_fail_closed() {
        let mut db = private_ubuntu_user();
        db.accounts = None;
        assert_eq!(private_group(&db), None);
        let mut db = private_ubuntu_user();
        db.members = None;
        assert_eq!(private_group(&db), None);
        let mut db = private_ubuntu_user();
        db.current = None;
        assert_eq!(private_group(&db), None);
    }

    #[test]
    fn group_writable_namespace_needs_the_users_private_group() {
        // Group 27 (e.g. sudo) is not the user's private group 1000.
        assert!(!namespace_protected(1000, 27, 0o40775, 1000, || Some(1000)));
        assert!(!namespace_protected(1000, 1000, 0o40775, 1000, || None));
        // Root-owned directories follow the same rule.
        assert!(namespace_protected(0, 1000, 0o40770, 1000, || Some(1000)));
    }

    #[test]
    fn system_account_database_reads_the_current_user() {
        // Host-independent: whatever the host's groups, the effective user
        // resolves and appears in the enumerated passwd database.
        let user = SystemAccounts.current_user().expect("current user");
        assert_eq!(user.uid, rustix::process::geteuid().as_raw());
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        assert!(
            SystemAccounts
                .accounts()
                .expect("passwd enumeration")
                .iter()
                .any(|a| a.uid == user.uid)
        );
    }

    #[test]
    fn namespace_rules_outside_group_write_are_unchanged() {
        let never = || -> Option<u32> { panic!("account databases consulted") };
        assert!(namespace_protected(1000, 1000, 0o40755, 1000, never));
        assert!(namespace_protected(0, 0, 0o40755, 1000, never));
        assert!(namespace_protected(0, 0, 0o41777, 1000, never));
        assert!(!namespace_protected(1001, 1000, 0o40700, 1000, never));
        assert!(!namespace_protected(1001, 1000, 0o40775, 1000, never));
        // World-writable without sticky stays rejected, private group or not.
        assert!(!namespace_protected(1000, 1000, 0o40777, 1000, never));
        assert!(!namespace_protected(1000, 1000, 0o40757, 1000, never));
    }
    #[test]
    fn picker_highlight_survives_styled_cells_and_plain_rows_stay_plain() {
        let rows = [
            "\x1b[1mHome\x1b[0m  \x1b[32m✓\x1b[0m".to_owned(),
            "\x1b[1mWork\x1b[0m  –".to_owned(),
        ];
        let frame = picker_frame("Choose", "↑/↓", "Profile", &rows, 1, true);
        let lines: Vec<&str> = frame.split('\n').collect();
        assert_eq!(lines[2], "\r\x1b[2K  \x1b[1mHome\x1b[0m  \x1b[32m✓\x1b[0m");
        assert_eq!(
            lines[3],
            "\r\x1b[2K\x1b[7m> \x1b[1mWork\x1b[0m\x1b[7m  –\x1b[0m"
        );
        assert!(picker_frame("Choose", "↑/↓", "Profile", &rows, 0, false).starts_with("\x1b[4A"));
    }
    #[test]
    fn copy_excludes_sensitive_and_linked_objects_at_depth() {
        let path = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("roost-copy-{}", super::super::random_id().unwrap()));
        let fixture = Directory::create(&path).unwrap();
        let source = fixture.create_dir("source").unwrap();
        let nested = source.create_dir("nested").unwrap();
        let destination = fixture.create_dir("destination").unwrap();
        source.write_new("settings.json", b"{}", 0o600).unwrap();
        source.write_new("plugin", b"#!/bin/sh\n", 0o700).unwrap();
        nested
            .write_new(".credentials.json.backup", b"fake", 0o600)
            .unwrap();
        nested.write_new(".roost-tmp-fake", b"fake", 0o600).unwrap();
        nested.write_new("ordinary", b"ordinary", 0o600).unwrap();
        nested.write_new("hard-source", b"hard", 0o600).unwrap();
        std::fs::hard_link(
            nested.path.join("hard-source"),
            nested.path.join("hard-link"),
        )
        .unwrap();
        std::os::unix::fs::symlink(&source.path, nested.path.join("linked-directory")).unwrap();
        source
            .create_dir("cache")
            .unwrap()
            .write_new("skip", b"skip", 0o600)
            .unwrap();
        let omitted = super::super::copy_profile(&source.path, &destination).unwrap();
        assert_eq!(
            destination.entries().unwrap(),
            ["nested", "plugin", "settings.json"]
        );
        assert_eq!(
            destination
                .child("nested", true)
                .unwrap()
                .entries()
                .unwrap(),
            ["ordinary"]
        );
        assert_eq!(omitted.len(), 6);
        assert_eq!(
            std::fs::metadata(destination.path.join("plugin"))
                .unwrap()
                .mode()
                & 0o777,
            0o700
        );
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn symlinks_are_created_and_read_relative_to_the_handle_without_following() {
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "roost-symlink-{}",
            super::super::random_id().unwrap()
        ));
        let fixture = Directory::create(&path).unwrap();
        let skills = fixture.create_dir("skills").unwrap();
        let target = Path::new("/nonexistent/skill-source/tdd");
        let identity = skills.symlink(target, "tdd").unwrap();
        let entry = skills.entry("tdd").unwrap().unwrap();
        assert!(entry.is_link);
        assert_eq!(entry.identity, identity);
        assert_eq!(skills.read_link("tdd").unwrap().as_deref(), Some(target));
        assert_eq!(skills.read_link("absent").unwrap(), None);
        // Existing entries are never replaced and regular files are not links.
        assert_eq!(
            skills
                .symlink(Path::new("/elsewhere"), "tdd")
                .err()
                .unwrap()
                .code,
            "collision"
        );
        skills.write_new("plain", b"x", 0o600).unwrap();
        assert_eq!(skills.read_link("plain").err().unwrap().code, "unsafe_path");
        assert!(skills.symlink(Path::new("relative"), "rel").is_err());
        assert!(skills.symlink(Path::new("/a\nb"), "nl").is_err());
        assert!(skills.symlink(target, "../escape").is_err());
        // A link standing in for the parent directory is never traversed.
        std::os::unix::fs::symlink(&skills.path, fixture.path.join("redirect")).unwrap();
        assert!(fixture.child("redirect", false).is_err());
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn bounded_pipe_input_stops_at_the_limit() {
        use std::os::fd::FromRawFd;
        for (length, accepted) in [(65536, true), (65537, false)] {
            let mut descriptors = [0; 2];
            assert_eq!(unsafe { libc::pipe(descriptors.as_mut_ptr()) }, 0);
            let reader = unsafe { File::from_raw_fd(descriptors[0]) };
            let mut writer = unsafe { File::from_raw_fd(descriptors[1]) };
            let write = std::thread::spawn(move || {
                let _ = writer.write_all(&vec![b'x'; length]);
            });
            let result = bounded_input(reader.as_raw_fd(), false, 65536);
            assert_eq!(result.is_ok(), accepted);
            drop(reader);
            write.join().unwrap();
        }
    }

    #[test]
    fn oversized_terminal_line_is_discarded() {
        use std::ffi::CStr;
        use std::os::fd::FromRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
        assert!(master >= 0);
        let mut master = unsafe { File::from_raw_fd(master) };
        assert_eq!(unsafe { libc::grantpt(master.as_raw_fd()) }, 0);
        assert_eq!(unsafe { libc::unlockpt(master.as_raw_fd()) }, 0);
        let name = unsafe { CStr::from_ptr(libc::ptsname(master.as_raw_fd())) };
        let slave = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(name.to_str().unwrap())
            .unwrap();
        let mut line = vec![b'y'; 200];
        line.extend_from_slice(b"\necho leaked\n");
        master.write_all(&line).unwrap();
        let result = bounded_input(slave.as_raw_fd(), true, 128);
        assert_eq!(result.unwrap_err().code, "invalid_token");
        // The rest of the line and the next one never reach the shell.
        assert!(read_ready(slave.as_raw_fd(), 200, 512).unwrap().is_none());
    }
    #[test]
    fn anchored_files_and_no_link_traversal() {
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "roost-platform-{}",
            super::super::random_id().unwrap()
        ));
        let directory = Directory::create(&path).unwrap();
        directory.write_new("private", b"fake", 0o600).unwrap();
        assert_eq!(
            directory.read("private", true, 10).unwrap().unwrap(),
            b"fake"
        );
        std::fs::hard_link(path.join("private"), path.join("hard")).unwrap();
        assert!(directory.read("hard", true, 10).is_err());
        std::os::unix::fs::symlink("private", path.join("link")).unwrap();
        assert!(directory.read("link", true, 10).is_err());
        assert!(Directory::open(&path.join("link"), false).is_err());
        let child = directory.create_dir("child").unwrap();
        child.write_new("file", b"ordinary", 0o600).unwrap();
        std::os::unix::fs::symlink(&path, child.path.join("outside")).unwrap();
        directory.write_new("marker", b"marker", 0o600).unwrap();
        directory.purge_children("marker").unwrap();
        assert_eq!(directory.entries().unwrap(), ["marker"]);
        std::fs::set_permissions(path.join("marker"), std::fs::Permissions::from_mode(0o644))
            .unwrap();
        assert!(directory.read("marker", true, 10).is_err());
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn purge_refuses_replaced_parent_path() {
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "roost-purge-{}",
            super::super::random_id().unwrap()
        ));
        let fixture = Directory::create(&path).unwrap();
        let profile = fixture.create_dir("profile").unwrap();
        profile.write_new("marker", b"marker", 0o600).unwrap();
        profile
            .create_dir("child")
            .unwrap()
            .write_new("owned", b"owned", 0o600)
            .unwrap();
        let foreign = fixture.create_dir("foreign").unwrap();
        foreign
            .create_dir("child")
            .unwrap()
            .write_new("foreign", b"foreign", 0o600)
            .unwrap();
        fixture.rename("profile", &fixture, "saved").unwrap();
        std::os::unix::fs::symlink(&foreign.path, path.join("profile")).unwrap();
        assert!(profile.purge_children("marker").is_err());
        assert!(path.join("saved/child/owned").exists());
        assert!(path.join("foreign/child/foreign").exists());
        std::fs::remove_dir_all(path).unwrap();
    }
}
