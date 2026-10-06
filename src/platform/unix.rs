use super::{Entry, FileIdentity, absolute, valid_name, validate_token};
use crate::{Error, Result};
use rustix::fs::{self, AtFlags, Mode, OFlags};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
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

fn check_namespace(file: &File, path: &Path) -> Result<()> {
    let metadata = file
        .metadata()
        .map_err(|e| Error::io("inspect parent namespace", path, e))?;
    let uid = rustix::process::geteuid().as_raw();
    if (metadata.uid() != 0 && metadata.uid() != uid)
        || (metadata.mode() & 0o022 != 0 && metadata.mode() & 0o1000 == 0)
    {
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
    #[allow(
        dead_code,
        reason = "shared scaffold for set links (05) and Desktop (09)"
    )]
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
    #[allow(
        dead_code,
        reason = "shared scaffold for set links (05) and Desktop (09)"
    )]
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
            Mode::from_raw_mode(mode),
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

    pub fn write_new(&self, name: &str, bytes: &[u8], mode: u32) -> Result<FileIdentity> {
        if cancelled() {
            return Err(Error::cancelled());
        }
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
    let mut buffer = vec![0_u8; 16384];
    loop {
        let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut result = std::ptr::null_mut();
        // getpwuid_r writes into caller-owned storage; record pointers remain valid while buffer lives.
        let code = unsafe {
            libc::getpwuid_r(
                libc::geteuid(),
                record.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        if code == libc::ERANGE && buffer.len() < 1048576 {
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        if code != 0 || result.is_null() {
            return Err(Error::new("io", "Cannot determine user home directory"));
        }
        let record = unsafe { record.assume_init() };
        let text = unsafe { std::ffi::CStr::from_ptr(record.pw_dir) }
            .to_str()
            .map_err(|_| Error::new("unsafe_path", "User home is not Unicode"))?;
        return absolute(Path::new(text));
    }
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

struct TerminalMode {
    descriptor: libc::c_int,
    original: libc::termios,
}
impl Drop for TerminalMode {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(self.descriptor, libc::TCSANOW, &self.original);
        }
    }
}

fn terminal() -> Result<File> {
    std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty")
        .map_err(|_| Error::new("usage", "An interactive terminal is required; use --stdin for tokens or --yes for confirmation"))
}

fn bounded_input(descriptor: libc::c_int, terminal: bool, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        if cancelled() {
            return Err(Error::cancelled());
        }
        let mut poll = libc::pollfd {
            fd: descriptor,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut poll, 1, 100) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(Error::new("io", "Cannot wait for input"));
        }
        if ready == 0 {
            continue;
        }
        let mut buffer = [0_u8; 512];
        let count = unsafe {
            libc::read(
                descriptor,
                buffer.as_mut_ptr().cast(),
                buffer.len().min(limit + 1 - bytes.len()),
            )
        };
        if count < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(Error::new("io", "Cannot read input"));
        }
        if count == 0 {
            break;
        }
        if terminal {
            for byte in &buffer[..count as usize] {
                if *byte == 4 {
                    return Ok(bytes);
                }
                if *byte == b'\n' || *byte == b'\r' {
                    bytes.push(*byte);
                    if bytes.len() > limit {
                        return Err(Error::new("invalid_token", "Input exceeds supported size"));
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
            bytes.extend_from_slice(&buffer[..count as usize]);
        }
        if bytes.len() > limit {
            return Err(Error::new("invalid_token", "Input exceeds supported size"));
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
    let mut tty = terminal()?;
    let descriptor = tty.as_raw_fd();
    let mut mode = std::mem::MaybeUninit::<libc::termios>::uninit();
    if unsafe { libc::tcgetattr(descriptor, mode.as_mut_ptr()) } != 0 {
        return Err(Error::new("io", "Cannot inspect terminal mode"));
    }
    let original = unsafe { mode.assume_init() };
    let mut hidden = original;
    // Noncanonical input avoids the kernel's much smaller canonical-line ceiling.
    hidden.c_lflag &= !(libc::ECHO | libc::ECHONL | libc::ICANON);
    hidden.c_cc[libc::VMIN] = 1;
    hidden.c_cc[libc::VTIME] = 0;
    if unsafe { libc::tcsetattr(descriptor, libc::TCSANOW, &hidden) } != 0 {
        return Err(Error::new("io", "Cannot disable terminal echo"));
    }
    let guard = TerminalMode {
        descriptor,
        original,
    };
    tty.write_all(b"Token: ")
        .map_err(|_| Error::new("io", "Cannot write terminal prompt"))?;
    let bytes = bounded_input(descriptor, true, 65536);
    drop(guard);
    let _ = tty.write_all(b"\n");
    let bytes = bytes?;
    if bytes.is_empty() {
        return Err(Error::cancelled());
    }
    validate_token(&bytes)
}

/// Column count of the terminal on stdout; None when stdout is not a terminal or
/// reports no size.
pub fn stdout_width() -> Option<usize> {
    use std::io::IsTerminal;
    let stdout = std::io::stdout();
    if !stdout.is_terminal() {
        return None;
    }
    rustix::termios::tcgetwinsize(&stdout)
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
    let bytes = bounded_input(tty.as_raw_fd(), true, 128)?;
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
        [_, ..] => (Key::Other, 1),
    })
}

/// Waits up to `timeout` milliseconds for input on `descriptor`; `None` on timeout,
/// an empty vector at EOF.
fn read_ready(descriptor: libc::c_int, timeout: libc::c_int) -> Result<Option<Vec<u8>>> {
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
    let mut buffer = [0_u8; 64];
    let count = unsafe { libc::read(descriptor, buffer.as_mut_ptr().cast(), buffer.len()) };
    if count < 0 {
        if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            return Ok(None);
        }
        return Err(Error::new("io", "Cannot read input"));
    }
    Ok(Some(buffer[..count as usize].to_vec()))
}

/// Single-choice picker on stderr reading keys from the terminal on stdin. Up/Down
/// or k/j move, Enter chooses; Ctrl-C, Esc, `q` or EOF cancel (exit 130). The
/// terminal mode is restored and the list erased on every exit.
pub fn pick(header: &str, rows: &[String], initial: usize) -> Result<usize> {
    if rows.is_empty() {
        return Err(Error::new("not_found", "Nothing to choose from"));
    }
    if cancelled() {
        return Err(Error::cancelled());
    }
    let descriptor = libc::STDIN_FILENO;
    let mut mode = std::mem::MaybeUninit::<libc::termios>::uninit();
    if unsafe { libc::tcgetattr(descriptor, mode.as_mut_ptr()) } != 0 {
        return Err(Error::new("io", "Cannot inspect terminal mode"));
    }
    let original = unsafe { mode.assume_init() };
    let mut raw = original;
    // Keep ISIG so Ctrl-C still raises SIGINT and reaches the cancel handler.
    raw.c_lflag &= !(libc::ECHO | libc::ICANON);
    raw.c_cc[libc::VMIN] = 1;
    raw.c_cc[libc::VTIME] = 0;
    if unsafe { libc::tcsetattr(descriptor, libc::TCSANOW, &raw) } != 0 {
        return Err(Error::new("io", "Cannot switch terminal mode"));
    }
    let guard = TerminalMode {
        descriptor,
        original,
    };
    let mut out = std::io::stderr();
    let height = rows.len() + 2;
    let draw = |current: usize, first: bool| -> String {
        let mut text = String::new();
        if !first {
            text.push_str(&format!("\x1b[{height}A"));
        }
        text.push_str("\r\x1b[2KChoose a profile for this project (↑/↓, Enter; Esc cancels)\n");
        text.push_str(&format!("\r\x1b[2K  {header}\n"));
        for (index, row) in rows.iter().enumerate() {
            if index == current {
                text.push_str(&format!("\r\x1b[2K\x1b[7m> {row}\x1b[0m\n"));
            } else {
                text.push_str(&format!("\r\x1b[2K  {row}\n"));
            }
        }
        text
    };
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
        match read_ready(descriptor, timeout) {
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
                Key::Choose => chosen = Some(Ok(current)),
                Key::Cancel => chosen = Some(Err(Error::cancelled())),
                Key::Other => (),
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
    fn copy_excludes_sensitive_and_linked_objects_at_depth() {
        let path =
            std::env::temp_dir().join(format!("roost-copy-{}", super::super::random_id().unwrap()));
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
        let path = std::env::temp_dir().join(format!(
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
    fn anchored_files_and_no_link_traversal() {
        let path = std::env::temp_dir().join(format!(
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
        let path = std::env::temp_dir().join(format!(
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
