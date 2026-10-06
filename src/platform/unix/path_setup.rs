use super::*;

const BEGIN: &str = "# >>> roost PATH >>>";
const END: &str = "# <<< roost PATH <<<";

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
fn fish_quote(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn block(path: &Path, fish: bool, newline: &str) -> Result<Vec<u8>> {
    let text = path
        .to_str()
        .ok_or_else(|| Error::new("unsafe_path", "PATH destination is not Unicode"))?;
    if text.contains(':') {
        return Err(Error::new(
            "unsafe_path",
            "Launcher directory contains a PATH separator",
        ));
    }
    let quoted = if fish {
        fish_quote(text)
    } else {
        shell_quote(text)
    };
    let lines = if fish {
        vec![
            BEGIN.to_owned(),
            format!("# roost launcher directory: {quoted}"),
            format!("fish_add_path --path {quoted}"),
            END.to_owned(),
        ]
    } else {
        vec![
            BEGIN.to_owned(),
            format!("# roost launcher directory: {quoted}"),
            "case \":${PATH-}:\" in".into(),
            format!("  *:{quoted}:*) ;;"),
            format!("  *) export PATH={quoted}${{PATH:+\":$PATH\"}} ;;"),
            "esac".into(),
            END.to_owned(),
        ]
    };
    Ok(format!("{}{newline}", lines.join(newline)).into_bytes())
}

fn decode_quote(quoted: &str, fish: bool) -> Option<String> {
    let inner = quoted.strip_prefix('\'')?.strip_suffix('\'')?;
    let decoded = if fish {
        let mut result = String::new();
        let mut chars = inner.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                let next = chars.next()?;
                if next != '\\' && next != '\'' {
                    return None;
                }
                result.push(next);
            } else {
                result.push(ch);
            }
        }
        result
    } else {
        inner.replace("'\\''", "'")
    };
    let roundtrip = if fish {
        fish_quote(&decoded)
    } else {
        shell_quote(&decoded)
    };
    (roundtrip == quoted).then_some(decoded)
}

fn replace_block(bytes: &[u8], path: &Path, fish: bool) -> Result<Vec<u8>> {
    let starts: Vec<usize> = bytes
        .windows(BEGIN.len())
        .enumerate()
        .filter_map(|(i, v)| (v == BEGIN.as_bytes()).then_some(i))
        .collect();
    let ends: Vec<usize> = bytes
        .windows(END.len())
        .enumerate()
        .filter_map(|(i, v)| (v == END.as_bytes()).then_some(i))
        .collect();
    let newline = if bytes.windows(2).any(|v| v == b"\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    if starts.is_empty() && ends.is_empty() {
        if fish && !bytes.is_empty() {
            return Err(Error::new("collision", "Fish PATH snippet is foreign"));
        }
        let mut result = bytes.to_vec();
        if !result.is_empty() && !result.ends_with(b"\n") {
            result.extend_from_slice(newline.as_bytes());
        }
        result.extend(block(path, fish, "\n")?);
        return Ok(result);
    }
    if starts.len() != 1
        || ends.len() != 1
        || ends[0] <= starts[0]
        || (starts[0] > 0 && bytes[starts[0] - 1] != b'\n')
    {
        return Err(Error::new(
            "collision",
            "Malformed or multiple Roost PATH blocks",
        ));
    }
    let start = starts[0];
    let mut end = ends[0] + END.len();
    if bytes.get(end..end + 2) == Some(b"\r\n") {
        end += 2;
    } else if bytes.get(end) == Some(&b'\n') {
        end += 1;
    } else {
        return Err(Error::new("collision", "Malformed Roost PATH block ending"));
    }
    if fish && (start != 0 || end != bytes.len()) {
        return Err(Error::new(
            "collision",
            "Fish PATH snippet contains foreign content",
        ));
    }
    let old = std::str::from_utf8(&bytes[start..end])
        .map_err(|_| Error::new("collision", "Roost PATH block is not UTF-8"))?;
    let old_newline = if old.contains("\r\n") { "\r\n" } else { "\n" };
    let quoted = old
        .split(old_newline)
        .nth(1)
        .and_then(|line| line.strip_prefix("# roost launcher directory: "))
        .and_then(|quoted| decode_quote(quoted, fish))
        .ok_or_else(|| Error::new("collision", "Roost PATH block has foreign content"))?;
    let old_path = absolute(Path::new(&quoted))?;
    if block(&old_path, fish, old_newline)? != bytes[start..end] {
        return Err(Error::new(
            "collision",
            "Roost PATH block has foreign content",
        ));
    }
    let mut result = bytes[..start].to_vec();
    result.extend(block(path, fish, "\n")?);
    result.extend_from_slice(&bytes[end..]);
    Ok(result)
}

fn protected_parent(directory: &Directory) -> Result<()> {
    directory.verify_namespace()?;
    let metadata = directory
        .file
        .metadata()
        .map_err(|e| Error::io("inspect startup directory", &directory.path, e))?;
    if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o022 != 0 {
        return Err(Error::new(
            "ownership",
            "Startup directory must be user-owned and not writable by others",
        ));
    }
    Ok(())
}

fn ensure_directory(path: &Path) -> Result<Directory> {
    let path = absolute(path)?;
    let mut directory = Directory::open(Path::new("/"), false)?;
    for component in path.components() {
        if let Component::Normal(name) = component {
            let name = name
                .to_str()
                .ok_or_else(|| Error::new("unsafe_path", "Startup directory is not Unicode"))?;
            directory = match directory.entry(name)? {
                Some(_) => directory.child(name, false)?,
                None => {
                    protected_parent(&directory)?;
                    directory.create_dir(name)?
                }
            };
        }
    }
    protected_parent(&directory)?;
    Ok(directory)
}

fn apply_file(target: &Path, launcher: &Path, fish: bool) -> Result<()> {
    let parent_path = target
        .parent()
        .ok_or_else(|| Error::new("unsafe_path", "Startup target has no parent"))?;
    let parent = if fish {
        ensure_directory(parent_path)?
    } else {
        Directory::open(parent_path, false)?
    };
    protected_parent(&parent)?;
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::new("unsafe_path", "Startup filename is not Unicode"))?;
    let existing = parent.entry(name)?;
    let (bytes, permissions) = if let Some(ref entry) = existing {
        let file = parent.open_file(name, false, false)?;
        let metadata = file
            .metadata()
            .map_err(|e| Error::io("inspect startup file", target, e))?;
        if metadata.uid() != rustix::process::geteuid().as_raw()
            || file_identity(&file)? != entry.identity
            || metadata.mode() & 0o022 != 0
        {
            return Err(Error::new(
                "ownership",
                "Startup file is not safely user-owned",
            ));
        }
        let bytes = parent
            .read(name, false, 4 * 1048576)?
            .ok_or_else(|| Error::new("ownership", "Startup file disappeared"))?;
        (bytes, Some(metadata.permissions()))
    } else {
        (Vec::new(), None)
    };
    let replacement = replace_block(&bytes, launcher, fish)?;
    if replacement == bytes {
        return Ok(());
    }
    let temporary = format!(".roost-tmp-{}", super::super::random_id()?);
    let mut file = parent.create_file(&temporary, 0o600)?;
    let result = (|| {
        file.write_all(&replacement)
            .map_err(|e| Error::io("write startup stage", target, e))?;
        if let Some(ref permissions) = permissions {
            file.set_permissions(permissions.clone())
                .map_err(|e| Error::io("preserve startup permissions", target, e))?;
        }
        file.sync_all()
            .map_err(|e| Error::io("flush startup stage", target, e))?;
        if cancelled() {
            return Err(Error::cancelled());
        }
        protected_parent(&parent)?;
        if parent.entry(name)?.map(|entry| entry.identity) != existing.map(|entry| entry.identity)
            || parent.read(name, false, 4 * 1048576)?.unwrap_or_default() != bytes
        {
            return Err(Error::new(
                "ownership",
                "Startup target changed before publication",
            ));
        }
        if let Some(ref permissions) = permissions {
            let current = parent.open_file(name, false, false)?;
            let metadata = current
                .metadata()
                .map_err(|e| Error::io("revalidate startup file", target, e))?;
            if metadata.uid() != rustix::process::geteuid().as_raw()
                || metadata.permissions() != *permissions
            {
                return Err(Error::new(
                    "ownership",
                    "Startup owner or permissions changed before publication",
                ));
            }
        }
        parent.rename(&temporary, &parent, name)?;
        parent.sync()
    })();
    if result.is_err() {
        let _ = parent.remove(&temporary, false);
    }
    result
}

pub fn setup_path(root: &Path, shell: Option<&str>, apply: bool) -> Result<Vec<String>> {
    let launcher = absolute(root)?.join("bin");
    let inferred = std::env::var_os("SHELL").and_then(|value| {
        Path::new(&value)
            .file_name()
            .and_then(|v| v.to_str())
            .map(str::to_owned)
    });
    let selected = shell.or(inferred.as_deref()).unwrap_or("posix");
    if apply
        && shell.is_none()
        && !matches!(inferred.as_deref(), Some("bash" | "zsh" | "sh" | "fish"))
    {
        return Err(Error::new(
            "usage",
            "Select --shell bash, zsh, posix or fish before applying PATH setup",
        ));
    }
    let home = home()?;
    let (target, fish) = match selected {
        "bash" => (home.join(".bashrc"), false),
        "zsh" => {
            let base = std::env::var_os("ZDOTDIR")
                .filter(|v| !v.is_empty())
                .map(|v| absolute(Path::new(&v)))
                .transpose()?
                .unwrap_or(home);
            (base.join(".zshrc"), false)
        }
        "posix" | "sh" => (home.join(".profile"), false),
        "fish" => (
            fish_config_directory()?.join("conf.d/roost-path.fish"),
            true,
        ),
        _ => {
            return Err(Error::new(
                "usage",
                "Unsupported shell; choose bash, zsh, posix or fish",
            ));
        }
    };
    let target = absolute(&target)?;
    let recipe = String::from_utf8(block(&launcher, fish, "\n")?).expect("generated Unicode block");
    let mut messages = vec![format!("PATH target: {}", target.display())];
    if apply {
        eprintln!("PATH target: {}", target.display());
        apply_file(&target, &launcher, fish)?;
        messages.push("PATH configured; open a new terminal".into());
    } else {
        messages.push(recipe);
        messages.push("Use --apply to update only this startup destination".into());
    }
    Ok(messages)
}

fn fish_config_directory() -> Result<PathBuf> {
    let executable = std::env::var_os("SHELL")
        .filter(|value| {
            Path::new(value)
                .file_name()
                .is_some_and(|name| name == "fish")
        })
        .unwrap_or_else(|| "fish".into());
    let mut command = std::process::Command::new(executable);
    command.args(["-c", "printf '%s\\n' \"$__fish_config_dir\""]);
    let (captured, status) = crate::launch::bounded_probe(command, "io")?;
    if !status.success() {
        return Err(Error::new("io", "Fish configuration discovery failed"));
    }
    let text = std::str::from_utf8(&captured)
        .map_err(|_| Error::new("unsafe_path", "Fish configuration discovery is not Unicode"))?;
    let path = text.strip_suffix('\n').unwrap_or(text);
    let path = path.strip_suffix('\r').unwrap_or(path);
    if path.is_empty() || path.contains(['\r', '\n']) || !Path::new(path).is_absolute() {
        return Err(Error::new(
            "unsafe_path",
            "Fish configuration discovery did not return one absolute directory",
        ));
    }
    absolute(Path::new(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_apply_preserves_permissions_and_refuses_links() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!(
            "roost-path-{}",
            super::super::super::random_id().unwrap()
        ));
        let fixture = Directory::create(&path).unwrap();
        let target = path.join(".bashrc");
        std::fs::write(&target, b"# existing config\r\n").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
        apply_file(&target, Path::new("/tmp/Unicode-λ 'quote'/bin"), false).unwrap();
        let content = std::fs::read(&target).unwrap();
        assert!(content.starts_with(b"# existing config\r\n"));
        assert_eq!(std::fs::metadata(&target).unwrap().mode() & 0o777, 0o640);
        apply_file(&target, Path::new("/tmp/Unicode-λ 'quote'/bin"), false).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), content);
        let sourced = std::process::Command::new("bash")
            .args([
                "-c",
                "source \"$1\"; source \"$1\"; printf %s \"$PATH\"",
                "roost-test",
            ])
            .arg(&target)
            .output()
            .unwrap();
        assert!(
            sourced.status.success(),
            "generated PATH block must execute in Bash"
        );
        assert_eq!(
            String::from_utf8(sourced.stdout)
                .unwrap()
                .split(':')
                .filter(|entry| *entry == "/tmp/Unicode-λ 'quote'/bin")
                .count(),
            1
        );
        std::os::unix::fs::symlink(&target, path.join(".profile")).unwrap();
        assert!(apply_file(&path.join(".profile"), Path::new("/tmp/bin"), false).is_err());
        assert_eq!(fixture.entries().unwrap(), [".bashrc", ".profile"]);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn blocks_preserve_other_bytes_and_refuse_foreign_content() {
        let path = Path::new("/tmp/a 'quoted' \\ Unicode-λ/bin");
        let initial = b"# user config\r\nexport USER_VALUE=x\r\n";
        let once = replace_block(initial, path, false).unwrap();
        assert!(once.starts_with(initial));
        assert_eq!(replace_block(&once, path, false).unwrap(), once);
        let updated = replace_block(&once, Path::new("/tmp/new/bin"), false).unwrap();
        assert!(updated.starts_with(initial));
        assert!(
            replace_block(
                b"# >>> roost PATH >>>\nforeign\n# <<< roost PATH <<<\n",
                path,
                false
            )
            .is_err()
        );
        let fish = replace_block(b"", path, true).unwrap();
        assert_eq!(replace_block(&fish, path, true).unwrap(), fish);
        assert!(replace_block(b"fish_add_path '/foreign'\n", path, true).is_err());
    }
}
