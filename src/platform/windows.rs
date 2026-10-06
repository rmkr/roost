//! Native Windows storage is deliberately unavailable until a handle-relative
//! no-reparse walk and protected current-user DACL implementation is verified.
//! Returning an error here prevents a pathname/readonly substitute from exposing secrets.
use super::{Entry, FileIdentity, absolute};
use crate::{Error, Result};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler};

fn unavailable() -> Error {
    Error::new("unsafe_path", "Native Windows protected storage backend is not implemented")
        .next("Use a Linux Roost and Claude installation in WSL 2; native Windows support requires verified handle and DACL operations")
}

pub struct Directory {
    pub path: PathBuf,
}
impl Directory {
    pub fn open(_: &Path, _: bool) -> Result<Self> {
        Err(unavailable())
    }
    pub fn create(_: &Path) -> Result<Self> {
        Err(unavailable())
    }
    pub fn identity(&self) -> Result<FileIdentity> {
        Err(unavailable())
    }
    pub(crate) fn restrict_to_owner(&self) -> Result<()> {
        Err(unavailable())
    }
    pub fn entries(&self) -> Result<Vec<String>> {
        Err(unavailable())
    }
    pub fn entry(&self, _: &str) -> Result<Option<Entry>> {
        Err(unavailable())
    }
    pub fn child(&self, _: &str, _: bool) -> Result<Self> {
        Err(unavailable())
    }
    pub fn create_dir(&self, _: &str) -> Result<Self> {
        Err(unavailable())
    }
    pub fn symlink(&self, _: &Path, _: &str) -> Result<FileIdentity> {
        Err(unavailable())
    }
    pub fn read_link(&self, _: &str) -> Result<Option<PathBuf>> {
        Err(unavailable())
    }
    pub fn open_file(&self, _: &str, _: bool, _: bool) -> Result<File> {
        Err(unavailable())
    }
    pub fn read(&self, _: &str, _: bool, _: usize) -> Result<Option<Vec<u8>>> {
        Err(unavailable())
    }
    pub(crate) fn create_file(&self, _: &str, _: u32) -> Result<File> {
        Err(unavailable())
    }
    pub fn write_new(&self, _: &str, _: &[u8], _: u32) -> Result<FileIdentity> {
        Err(unavailable())
    }
    pub fn rename(&self, _: &str, _: &Self, _: &str) -> Result<()> {
        Err(unavailable())
    }
    pub fn remove(&self, _: &str, _: bool) -> Result<()> {
        Err(unavailable())
    }
    pub fn sync(&self) -> Result<()> {
        Err(unavailable())
    }
    pub fn purge_children(&self, _: &str) -> Result<()> {
        Err(unavailable())
    }
}

pub fn file_identity(_: &File) -> Result<FileIdentity> {
    Err(unavailable())
}
pub(crate) fn copy_mode(_: &File) -> Result<u32> {
    Err(unavailable())
}
pub fn home() -> Result<PathBuf> {
    let value = std::env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            Error::new(
                "unsafe_path",
                "Windows user profile directory is unavailable",
            )
        })?;
    absolute(Path::new(&value))
}

static CANCELLED: AtomicBool = AtomicBool::new(false);
unsafe extern "system" fn console_handler(event: u32) -> i32 {
    if event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT {
        CANCELLED.store(true, Ordering::Relaxed);
        1
    } else {
        0
    }
}
pub fn cancelled() -> bool {
    CANCELLED.load(Ordering::Relaxed)
}
pub fn install_cancel_handler() -> Result<()> {
    if unsafe { SetConsoleCtrlHandler(Some(console_handler), 1) } == 0 {
        return Err(Error::new("io", "Cannot install Windows console handler"));
    }
    Ok(())
}
pub fn restore_for_exec() -> Result<()> {
    // Keep the handler installed so the shared-console parent waits for the child.
    if cancelled() {
        Err(Error::cancelled())
    } else {
        Ok(())
    }
}
pub fn token_input(_: bool) -> Result<String> {
    Err(unavailable())
}
/// Width fitting is unverified on Windows: render every column.
pub fn stdout_width() -> Option<usize> {
    None
}
pub fn prompt_line(_: &str) -> Result<String> {
    Err(unavailable())
}
pub fn confirm(_: &str, _: bool) -> Result<()> {
    Err(unavailable())
}
/// The arrow-key picker is unverified on Windows: refuse.
pub fn pick(_: &str, _: &[String], _: usize) -> Result<usize> {
    Err(unavailable())
}
pub fn setup_path(_: &Path, _: Option<&str>, _: bool) -> Result<Vec<String>> {
    Err(Error::new(
        "io",
        "Native Windows HKCU PATH setup backend is not implemented",
    ))
}
