use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "platform", rename_all = "snake_case", deny_unknown_fields)]
pub enum FileIdentity {
    Unix { device: String, inode: String },
    Windows { volume: String, index: String },
}

#[derive(Debug)]
pub struct Entry {
    pub identity: FileIdentity,
    pub is_dir: bool,
    pub is_file: bool,
    pub is_link: bool,
    pub nlink: u64,
}

pub fn absolute(path: &Path) -> Result<PathBuf> {
    let text = path
        .to_str()
        .ok_or_else(|| Error::new("unsafe_path", "Path is not valid Unicode"))?;
    if text.is_empty() || text.contains(['\r', '\n', '\0']) {
        return Err(Error::new(
            "unsafe_path",
            "Path is empty or contains unsupported characters",
        ));
    }
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|e| Error::io("read current directory", path, e))?
            .join(path)
    };
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => (),
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    if !result.is_absolute() {
        return Err(Error::new("unsafe_path", "Cannot resolve an absolute path"));
    }
    Ok(result)
}

/// Seconds since the Unix epoch (0 if the clock is before it): the one clock used
/// for recorded launch times, the auto-update window and relative ages.
pub fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub fn root() -> Result<PathBuf> {
    match std::env::var_os("ROOST_DIR") {
        Some(value) => absolute(Path::new(&value)),
        None => Ok(home()?.join(".roost")),
    }
}

pub fn default_directory() -> Result<PathBuf> {
    Ok(home()?.join(".claude"))
}

pub fn random_id() -> Result<String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|_| Error::new("io", "Operating-system randomness unavailable"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

pub fn path_member(path: &Path) -> bool {
    std::env::var_os("PATH").is_some_and(|value| {
        std::env::split_paths(&value).any(|entry| {
            #[cfg(unix)]
            {
                absolute(&entry).is_ok_and(|entry| entry == path)
            }
            #[cfg(windows)]
            {
                absolute(&entry).is_ok_and(|entry| {
                    entry
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&path.to_string_lossy())
                })
            }
        })
    })
}

pub(crate) fn valid_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\0', '\r', '\n'])
        || (cfg!(windows) && name.contains('\\'))
    {
        Err(Error::new("unsafe_path", "Unsafe directory entry name"))
    } else {
        Ok(())
    }
}

pub(crate) fn validate_token(bytes: &[u8]) -> Result<String> {
    if bytes.len() > 65536 {
        return Err(Error::new("invalid_token", "Token exceeds 64 KiB"));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::new("invalid_token", "Token is not valid UTF-8"))?
        .trim();
    if text.is_empty() || text.chars().any(|c| c == '\0' || c.is_whitespace()) {
        return Err(Error::new(
            "invalid_token",
            "Token is empty or contains whitespace or NUL",
        ));
    }
    Ok(text.to_owned())
}

fn excluded(name: &str, top: bool) -> bool {
    (top && matches!(
        name,
        "cache" | "daemon" | "ide" | "paste-cache" | "shell-snapshots" | "telemetry" | "backups"
    )) || matches!(
        name,
        ".ccm-oauth-token"
            | ".ccm-linked-default"
            | ".roost-root.json"
            | ".roost-profile.json"
            | ".roost-token"
            | ".roost-operation.json"
            | ".roost-lock"
            | ".roost-stage"
    ) || name.starts_with(".roost-tmp-")
        || [".credentials.json", ".claude.json"].iter().any(|base| {
            name == *base
                || name
                    .strip_prefix(base)
                    .is_some_and(|tail| tail.starts_with('.'))
        })
}

pub fn copy_profile(source: &Path, destination: &Directory) -> Result<Vec<String>> {
    let source = Directory::open(source, false)?;
    if source.identity()? == destination.identity()?
        || source.path.starts_with(&destination.path)
        || destination.path.starts_with(&source.path)
    {
        return Err(Error::new(
            "unsafe_path",
            "Copy source and destination overlap",
        ));
    }
    let mut omissions = Vec::new();
    copy_contents(&source, destination, Path::new(""), &mut omissions, 0)?;
    Ok(omissions)
}

fn copy_contents(
    source: &Directory,
    destination: &Directory,
    relative: &Path,
    omissions: &mut Vec<String>,
    depth: usize,
) -> Result<()> {
    if depth > 128 {
        return Err(Error::new(
            "unsafe_path",
            "Copy source nesting exceeds 128 directories",
        ));
    }
    for name in source.entries()? {
        if cancelled() {
            return Err(Error::cancelled());
        }
        let selected = relative.join(&name);
        let entry = source
            .entry(&name)?
            .ok_or_else(|| Error::new("io", "Copy source changed during traversal"))?;
        let reason = if excluded(&name, depth == 0) {
            Some("excluded")
        } else if entry.is_link {
            Some("linked entry")
        } else if entry.is_file && entry.nlink != 1 {
            Some("multiply linked file")
        } else if !entry.is_file && !entry.is_dir {
            Some("special file")
        } else {
            None
        };
        if let Some(reason) = reason {
            omissions.push(format!("Omitted {}: {reason}", selected.display()));
            continue;
        }
        if entry.is_dir {
            let child = source.child(&name, false)?;
            if child.identity()? != entry.identity {
                return Err(Error::new("unsafe_path", "Copy source directory changed"));
            }
            let target = destination.create_dir(&name)?;
            copy_contents(&child, &target, &selected, omissions, depth + 1)?;
            target.sync()?;
        } else {
            let mut input = source.open_file(&name, false, false)?;
            if file_identity(&input)? != entry.identity {
                return Err(Error::new("unsafe_path", "Copy source file changed"));
            }
            let mut output = destination.create_file(&name, copy_mode(&input)?)?;
            let mut buffer = [0_u8; 65536];
            loop {
                if cancelled() {
                    return Err(Error::cancelled());
                }
                let length = input
                    .read(&mut buffer)
                    .map_err(|e| Error::io("read copy source", &source.path.join(&name), e))?;
                if length == 0 {
                    break;
                }
                output.write_all(&buffer[..length]).map_err(|e| {
                    Error::io("write copied file", &destination.path.join(&name), e)
                })?;
            }
            output
                .sync_all()
                .map_err(|e| Error::io("flush copied file", &destination.path.join(&name), e))?;
        }
    }
    destination.sync()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn token_and_exclusion_boundaries() {
        assert_eq!(validate_token(b"  fake-token\n").unwrap(), "fake-token");
        for bytes in [b"".as_slice(), b"fake token", b"fake\0token"] {
            assert!(validate_token(bytes).is_err());
        }
        assert!(validate_token(&vec![b'x'; 65536]).is_ok());
        assert!(validate_token(&vec![b'x'; 65537]).is_err());
        assert!(excluded(".credentials.json.backup", false));
        assert!(excluded(".claude.json", false));
        assert!(!excluded("cache", false));
        assert!(excluded("cache", true));
        assert!(absolute(Path::new("bad\npath")).is_err());
        #[cfg(unix)]
        assert!(valid_name("ordinary\\file").is_ok());
    }
}
