---
status: accepted
---

# Claude subcommands run from roost by a fixed allowlist

`roost agents …` and the other subcommands in `CLAUDE_SUBCOMMANDS` (`src/cli.rs`) mean exactly `roost -- agents …`: they launch this project's selected profile, or open the picker when none is selected, with a full launch's side effects. Only a fixed list is forwarded. Forwarding every word Roost doesn't know was rejected because Claude reads an unknown word as a prompt, so a typo such as `roost lsit` would start an interactive session instead of failing.

## Consequences

- A new Claude subcommand needs a Roost change before `roost <it>` works; until then `roost -- <it>` and `roost run NAME <it>` reach it.
- Roost's own commands win a name clash: `doctor`, `plugin`, `update` and `rm` stay Roost's (`roost rm work` removes a profile, not a background session). Claude's versions are reachable with `--`.
- `install`, `gateway` and `purge` are left out: `install` acts on the shared installation, `gateway` is niche, and `purge` is too easily confused with `roost remove --purge`.
- A profile name is never accepted as the first word (`roost personal agents`); `roost run personal agents` and the generated `personal` launcher cover that.
- These commands finish without showing a session, so Roost names the profile on stderr (`roost: using profile NAME`). A wrong selection would otherwise act silently, for example `roost auth logout` in the wrong repository.
