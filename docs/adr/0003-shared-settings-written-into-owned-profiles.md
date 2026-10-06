---
status: accepted
---

# Shared settings are written into owned profiles' settings.json

Settings fragments (hooks, `statusLine`, `outputStyle`) from a profile's subscribed sets are merged by Roost into that owned profile's own `settings.json` at launch, editing only the entries Roost recorded, and only when the desired result changed. The cleaner route, a generated file passed with `claude --settings`, was rejected because Claude Desktop's Code tab already occupies the single flag-settings tier and reads only the profile's own `settings.json`, so shared hooks would never reach Desktop sessions; the flag tier would also hide later `/config` changes behind shared single-value keys, and a user's own `--settings` would replace Roost's silently.

## Consequences

- Roost co-edits a file Claude and the user also write. It touches only entries it recorded (and only while they are unchanged), preserves all other keys and their order, and replaces the file atomically only if it did not change since read; otherwise it skips with a warning and retries at the next launch, never blocking it.
- Hooks are re-added if removed by hand (the set is the source of truth); `statusLine`/`outputStyle` back off when the profile sets its own value, so `/config` choices win.
- Registered upstream profiles receive no shared settings, because Roost never writes borrowed data.
