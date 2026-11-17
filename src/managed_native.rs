//! Native plugin operations on Roost's Claude store and owned Codex homes.

use crate::{
    Error, Result, launch,
    platform::{self, Directory},
    plugins,
    store::Store,
};
use serde_json::Value;
use std::{
    fs::{self, DirBuilder, File},
    io::Read,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub(crate) use plugins::{
    claude_check, claude_current, claude_install, claude_remove, claude_update,
};

const CODEX_LOCK: &str = ".roost-plugin-lock";
const LIST_TIMEOUT: Duration = Duration::from_secs(10);
const MUTATION_TIMEOUT: Duration = Duration::from_secs(130);
const CONTENT_LIMIT: u64 = 128 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct NativeInstall {
    pub id: String,
    pub revision: String,
    pub owned: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CatalogSource {
    Github(String),
    Https(String),
    Local(PathBuf),
}

impl CatalogSource {
    pub(crate) fn remote(value: &str) -> Option<Self> {
        let github = value
            .strip_prefix("https://github.com/")
            .map(|path| path.strip_suffix(".git").unwrap_or(path))
            .unwrap_or(value);
        if let Some((owner, repo)) = github.split_once('/')
            && valid_part(owner)
            && valid_part(repo)
        {
            return Some(Self::Github(github.to_ascii_lowercase()));
        }
        let (host, path) = value.strip_prefix("https://")?.split_once('/')?;
        if !host.contains('.') || !host.split('.').all(valid_part) {
            return None;
        }
        path.split('/')
            .all(valid_part)
            .then(|| Self::Https(value.into()))
    }

    pub(crate) fn local(value: &str) -> Option<Self> {
        if !Path::new(value).is_absolute() || value.chars().any(char::is_control) {
            return None;
        }
        let path = fs::canonicalize(value).ok()?;
        path.to_str()
            .is_some_and(|text| !text.chars().any(char::is_control))
            .then_some(Self::Local(path))
    }

    pub(crate) fn is_git(&self) -> bool {
        !matches!(self, Self::Local(_))
    }

    pub(crate) fn arg(&self) -> &str {
        match self {
            Self::Github(value) | Self::Https(value) => value,
            Self::Local(path) => path.to_str().expect("validated marketplace path"),
        }
    }
}

pub(crate) enum Source<'a> {
    Id(&'a str),
    Catalog(CatalogSource),
}

pub(crate) fn valid_part(value: &str) -> bool {
    !value.starts_with('-')
        && !matches!(value, "." | "..")
        && (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

pub(crate) fn split_id(id: &str) -> Result<(&str, &str)> {
    id.split_once('@')
        .filter(|(plugin, market)| valid_part(plugin) && valid_part(market))
        .ok_or_else(|| Error::new("usage", "Expected a safe PLUGIN@MARKETPLACE identifier"))
}

pub(crate) fn source_for<'a>(provider: &str, source: &'a str) -> Result<Source<'a>> {
    let source = source
        .strip_prefix(provider)
        .and_then(|suffix| suffix.strip_prefix(':'))
        .unwrap_or(source);
    if split_id(source).is_ok() {
        return Ok(Source::Id(source));
    }
    CatalogSource::remote(source)
        .or_else(|| CatalogSource::local(source))
        .map(Source::Catalog)
        .ok_or_else(|| {
            Error::new(
                "usage",
                "Expected PLUGIN@MARKETPLACE, OWNER/REPO, a credential-free HTTPS Git URL, or an absolute marketplace path",
            )
        })
}

#[derive(Debug)]
pub(crate) struct Marketplace {
    pub name: String,
    pub source: Option<CatalogSource>,
    pub location: Option<PathBuf>,
}

pub(crate) fn match_market<'a>(
    markets: &'a [Marketplace],
    source: &CatalogSource,
) -> Result<Option<&'a Marketplace>> {
    let mut found = markets.iter().filter(|m| m.source.as_ref() == Some(source));
    let first = found.next();
    if found.next().is_some() {
        return Err(Error::new(
            "collision",
            "The source matches multiple configured marketplaces",
        ));
    }
    Ok(first)
}

pub(crate) fn market_for_id<'a>(
    markets: &'a [Marketplace],
    source: Source<'_>,
    id: &str,
) -> Result<Option<&'a Marketplace>> {
    let (_, market) = split_id(id)?;
    let mut matches = markets.iter().filter(|m| m.name == market);
    let found = matches.next();
    if matches.next().is_some() {
        return Err(Error::new(
            "collision",
            "Several configured marketplaces have the plugin's marketplace name",
        ));
    }
    match source {
        Source::Id(expected) if expected == id => Ok(found),
        Source::Catalog(expected) if found.and_then(|m| m.source.as_ref()) == Some(&expected) => {
            Ok(found)
        }
        _ => Err(Error::new(
            "conflict",
            "The installed plugin no longer matches its marketplace source",
        )),
    }
}

pub(crate) fn unique_plugin(mut ids: Vec<String>) -> Result<String> {
    ids.sort();
    ids.dedup();
    match ids.as_slice() {
        [id] => Ok(id.clone()),
        [] => Err(Error::new(
            "not_found",
            "The marketplace offers no compatible native plugin",
        )),
        _ => Err(Error::new(
            "collision",
            format!("The marketplace offers several plugins: {}", ids.join(", ")),
        )
        .next("Add the marketplace to the target first, then use PLUGIN@MARKETPLACE")),
    }
}

pub(crate) fn scratch() -> Result<Scratch> {
    let parent = fs::canonicalize(std::env::temp_dir()).map_err(|e| {
        Error::io(
            "open native plugin scratch parent",
            &std::env::temp_dir(),
            e,
        )
    })?;
    let path = parent.join(format!("roost-native-{}", platform::random_id()?));
    DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .map_err(|e| Error::io("create native plugin scratch directory", &path, e))?;
    Ok(Scratch(path))
}

pub(crate) struct Scratch(pub PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn run(
    mut command: Command,
    provider: &str,
    operation: &str,
    args: &[&str],
    listing: bool,
) -> Result<Vec<u8>> {
    let timeout = if listing {
        LIST_TIMEOUT
    } else {
        MUTATION_TIMEOUT
    };
    command.current_dir("/").args(args);
    let (output, exit) = launch::probe(command, "native_plugin", timeout).map_err(|error| {
        if error.code == "cancelled" {
            error
        } else {
            Error::new(
                "native_plugin",
                error.message.replace("Claude probe", operation),
            )
        }
    })?;
    if !exit.success() {
        let error = Error::new(
            "native_plugin",
            format!(
                "{provider} {operation} failed ({})",
                exit.code()
                    .map_or("terminated".into(), |n| format!("exit {n}")),
            ),
        );
        return Err(if provider == "Codex" {
            error.next("Check the marketplace and sign-in status in the selected Codex home")
        } else {
            error
        });
    }
    Ok(output)
}

pub(crate) fn json(output: &[u8], provider: &str, operation: &str) -> Result<Value> {
    serde_json::from_slice(output).map_err(|_| {
        Error::new(
            "native_plugin",
            format!("{provider} {operation} returned unrecognized JSON"),
        )
    })
}

pub(crate) fn valid_version(version: &str) -> bool {
    (1..=128).contains(&version.len())
        && !matches!(version, "." | "..")
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'+'))
}

/// Stable fingerprint of the provider version and installed content. Frame all
/// names and payloads unambiguously, and inspect links without following them.
pub(crate) fn revision(version: &str, directory: &Directory) -> Result<String> {
    if !valid_version(version) {
        return Err(Error::new(
            "native_plugin",
            "The plugin has no verifiable version",
        ));
    }
    let mut hash = 0xcbf29ce484222325_u64;
    let mut left = CONTENT_LIMIT;
    hash_tree(directory, &mut hash, &mut left, 0)?;
    Ok(format!("{version}+roost1.{hash:016x}"))
}

pub(crate) fn reported_version(revision: &str) -> Option<&str> {
    revision.rsplit_once("+roost1.").map(|(version, _)| version)
}

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

fn hash_field(hash: &mut u64, bytes: &[u8]) {
    hash_bytes(hash, &(bytes.len() as u64).to_le_bytes());
    hash_bytes(hash, bytes);
}

fn hash_tree(directory: &Directory, hash: &mut u64, left: &mut u64, depth: usize) -> Result<()> {
    let invalid = || {
        Error::new(
            "native_plugin",
            "The installed plugin tree cannot be verified",
        )
    };
    if depth > 64 {
        return Err(invalid());
    }
    let mut names = directory.entries()?;
    if depth == 0 && directory.entry(".git")?.is_some_and(|entry| entry.is_dir) {
        // Some Codex marketplace installs copy the checkout's history, index
        // and reflogs. These differ between independent copies of one plugin.
        names.retain(|name| name != ".git");
    }
    names.sort_unstable();
    hash_bytes(hash, &(names.len() as u64).to_le_bytes());
    for name in names {
        let entry = directory.entry(&name)?.ok_or_else(invalid)?;
        hash_field(hash, name.as_bytes());
        if entry.is_dir {
            hash_bytes(hash, b"d");
            hash_tree(&directory.child(&name, false)?, hash, left, depth + 1)?;
        } else if entry.is_link {
            let link = directory.read_link(&name)?.ok_or_else(invalid)?;
            hash_bytes(hash, b"l");
            hash_field(hash, link.as_os_str().as_encoded_bytes());
        } else if entry.is_file {
            hash_bytes(hash, b"f");
            let file = directory.open_file(&name, false, false)?;
            let size = file.metadata().map_err(|_| invalid())?.len();
            *left = left.checked_sub(size).ok_or_else(invalid)?;
            let executable = platform::file_mode(&file)? & 0o111;
            hash_bytes(hash, &executable.to_le_bytes());
            hash_bytes(hash, &size.to_le_bytes());
            let mut file = file.take(size + 1);
            let mut buffer = [0; 8192];
            loop {
                let count = file.read(&mut buffer).map_err(|_| invalid())?;
                if count == 0 {
                    break;
                }
                hash_bytes(hash, &buffer[..count]);
            }
            if file.limit() != 1 {
                return Err(invalid());
            }
        } else {
            return Err(invalid());
        }
    }
    Ok(())
}

pub(crate) fn require_revision(current: Option<&str>, expected: &str) -> Result<()> {
    if current != Some(expected) {
        return Err(Error::new(
            "conflict",
            "The installed plugin changed since Roost recorded its revision",
        ));
    }
    Ok(())
}

pub(crate) fn changed_revision(current: &str, candidate: String) -> Option<String> {
    (current != candidate).then_some(candidate)
}

pub(crate) fn require_candidate(expected: &str, actual: &str, installed: bool) -> Result<()> {
    if expected != actual {
        return Err(Error::new(
            "conflict",
            if installed {
                "The provider installed a different revision than the one Roost checked"
            } else {
                "The upstream plugin revision changed since Roost checked it"
            },
        ));
    }
    Ok(())
}

struct Codex {
    home: Directory,
    _lock: Option<File>,
}

impl Codex {
    fn open(home: &Path) -> Result<Self> {
        let bad_home = || Error::new("ownership", "Expected a marker-verified Roost Codex home");
        let parent = home.parent().ok_or_else(bad_home)?;
        if parent.file_name().and_then(|n| n.to_str()) != Some("codex") {
            return Err(bad_home());
        }
        let root = parent.parent().ok_or_else(bad_home)?;
        let name = home
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(bad_home)?;
        let directory = Directory::open(home, true)?;
        let verify = || -> Result<()> {
            let store = Store::open(root, false, crate::store::OpenMode::Read)?;
            if store.pending() || store.codex_profile(name)?.path != home {
                return Err(bad_home());
            }
            directory.verify_namespace()
        };
        verify()?;
        let lock = plugins::provider_lock(&directory, CODEX_LOCK, "Codex home", true)?;
        verify()?;
        Ok(Self {
            home: directory,
            _lock: lock,
        })
    }

    fn scratch(scratch: &Scratch) -> Result<Self> {
        Ok(Self {
            home: Directory::open(&scratch.0, false)?,
            _lock: None,
        })
    }

    fn command(&self, operation: &str, args: &[&str], listing: bool) -> Result<Value> {
        self.home.verify_namespace()?;
        let command = launch::codex_command(&self.home.path, false)?;
        let output = run(command, "Codex", operation, args, listing)?;
        json(&output, "Codex", operation)
    }

    fn marketplaces(&self) -> Result<Vec<Marketplace>> {
        let listing = self.command(
            "marketplace list",
            &["plugin", "marketplace", "list", "--json"],
            true,
        )?;
        let rows = listing["marketplaces"].as_array().ok_or_else(|| {
            Error::new(
                "native_plugin",
                "Codex did not report its configured marketplaces",
            )
        })?;
        Ok(rows
            .iter()
            .filter_map(|row| {
                let name = row["name"].as_str().filter(|s| valid_part(s))?.into();
                let metadata = &row["marketplaceSource"];
                let source = metadata["source"]
                    .as_str()
                    .and_then(|s| match metadata["sourceType"].as_str() {
                        Some("git")
                            if metadata["ref"].is_null()
                                && metadata["sparsePaths"].as_array().is_none_or(Vec::is_empty) =>
                        {
                            CatalogSource::remote(s)
                        }
                        Some("local") => CatalogSource::local(s),
                        _ => None,
                    });
                Some(Marketplace {
                    name,
                    source,
                    location: None,
                })
            })
            .collect())
    }

    fn add_marketplace(&self, source: &CatalogSource) -> Result<()> {
        self.command(
            "marketplace add",
            &["plugin", "marketplace", "add", source.arg(), "--json"],
            false,
        )?;
        Ok(())
    }

    fn catalog_ids(&self, name: &str) -> Result<Vec<String>> {
        let listing = self.command(
            "plugin list",
            &[
                "plugin",
                "list",
                "--marketplace",
                name,
                "--available",
                "--json",
            ],
            true,
        )?;
        Ok(codex_rows(&listing, "installed")?
            .iter()
            .chain(codex_rows(&listing, "available")?)
            .filter_map(|row| {
                let id = row["pluginId"].as_str()?;
                (split_id(id).ok()?.1 == name).then(|| id.to_owned())
            })
            .collect())
    }

    fn current(&self, id: &str) -> Result<Option<String>> {
        let (plugin, market) = split_id(id)?;
        let listing = self.command(
            "plugin list",
            &["plugin", "list", "--marketplace", market, "--json"],
            true,
        )?;
        let mut found = codex_rows(&listing, "installed")?
            .iter()
            .filter(|row| row["pluginId"] == id);
        let Some(row) = found.next() else {
            return Ok(None);
        };
        let verifiable = || {
            Error::new(
                "native_plugin",
                "Codex did not report a verifiable installed plugin version",
            )
        };
        if found.next().is_some() || row["installed"] == false {
            return Err(verifiable());
        }
        let version = row["version"]
            .as_str()
            .filter(|v| valid_version(v))
            .ok_or_else(verifiable)?;
        let cache = self
            .home
            .child("plugins", false)?
            .child("cache", false)?
            .child(market, false)?
            .child(plugin, false)?
            .child(version, false)?;
        revision(version, &cache).map(Some)
    }

    fn install(&self, id: &str, owned: bool) -> Result<NativeInstall> {
        let receipt = self.command("plugin add", &["plugin", "add", id, "--json"], false)?;
        let verified = self.require_current(id)?;
        let version = receipt["version"].as_str().filter(|v| valid_version(v));
        if receipt["pluginId"] != id || version != reported_version(&verified) {
            return Err(Error::new(
                "native_plugin",
                "Codex's install receipt did not match the installed plugin",
            ));
        }
        Ok(NativeInstall {
            id: id.into(),
            revision: verified,
            owned,
        })
    }

    fn require_current(&self, id: &str) -> Result<String> {
        self.current(id)?.ok_or_else(|| {
            Error::new(
                "not_found",
                "The plugin was not found in the selected Codex home",
            )
        })
    }
}

fn codex_rows<'a>(listing: &'a Value, kind: &str) -> Result<&'a [Value]> {
    listing[kind].as_array().map(Vec::as_slice).ok_or_else(|| {
        Error::new(
            "native_plugin",
            "Codex's plugin listing did not contain the expected rows",
        )
    })
}

pub(crate) fn codex_install(home: &Path, source: &str) -> Result<NativeInstall> {
    let source = source_for("codex", source)?;
    let codex = Codex::open(home)?;
    let id = match source {
        Source::Id(id) => id.to_owned(),
        Source::Catalog(source) => {
            let markets = codex.marketplaces()?;
            if let Some(market) = match_market(&markets, &source)? {
                unique_plugin(codex.catalog_ids(&market.name)?)?
            } else {
                let temporary = scratch()?;
                let candidate = Codex::scratch(&temporary)?;
                candidate.add_marketplace(&source)?;
                let fresh = candidate.marketplaces()?;
                let market = match_market(&fresh, &source)?.ok_or_else(|| {
                    Error::new(
                        "native_plugin",
                        "Codex did not register the requested marketplace",
                    )
                })?;
                if markets.iter().any(|m| m.name == market.name) {
                    return Err(Error::new(
                        "collision",
                        "A different Codex marketplace already uses that name",
                    ));
                }
                let id = unique_plugin(candidate.catalog_ids(&market.name)?)?;
                codex.add_marketplace(&source)?;
                market_for_id(&codex.marketplaces()?, Source::Catalog(source), &id)?;
                if unique_plugin(codex.catalog_ids(&market.name)?)? != id {
                    return Err(Error::new(
                        "conflict",
                        "The Codex marketplace changed during installation",
                    ));
                }
                id
            }
        }
    };
    if let Some(revision) = codex.current(&id)? {
        return Ok(NativeInstall {
            id,
            revision,
            owned: false,
        });
    }
    codex.install(&id, true)
}

pub(crate) fn codex_current(home: &Path, id: &str) -> Result<Option<String>> {
    split_id(id)?;
    Codex::open(home)?.current(id)
}

pub(crate) fn codex_check(home: &Path, source: &str, id: &str) -> Result<Option<String>> {
    let source = source_for("codex", source)?;
    let (current, catalog) = {
        let codex = Codex::open(home)?;
        let current = codex.require_current(id)?;
        let markets = codex.marketplaces()?;
        let catalog = market_for_id(&markets, source, id)?
            .and_then(|m| m.source.clone())
            .ok_or_else(|| {
                Error::new(
                    "unsupported",
                    "Codex cannot reliably check updates for this marketplace",
                )
            })?;
        (current, catalog)
    };
    Ok(changed_revision(&current, codex_candidate(catalog, id)?))
}

fn codex_candidate(catalog: CatalogSource, id: &str) -> Result<String> {
    let temporary = scratch()?;
    let candidate = Codex::scratch(&temporary)?;
    candidate.add_marketplace(&catalog)?;
    market_for_id(&candidate.marketplaces()?, Source::Catalog(catalog), id)?;
    Ok(candidate.install(id, false)?.revision)
}

pub(crate) fn codex_update(
    home: &Path,
    source: &str,
    id: &str,
    expected_revision: &str,
    expected_candidate: Option<&str>,
) -> Result<NativeInstall> {
    let source = source_for("codex", source)?;
    let codex = Codex::open(home)?;
    require_revision(codex.current(id)?.as_deref(), expected_revision)?;
    let markets = codex.marketplaces()?;
    let catalog = market_for_id(&markets, source, id)?.and_then(|m| m.source.as_ref());
    if let Some(expected) = expected_candidate {
        let catalog = catalog.cloned().ok_or_else(|| {
            Error::new(
                "unsupported",
                "Codex cannot verify a candidate for this marketplace",
            )
        })?;
        require_candidate(expected, &codex_candidate(catalog, id)?, false)?;
        require_revision(codex.current(id)?.as_deref(), expected_revision)?;
    }
    if catalog.is_some_and(CatalogSource::is_git) {
        let (_, market) = split_id(id)?;
        let upgraded = codex.command(
            "marketplace upgrade",
            &["plugin", "marketplace", "upgrade", market, "--json"],
            false,
        )?;
        if !upgraded["errors"].as_array().is_some_and(Vec::is_empty)
            || !upgraded["selectedMarketplaces"]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|m| m == market))
        {
            return Err(Error::new(
                "native_plugin",
                "Codex did not upgrade the selected Git marketplace",
            ));
        }
        require_revision(codex.current(id)?.as_deref(), expected_revision)?;
    }
    // Ownership is decided on initial installation. Updating can also be
    // authorized for an artifact that Roost did not originally install.
    let installed = codex.install(id, false)?;
    if let Some(expected) = expected_candidate {
        require_candidate(expected, &installed.revision, true)?;
    }
    Ok(installed)
}

pub(crate) fn codex_remove(home: &Path, id: &str, expected_revision: &str) -> Result<()> {
    split_id(id)?;
    let codex = Codex::open(home)?;
    let current = codex.current(id)?;
    if current.is_none() {
        return Ok(());
    }
    require_revision(current.as_deref(), expected_revision)?;
    codex.command("plugin remove", &["plugin", "remove", id, "--json"], false)?;
    if codex.current(id)?.is_some() {
        return Err(Error::new(
            "native_plugin",
            "Codex still reports the plugin as installed after removal",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_sources_separate_ids_from_catalogs() {
        assert!(matches!(
            source_for("claude", "claude:plug@catalog").unwrap(),
            Source::Id("plug@catalog")
        ));
        assert!(matches!(
            source_for("codex", "mattpocock/skills").unwrap(),
            Source::Catalog(CatalogSource::Github(repo)) if repo == "mattpocock/skills"
        ));
        assert_eq!(
            CatalogSource::remote("https://github.com/MattPocock/skills.git"),
            CatalogSource::remote("mattpocock/skills"),
        );
        for source in [
            "-option@catalog",
            "plugin@..",
            "../repo",
            "owner/repo@branch",
            "git@github.com:owner/repo.git",
            "https://user:secret@example.com/owner/repo",
            "https://example.com/owner/repo?token=secret",
            "https://example.com/owner/%2e%2e",
        ] {
            assert!(source_for("codex", source).is_err(), "{source}");
        }
    }

    #[test]
    fn revisions_match_equal_caches_and_detect_content_drift() {
        let first = scratch().unwrap();
        let second = scratch().unwrap();
        let first = Directory::open(&first.0, false).unwrap();
        let second = Directory::open(&second.0, false).unwrap();
        for directory in [&first, &second] {
            directory.write_new("file", b"abc", 0o600).unwrap();
            std::os::unix::fs::symlink("file", directory.path.join("link")).unwrap();
        }
        let git = second.create_dir(".git").unwrap();
        git.write_new("index", b"provider metadata", 0o600).unwrap();
        let original = revision("1.0.0", &first).unwrap();
        let copied = revision("1.0.0", &second).unwrap();
        assert_eq!(original, copied);
        fs::write(second.path.join("file"), b"abd").unwrap();
        let edited = revision("1.0.0", &second).unwrap();
        assert_ne!(original, edited);
        assert!(require_revision(Some(&edited), &original).is_err());
    }

    #[test]
    fn marketplace_source_cannot_remap_an_installed_id() {
        let markets = vec![Marketplace {
            name: "market".into(),
            source: CatalogSource::remote("owner/catalog"),
            location: None,
        }];
        assert!(market_for_id(&markets, Source::Id("plug@market"), "plug@market").is_ok());
        assert!(
            market_for_id(
                &markets,
                source_for("codex", "another/catalog").unwrap(),
                "plug@market"
            )
            .is_err()
        );
        assert!(market_for_id(&markets, Source::Id("other@market"), "plug@market").is_err());
    }
}
