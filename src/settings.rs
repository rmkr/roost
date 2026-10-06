//! Shared settings fragments (ADR 0003): small JSON files carrying `hooks`,
//! `statusLine` and `outputStyle`, merged at launch into an owned profile's own
//! `settings.json`.
//!
//! Roost co-edits a file Claude and the user also write, so it records in
//! `state.json` exactly which hook handlers and keys it wrote, removes only
//! recorded entries that are still identical, and replaces the file atomically only
//! when it is unchanged since read. All other keys keep their order, and numbers
//! their exact text, through [`Json`], an order-preserving document (serde_json's
//! `preserve_order` would reorder every `--json` envelope in the crate, and
//! `arbitrary_precision` would change how every other `Value` compares numbers).

use crate::{
    Error, Result,
    platform::{Directory, FileIdentity},
    store::{
        Kind, Registration, State, Store,
        side::{HookRecord, Item, ItemKind, SettingsRecord},
    },
};
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::{
    cell::RefCell,
    fmt,
    path::{Path, PathBuf},
};

/// Largest fragment or `settings.json` Roost reads.
const MAX_READ_BYTES: usize = 4 * 1024 * 1024;
const SETTINGS: &str = "settings.json";
const HOOKS: &str = "hooks";
const STATUS_LINE: &str = "statusLine";
const OUTPUT_STYLE: &str = "outputStyle";
/// Keys a fragment may set.
const ALLOWED: [&str; 3] = [HOOKS, STATUS_LINE, OUTPUT_STYLE];

/// A JSON value whose objects keep their key order and whose numbers keep their
/// exact text; duplicate keys are refused.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    /// The number exactly as written (`1e2` stays `1e2`, big integers keep every
    /// digit), so a rewrite never changes numbers Roost does not own.
    Number(String),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

/// Number tokens of a JSON text in document order, exactly as written. Only
/// meaningful for text serde_json accepts, which visits numbers in the same order.
fn number_texts(bytes: &[u8]) -> Vec<String> {
    let mut found = vec![];
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }
            b'-' | b'0'..=b'9' => {
                let start = i;
                while i < bytes.len()
                    && matches!(bytes[i], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
                {
                    i += 1;
                }
                found.push(String::from_utf8_lossy(&bytes[start..i]).into_owned());
            }
            _ => i += 1,
        }
    }
    found
}

/// Builds a [`Json`] while serde_json validates the text, taking each number's
/// text from the pre-scanned tokens.
#[derive(Clone, Copy)]
struct JsonSeed<'a>(&'a RefCell<std::vec::IntoIter<String>>);

impl JsonSeed<'_> {
    fn number<E: de::Error>(self) -> std::result::Result<Json, E> {
        self.0
            .borrow_mut()
            .next()
            .map(Json::Number)
            .ok_or_else(|| E::custom("unexpected number"))
    }
}

impl<'de> DeserializeSeed<'de> for JsonSeed<'_> {
    type Value = Json;
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Json, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for JsonSeed<'_> {
    type Value = Json;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON value")
    }
    fn visit_unit<E>(self) -> std::result::Result<Json, E> {
        Ok(Json::Null)
    }
    fn visit_bool<E>(self, v: bool) -> std::result::Result<Json, E> {
        Ok(Json::Bool(v))
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<Json, E> {
        self.number()
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<Json, E> {
        self.number()
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Json, E> {
        if !v.is_finite() {
            return Err(E::custom("non-finite number"));
        }
        self.number()
    }
    fn visit_str<E>(self, v: &str) -> std::result::Result<Json, E> {
        Ok(Json::String(v.to_owned()))
    }
    fn visit_string<E>(self, v: String) -> std::result::Result<Json, E> {
        Ok(Json::String(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Json, A::Error> {
        let mut items = vec![];
        while let Some(item) = seq.next_element_seed(self)? {
            items.push(item);
        }
        Ok(Json::Array(items))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<Json, A::Error> {
        let mut entries: Vec<(String, Json)> = vec![];
        while let Some(key) = map.next_key::<String>()? {
            if entries.iter().any(|(k, _)| *k == key) {
                return Err(de::Error::custom(format!("duplicate key {key}")));
            }
            let value = map.next_value_seed(self)?;
            entries.push((key, value));
        }
        Ok(Json::Object(entries))
    }
}

impl Json {
    pub(crate) fn parse(bytes: &[u8]) -> std::result::Result<Self, String> {
        let numbers = RefCell::new(number_texts(bytes).into_iter());
        let mut deserializer = serde_json::Deserializer::from_slice(bytes);
        let value = JsonSeed(&numbers)
            .deserialize(&mut deserializer)
            .and_then(|value| deserializer.end().map(|()| value))
            .map_err(|e| e.to_string())?;
        if numbers.borrow_mut().next().is_some() {
            return Err("unexpected number".into());
        }
        Ok(value)
    }
    /// The unordered value, for comparisons and records.
    pub(crate) fn to_value(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(b) => Value::Bool(*b),
            Self::Number(n) => serde_json::from_str(n).unwrap_or(Value::Null),
            Self::String(s) => Value::String(s.clone()),
            Self::Array(items) => Value::Array(items.iter().map(Self::to_value).collect()),
            Self::Object(entries) => Value::Object(
                entries
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_value()))
                    .collect(),
            ),
        }
    }
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
        match self {
            Self::Object(entries) => entries.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    /// Replaces `key` in place, or appends it at the end of the object.
    fn set(&mut self, key: &str, value: Json) {
        if let Self::Object(entries) = self {
            match entries.iter_mut().find(|(k, _)| k == key) {
                Some((_, slot)) => *slot = value,
                None => entries.push((key.to_owned(), value)),
            }
        }
    }
    fn remove(&mut self, key: &str) {
        if let Self::Object(entries) = self {
            entries.retain(|(k, _)| k != key);
        }
    }
    fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }
    /// Claude's `JSON.stringify(value, null, 2)` style.
    pub(crate) fn render(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out
    }
    fn write(&self, out: &mut String, depth: usize) {
        let indent = |out: &mut String, depth: usize| out.push_str(&"  ".repeat(depth));
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Number(n) => out.push_str(n),
            Self::String(s) => out.push_str(&quote(s)),
            Self::Array(items) if items.is_empty() => out.push_str("[]"),
            Self::Object(entries) if entries.is_empty() => out.push_str("{}"),
            Self::Array(items) => {
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    indent(out, depth + 1);
                    item.write(out, depth + 1);
                    out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
                }
                indent(out, depth);
                out.push(']');
            }
            Self::Object(entries) => {
                out.push_str("{\n");
                for (i, (key, value)) in entries.iter().enumerate() {
                    indent(out, depth + 1);
                    out.push_str(&quote(key));
                    out.push_str(": ");
                    value.write(out, depth + 1);
                    out.push_str(if i + 1 < entries.len() { ",\n" } else { "\n" });
                }
                indent(out, depth);
                out.push('}');
            }
        }
    }
}

fn quote(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into())
}

/// One hook handler under an event and matcher.
#[derive(Clone, Debug)]
struct Hook {
    event: String,
    matcher: Option<String>,
    handler: Json,
}
impl Hook {
    fn record(&self) -> HookRecord {
        HookRecord {
            event: self.event.clone(),
            matcher: self.matcher.clone(),
            handler: self.handler.to_value(),
        }
    }
    fn is(&self, record: &HookRecord) -> bool {
        self.event == record.event
            && self.matcher == record.matcher
            && self.handler.to_value() == record.handler
    }
}

/// A validated fragment.
#[derive(Debug, Default)]
struct Fragment {
    hooks: Vec<Hook>,
    status_line: Option<Json>,
    output_style: Option<Json>,
}

/// Validates a fragment document: an object with only `hooks`, `statusLine` and
/// `outputStyle`, each in Claude's shape.
fn fragment(bytes: &[u8]) -> std::result::Result<Fragment, String> {
    let document = Json::parse(bytes)?;
    let Json::Object(entries) = document else {
        return Err("not a JSON object".into());
    };
    let mut fragment = Fragment::default();
    for (key, value) in entries {
        match key.as_str() {
            HOOKS => fragment.hooks = hooks(&value)?,
            STATUS_LINE => {
                let valid = matches!(&value, Json::Object(_))
                    && value
                        .get("type")
                        .and_then(Json::as_str)
                        .is_some_and(|t| !t.is_empty())
                    && (value.get("type").and_then(Json::as_str) != Some("command")
                        || value.get("command").and_then(Json::as_str).is_some());
                if !valid {
                    return Err(
                        "statusLine must be an object with a type (and a command for type command)"
                            .into(),
                    );
                }
                fragment.status_line = Some(value);
            }
            OUTPUT_STYLE => {
                if !value.as_str().is_some_and(|s| !s.is_empty()) {
                    return Err("outputStyle must be a non-empty string".into());
                }
                fragment.output_style = Some(value);
            }
            other => {
                return Err(format!(
                    "key {other} is not allowed (only {})",
                    ALLOWED.join(", ")
                ));
            }
        }
    }
    Ok(fragment)
}

fn hooks(value: &Json) -> std::result::Result<Vec<Hook>, String> {
    let shape = "hooks must map event names to arrays of {matcher?, hooks:[{type, ...}]}";
    let Json::Object(events) = value else {
        return Err(shape.into());
    };
    let mut found = vec![];
    for (event, groups) in events {
        let Json::Array(groups) = groups else {
            return Err(shape.into());
        };
        if event.is_empty() {
            return Err(shape.into());
        }
        for group in groups {
            let Json::Object(fields) = group else {
                return Err(shape.into());
            };
            if fields.iter().any(|(k, _)| k != "matcher" && k != HOOKS) {
                return Err(shape.into());
            }
            let matcher = match group.get("matcher") {
                None => None,
                Some(Json::String(m)) => Some(m.clone()),
                Some(_) => return Err(shape.into()),
            };
            let Some(Json::Array(handlers)) = group.get(HOOKS) else {
                return Err(shape.into());
            };
            for handler in handlers {
                let typed = matches!(handler, Json::Object(_))
                    && handler
                        .get("type")
                        .and_then(Json::as_str)
                        .is_some_and(|t| !t.is_empty());
                if !typed {
                    return Err(shape.into());
                }
                found.push(Hook {
                    event: event.clone(),
                    matcher: matcher.clone(),
                    handler: handler.clone(),
                });
            }
        }
    }
    Ok(found)
}

/// Reads and validates one fragment file. An explicit `--setting` path may be a
/// link (the user manages it) and is followed; a source's children are opened
/// without following. Only a regular file is read, through a non-blocking open
/// and at most [`MAX_READ_BYTES`] + 1 bytes, so a FIFO or device swapped in is
/// refused instead of hanging the launch or filling memory.
fn read_fragment(path: &Path, follow: bool) -> std::result::Result<Fragment, String> {
    use std::io::Read;
    let file = crate::platform::open_user_file(path, follow).map_err(|e| e.message)?;
    let mut bytes = vec![];
    file.take(MAX_READ_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.kind().to_string())?;
    if bytes.len() > MAX_READ_BYTES {
        return Err("larger than 4 MiB".into());
    }
    fragment(&bytes)
}

/// Fragment files of a setting item: the file itself, or a source's immediate
/// regular `*.json` children (dot-names, links and other types skipped), sorted.
fn fragment_paths(item: &Item) -> std::result::Result<Vec<PathBuf>, String> {
    let value = PathBuf::from(&item.value);
    if item.kind == ItemKind::Setting {
        return Ok(vec![value]);
    }
    let mut paths = vec![];
    for entry in std::fs::read_dir(&value)
        .map_err(|e| e.kind().to_string())?
        .flatten()
    {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let regular = entry.file_type().is_ok_and(|t| t.is_file());
        if regular
            && !name.starts_with('.')
            && name.ends_with(".json")
            && !name.contains(['\r', '\n'])
        {
            paths.push(value.join(name));
        }
    }
    paths.sort();
    Ok(paths)
}

/// Whether a kind is a settings item.
pub(crate) fn is_setting(kind: ItemKind) -> bool {
    matches!(kind, ItemKind::Setting | ItemKind::SettingSource)
}

/// `set add` check: every fragment of the item is valid (usage error naming it).
pub(crate) fn validate(item: &Item) -> Result<()> {
    let paths = fragment_paths(item).map_err(|reason| {
        Error::new("not_found", format!("Cannot list {}: {reason}", item.value))
    })?;
    for path in paths {
        read_fragment(&path, item.kind == ItemKind::Setting).map_err(|reason| {
            Error::new(
                "usage",
                format!(
                    "{} is not a valid settings fragment: {reason}",
                    path.display()
                ),
            )
            .next("Fragments may set only hooks, statusLine and outputStyle")
        })?;
    }
    Ok(())
}

/// The desired value of a single-value key across subscribed fragments.
#[derive(Debug)]
enum Single {
    Absent,
    Value(Json),
    /// Set by more than one fragment: left exactly as it is.
    Conflict,
}

/// Merged desired settings of a registration's subscribed fragments.
struct Desired {
    hooks: Vec<Hook>,
    status_line: Single,
    output_style: Single,
}
impl Desired {
    fn is_empty(&self) -> bool {
        self.hooks.is_empty()
            && matches!(self.status_line, Single::Absent)
            && matches!(self.output_style, Single::Absent)
    }
}

/// Valid fragments of the subscribed setting items, each fragment file once, in
/// set and item order. Problems are reported through `warn`.
fn subscribed_fragments(
    items: &[(&str, &Item)],
    mut warn: impl FnMut(String),
) -> Vec<(PathBuf, Fragment)> {
    let mut found: Vec<(PathBuf, Fragment)> = vec![];
    for (set, item) in items.iter().filter(|(_, i)| is_setting(i.kind)) {
        let directory = item.kind == ItemKind::SettingSource;
        let present = std::fs::metadata(&item.value)
            .is_ok_and(|m| if directory { m.is_dir() } else { m.is_file() });
        if !present {
            warn(format!(
                "Skipped {} from set {set}: missing or not {}",
                item.value,
                if directory {
                    "a directory"
                } else {
                    "a regular file"
                }
            ));
            continue;
        }
        let paths = match fragment_paths(item) {
            Ok(paths) => paths,
            Err(reason) => {
                warn(format!("Skipped {} from set {set}: {reason}", item.value));
                continue;
            }
        };
        for path in paths {
            if found.iter().any(|(p, _)| *p == path) {
                continue;
            }
            match read_fragment(&path, !directory) {
                Ok(fragment) => found.push((path, fragment)),
                Err(reason) => warn(format!(
                    "Skipped settings fragment {} from set {set}: {reason}",
                    path.display()
                )),
            }
        }
    }
    found
}

/// Single-value keys set by more than one fragment, as messages.
fn single_conflicts(fragments: &[(PathBuf, Fragment)]) -> Vec<(&'static str, String)> {
    let mut found = vec![];
    for (key, pick) in [
        (
            STATUS_LINE,
            (|f: &Fragment| f.status_line.is_some()) as fn(&Fragment) -> bool,
        ),
        (OUTPUT_STYLE, |f: &Fragment| f.output_style.is_some()),
    ] {
        let paths: Vec<String> = fragments
            .iter()
            .filter(|(_, f)| pick(f))
            .map(|(p, _)| p.display().to_string())
            .collect();
        if paths.len() > 1 {
            found.push((
                key,
                format!("{key} would come from {}", paths.join(" and ")),
            ));
        }
    }
    found
}

/// Subscribe-time conflicts: a single-value key set by two subscribed fragments.
/// Invalid or vanished fragments contribute nothing.
pub(crate) fn conflicts(items: &[(&str, &Item)]) -> Vec<String> {
    let fragments = subscribed_fragments(items, |_| ());
    single_conflicts(&fragments)
        .into_iter()
        .map(|(_, message)| message)
        .collect()
}

fn desired(items: &[(&str, &Item)], warnings: &mut Vec<String>) -> Desired {
    let fragments = subscribed_fragments(items, |w| warnings.push(w));
    let conflicting = single_conflicts(&fragments);
    for (_, message) in &conflicting {
        warnings.push(format!("Skipped shared {message}"));
    }
    let single = |key: &str, pick: fn(&Fragment) -> Option<&Json>| {
        if conflicting.iter().any(|(k, _)| *k == key) {
            return Single::Conflict;
        }
        fragments
            .iter()
            .find_map(|(_, f)| pick(f).cloned())
            .map_or(Single::Absent, Single::Value)
    };
    let status_line = single(STATUS_LINE, |f| f.status_line.as_ref());
    let output_style = single(OUTPUT_STYLE, |f| f.output_style.as_ref());
    let mut hooks: Vec<Hook> = vec![];
    for hook in fragments.into_iter().flat_map(|(_, f)| f.hooks) {
        if !hooks.iter().any(|h| h.is(&hook.record())) {
            hooks.push(hook);
        }
    }
    Desired {
        hooks,
        status_line,
        output_style,
    }
}

fn group_matcher(group: &Json) -> Option<Option<&str>> {
    match group.get("matcher") {
        None => Some(None),
        Some(Json::String(m)) => Some(Some(m)),
        Some(_) => None,
    }
}

/// Whether `settings.hooks` holds a handler identical to `hook`.
fn contains_hook(settings: &Json, hook: &Hook) -> bool {
    let wanted = hook.handler.to_value();
    let Some(Json::Array(groups)) = settings.get(HOOKS).and_then(|h| h.get(&hook.event)) else {
        return false;
    };
    groups.iter().any(|group| {
        group_matcher(group) == Some(hook.matcher.as_deref())
            && matches!(group.get(HOOKS), Some(Json::Array(handlers))
                if handlers.iter().any(|h| h.to_value() == wanted))
    })
}

/// Removes one handler identical to `record`, pruning the group, event and
/// `hooks` key it leaves empty. Anything not identical is left alone.
fn remove_hook(settings: &mut Json, record: &HookRecord) {
    let Some(events) = settings.get_mut(HOOKS) else {
        return;
    };
    let Some(Json::Array(groups)) = events.get_mut(&record.event) else {
        return;
    };
    let mut removed = false;
    for group in groups.iter_mut() {
        if group_matcher(group) != Some(record.matcher.as_deref()) {
            continue;
        }
        let Some(Json::Array(handlers)) = group.get_mut(HOOKS) else {
            continue;
        };
        if let Some(index) = handlers.iter().position(|h| h.to_value() == record.handler) {
            handlers.remove(index);
            removed = true;
            break;
        }
    }
    if !removed {
        return;
    }
    groups.retain(|g| !matches!(g.get(HOOKS), Some(Json::Array(h)) if h.is_empty()));
    if groups.is_empty() {
        events.remove(&record.event);
    }
    if matches!(events, Json::Object(e) if e.is_empty()) {
        settings.remove(HOOKS);
    }
}

/// Appends `hook` to the first group with its matcher (or a new group); false
/// when the event's value is not an array (left untouched).
fn add_hook(settings: &mut Json, hook: &Hook) -> bool {
    if settings.get(HOOKS).is_none() {
        settings.set(HOOKS, Json::Object(vec![]));
    }
    let Some(events) = settings.get_mut(HOOKS) else {
        return false;
    };
    if events.get(&hook.event).is_none() {
        events.set(&hook.event, Json::Array(vec![]));
    }
    let Some(Json::Array(groups)) = events.get_mut(&hook.event) else {
        return false;
    };
    for group in groups.iter_mut() {
        if group_matcher(group) != Some(hook.matcher.as_deref()) {
            continue;
        }
        if let Some(Json::Array(handlers)) = group.get_mut(HOOKS) {
            handlers.push(hook.handler.clone());
            return true;
        }
    }
    let mut group = vec![];
    if let Some(matcher) = &hook.matcher {
        group.push(("matcher".to_owned(), Json::String(matcher.clone())));
    }
    group.push((HOOKS.to_owned(), Json::Array(vec![hook.handler.clone()])));
    groups.push(Json::Object(group));
    true
}

/// Reconciles hooks; returns the new hook records.
fn apply_hooks(
    settings: &mut Json,
    desired: &[Hook],
    recorded: &[HookRecord],
    warnings: &mut Vec<String>,
) -> Vec<HookRecord> {
    if !matches!(settings.get(HOOKS), None | Some(Json::Object(_))) {
        warnings.push("Skipped shared hooks: hooks in settings.json is not an object".into());
        return recorded.to_vec();
    }
    for record in recorded {
        if !desired.iter().any(|h| h.is(record)) {
            remove_hook(settings, record);
        }
    }
    let mut records = vec![];
    for hook in desired {
        if contains_hook(settings, hook) {
            // Present: still Roost's when recorded, else the profile's own.
            if recorded.iter().any(|r| hook.is(r)) {
                records.push(hook.record());
            }
        } else if add_hook(settings, hook) {
            records.push(hook.record());
        } else {
            warnings.push(format!(
                "Skipped shared {} hook: hooks.{} in settings.json is not an array",
                hook.event, hook.event
            ));
        }
    }
    records
}

/// Reconciles one single-value key; returns its new record. The profile's own
/// value wins: Roost replaces or removes only the value it recorded, unchanged.
fn apply_single(
    settings: &mut Json,
    key: &str,
    desired: &Single,
    recorded: Option<&Value>,
    warnings: &mut Vec<String>,
) -> Option<Value> {
    let current = settings.get(key).map(Json::to_value);
    match desired {
        Single::Conflict => recorded.cloned(),
        Single::Absent => {
            if recorded.is_some() && current.as_ref() == recorded {
                settings.remove(key);
            }
            None
        }
        Single::Value(value) => {
            let wanted = value.to_value();
            match &current {
                None => {
                    settings.set(key, value.clone());
                    Some(wanted)
                }
                Some(current) if *current == wanted => recorded.filter(|r| **r == wanted).cloned(),
                Some(current) if Some(current) == recorded => {
                    settings.set(key, value.clone());
                    Some(wanted)
                }
                Some(_) => {
                    if recorded.is_some() {
                        warnings.push(format!(
                            "{key} in settings.json was changed; the profile's own value wins and Roost no longer manages it there"
                        ));
                    }
                    None
                }
            }
        }
    }
}

/// `settings.json` as read: identity and exact bytes.
type Snapshot = Option<(FileIdentity, Vec<u8>)>;

/// Reads `settings.json` without following links; a link or multiply linked file
/// is refused (never edited).
fn snapshot(profile: &Directory) -> Result<Snapshot> {
    let Some(entry) = profile.entry(SETTINGS)? else {
        return Ok(None);
    };
    if entry.is_link || !entry.is_file || entry.nlink != 1 {
        return Err(Error::new(
            "unsafe_path",
            "settings.json is a link or not a single regular file",
        ));
    }
    let file = profile.open_file(SETTINGS, false, false)?;
    let identity = crate::platform::file_identity(&file)?;
    if identity != entry.identity {
        return Err(Error::new(
            "unsafe_path",
            "settings.json changed while opening",
        ));
    }
    let mut bytes = vec![];
    use std::io::Read;
    file.take(MAX_READ_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Error::io("read settings", &profile.path.join(SETTINGS), e))?;
    if bytes.len() > MAX_READ_BYTES {
        return Err(Error::new(
            "unsafe_path",
            "settings.json is larger than 4 MiB",
        ));
    }
    Ok(Some((identity, bytes)))
}

/// Atomic compare-and-replace: writes `bytes` (or removes the file for `None`)
/// only when `settings.json` is still exactly `before`. Returns the new identity.
fn replace(
    profile: &Directory,
    before: &Snapshot,
    bytes: Option<&[u8]>,
) -> Result<Option<FileIdentity>> {
    let changed = || {
        Error::new(
            "unsafe_path",
            "settings.json changed while Roost was updating it",
        )
    };
    let Some(bytes) = bytes else {
        if snapshot(profile)? != *before {
            return Err(changed());
        }
        profile.remove(SETTINGS, false)?;
        profile.sync()?;
        return Ok(None);
    };
    let temp = format!(".roost-settings-{}.tmp", crate::platform::random_id()?);
    let identity = profile.write_new(&temp, bytes, 0o600)?;
    let unchanged = snapshot(profile).is_ok_and(|now| now == *before);
    if !unchanged {
        let _ = profile.remove(&temp, false);
        return Err(changed());
    }
    if let Err(error) = profile.rename(&temp, profile, SETTINGS) {
        let _ = profile.remove(&temp, false);
        return Err(error);
    }
    profile.sync()?;
    Ok(Some(identity))
}

/// Launch-time reconciliation of shared settings into an owned active profile's
/// own `settings.json`, under the held lock. Never fails the launch: every
/// problem is a warning, and a skipped update is retried at the next launch.
pub(crate) fn reconcile(store: &Store, registration: &Registration) -> Vec<String> {
    let mut warnings = vec![];
    if registration.kind != Kind::Owned || registration.state != State::Active {
        return warnings;
    }
    let (sets, state) = match (store.read_sets(), store.read_state()) {
        (Ok(sets), Ok(state)) => (sets, state),
        (Err(error), _) | (_, Err(error)) => {
            warnings.push(format!("Skipped shared settings: {}", error.message));
            return warnings;
        }
    };
    let id = &registration.registration_id;
    let items = crate::sets::subscribed(&sets, id);
    let desired = desired(&items, &mut warnings);
    let recorded = state
        .settings
        .iter()
        .find(|r| &r.registration_id == id)
        .cloned()
        .unwrap_or_else(|| SettingsRecord {
            registration_id: id.clone(),
            hooks: vec![],
            status_line: None,
            output_style: None,
        });
    if desired.is_empty() && recorded.is_empty() {
        return warnings;
    }
    let skipped = |warnings: &mut Vec<String>, reason: &str| {
        warnings.push(format!(
            "Skipped shared settings for {}: {reason}; retrying at the next launch",
            registration.name
        ));
    };
    let profile = match store.profile_directory(registration) {
        Ok(profile) => profile,
        Err(error) => {
            skipped(&mut warnings, &error.message);
            return warnings;
        }
    };
    let before = match snapshot(&profile) {
        Ok(before) => before,
        Err(error) => {
            skipped(&mut warnings, &error.message);
            return warnings;
        }
    };
    let original = match &before {
        None => Json::Object(vec![]),
        Some((_, bytes)) => match Json::parse(bytes) {
            Ok(document @ Json::Object(_)) => document,
            Ok(_) => {
                skipped(&mut warnings, "settings.json is not a JSON object");
                return warnings;
            }
            Err(_) => {
                skipped(&mut warnings, "settings.json is not valid JSON");
                return warnings;
            }
        },
    };
    let mut document = original.clone();
    let record = SettingsRecord {
        registration_id: id.clone(),
        hooks: apply_hooks(
            &mut document,
            &desired.hooks,
            &recorded.hooks,
            &mut warnings,
        ),
        status_line: apply_single(
            &mut document,
            STATUS_LINE,
            &desired.status_line,
            recorded.status_line.as_ref(),
            &mut warnings,
        ),
        output_style: apply_single(
            &mut document,
            OUTPUT_STYLE,
            &desired.output_style,
            recorded.output_style.as_ref(),
            &mut warnings,
        ),
    };
    let mut written = None;
    if document != original {
        let mut text = document.render();
        // A new file ends with a newline like the ones Claude writes; an existing
        // file keeps its own ending.
        if before.as_ref().is_none_or(|(_, b)| b.ends_with(b"\n")) {
            text.push('\n');
        }
        match replace(&profile, &before, Some(text.as_bytes())) {
            Ok(identity) => written = identity.map(|i| (i, text.into_bytes())),
            Err(error) => {
                skipped(&mut warnings, &error.message);
                return warnings;
            }
        }
    }
    if record != recorded
        && let Err(error) = store.update_state(|state| {
            state.settings.retain(|r| &r.registration_id != id);
            if !record.is_empty() {
                state.settings.push(record);
            }
            Ok(())
        })
    {
        warnings.push(format!(
            "Shared settings records not saved: {}",
            error.message
        ));
        // Unrecorded entries would become the profile's own; undo this launch's write.
        if let Some(written) = written {
            let restore = before.as_ref().map(|(_, bytes)| bytes.as_slice());
            if replace(&profile, &Some(written), restore).is_err() {
                warnings.push("settings.json keeps this launch's shared settings".into());
            }
        }
    }
    warnings
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "roost-settings-unit-{}",
                crate::platform::random_id().unwrap()
            ));
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            Self(path)
        }
        fn names(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(&self.0)
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            names
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn replace_refuses_a_settings_file_changed_since_read() {
        let temp = Temp::new();
        let file = temp.0.join(SETTINGS);
        let profile = Directory::open(&temp.0, true).unwrap();
        // Content changed in place (same object).
        fs::write(&file, "{\"a\": 1}").unwrap();
        let before = snapshot(&profile).unwrap();
        fs::write(&file, "{\"a\": 2}").unwrap();
        let error = replace(&profile, &before, Some(b"{}")).unwrap_err();
        assert!(error.message.contains("changed"), "{}", error.message);
        assert_eq!(fs::read_to_string(&file).unwrap(), "{\"a\": 2}");
        // Same content, but a different object (replaced by another writer).
        let before = snapshot(&profile).unwrap();
        fs::write(temp.0.join("other"), "{\"a\": 2}").unwrap();
        fs::rename(temp.0.join("other"), &file).unwrap();
        assert!(replace(&profile, &before, Some(b"{}")).is_err());
        // Created after an absent read.
        fs::remove_file(&file).unwrap();
        let before = snapshot(&profile).unwrap();
        assert!(before.is_none());
        fs::write(&file, "{\"b\": 1}").unwrap();
        assert!(replace(&profile, &before, Some(b"{}")).is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), "{\"b\": 1}");
        assert_eq!(temp.names(), [SETTINGS]);
        // Unchanged: replaced atomically by a new object.
        let before = snapshot(&profile).unwrap();
        let identity = replace(&profile, &before, Some(b"{}")).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "{}");
        assert_eq!(snapshot(&profile).unwrap().map(|s| s.0), identity);
        assert_eq!(temp.names(), [SETTINGS]);
    }

    /// Runs `read_fragment` on another thread, failing instead of hanging.
    fn read_within_a_second(path: PathBuf) -> std::result::Result<Fragment, String> {
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || drop(send.send(read_fragment(&path, true))));
        receive
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("fragment read must not hang")
    }

    #[test]
    fn fragment_reads_refuse_fifos_and_devices_without_hanging() {
        let temp = Temp::new();
        let fifo = temp.0.join("fifo.json");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(status.success());
        let link = temp.0.join("link.json");
        std::os::unix::fs::symlink(&fifo, &link).unwrap();
        for path in [fifo, link, PathBuf::from("/dev/zero")] {
            let error = read_within_a_second(path.clone()).unwrap_err();
            assert!(
                error.contains("regular file"),
                "{}: {error}",
                path.display()
            );
        }
        // A file over the limit is refused after reading at most limit + 1 bytes.
        let big = temp.0.join("big.json");
        fs::write(&big, vec![b' '; MAX_READ_BYTES + 2]).unwrap();
        assert!(read_within_a_second(big).unwrap_err().contains("larger"));
    }

    #[test]
    fn rendering_keeps_key_order_and_claudes_two_space_style() {
        let text = r#"{"z":1,"a":[1.5,"x\n\"",{},[]],"m":{"k":null,"b":true}}"#;
        let document = Json::parse(text.as_bytes()).unwrap();
        assert_eq!(
            document.render(),
            "{\n  \"z\": 1,\n  \"a\": [\n    1.5,\n    \"x\\n\\\"\",\n    {},\n    []\n  ],\n  \"m\": {\n    \"k\": null,\n    \"b\": true\n  }\n}"
        );
        assert!(Json::parse(br#"{"a":1,"a":2}"#).is_err());
        // Numbers keep their text; digits inside strings are not numbers.
        let document = Json::parse(br#"{"s\"-1":"a\"1e5\\","n":[2.50,-0E+1]}"#).unwrap();
        assert_eq!(
            document.render(),
            "{\n  \"s\\\"-1\": \"a\\\"1e5\\\\\",\n  \"n\": [\n    2.50,\n    -0E+1\n  ]\n}"
        );
        assert!(Json::parse(b"1e400").is_err());
    }
}
