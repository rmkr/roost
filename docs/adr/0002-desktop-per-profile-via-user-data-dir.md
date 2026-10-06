---
status: accepted
---

# Claude Desktop per profile via Electron's `--user-data-dir`

`roost desktop NAME` launches Claude Desktop with `--user-data-dir` pointing at a Roost-owned data folder for that profile (outside the profile's config directory, following the registration's lifecycle) and `CLAUDE_CONFIG_DIR` set to the profile. Desktop's sign-in lives in its Electron data folder rather than in `CLAUDE_CONFIG_DIR`, and Desktop passes its own account to Code-tab sessions, so this is the only way to give each profile its own Desktop account. The flag is undocumented and Anthropic deliberately ignores the `CLAUDE_USER_DATA_DIR` equivalent in signed builds, so a Desktop update may break it at any time; the command is marked experimental and `doctor` checks that the data folder really moved.

## Consequences

- Verified on Linux, Desktop 2.19675.1: a separate data folder gets its own lock and asks for its own sign-in, and two such instances run at the same time.
- Cowork's helper socket (`/run/user/<uid>/claude-cowork-vm.sock`) is fixed, so only the first running instance owns it. Cowork in a second concurrent instance is untested, and Roost warns when another Desktop is running.
- Launching the same profile's Desktop while it is already running only focuses the existing window and ignores the new environment, so Roost refuses with a message instead.
- The default alias launches plain `claude-desktop` with its usual data folder, unchanged.
- Amended 2026-10-06: one owned or upstream profile may instead borrow that conventional folder in place (`roost desktop --link NAME`), so the already signed-in Desktop opens with that profile's configuration. Linked launches omit `--user-data-dir`. Roost never copies or moves the folder: a copy would duplicate Desktop's sign-in, which Roost must not extract, and the copies would fight over one refresh token. The default alias then shares that instance, so launching either while it runs is refused. Remove and purge drop the link and never touch the borrowed folder; `--link --replace` deletes the profile's own Roost folder first, after confirmation.
