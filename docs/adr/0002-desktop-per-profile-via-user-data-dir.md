---
status: accepted
---

# Claude Desktop per profile via Electron's `--user-data-dir`

`roost desktop NAME` launches Claude Desktop with `--user-data-dir` pointing at a Roost-owned data folder for that profile (outside the profile's config directory, following the registration's lifecycle) and `CLAUDE_CONFIG_DIR` set to the profile. Desktop's sign-in lives in its Electron data folder rather than in `CLAUDE_CONFIG_DIR`, and Desktop passes its own account to Code-tab sessions, so this is the only way to give each profile its own Desktop account. The flag is undocumented and Anthropic deliberately ignores the `CLAUDE_USER_DATA_DIR` equivalent in signed builds, so a Desktop update may break it at any time; the command is marked experimental and `doctor` checks that the data folder really moved.

## Consequences

- Verified on Linux, Desktop 2.19675.1: a separate data folder gets its own lock and asks for its own sign-in, and two such instances run at the same time.
- Cowork's helper (`cowork-linux-helper`, socket `/run/user/<uid>/claude-cowork-vm.sock`) is per user, not per data folder. Verified 2026-10-06 with `work` borrowing the conventional folder and `personal` on its own folder: the helper is started on demand by whichever running Desktop uses Cowork first (here the instance launched second), Cowork works there, and the other instance cannot use Cowork until both are closed (or force-quit) and reopened. Chat and the Code tab work in both. Roost warns about this whenever another Desktop is running.
- Layout differences seen between profiles' Desktops (for example a Chat/Cowork switch in one account but not another) follow the signed-in account, not Roost: the same profile looked identical running alone.
- Launching the same profile's Desktop while it is already running only focuses the existing window and ignores the new environment, so Roost refuses with a message instead.
- The default alias launches the installed Desktop executable with its usual data folder, unchanged.
- Amended 2026-10-06: one owned or upstream profile may instead borrow that conventional folder in place (`roost desktop --link NAME`), so the already signed-in Desktop opens with that profile's configuration. Linked launches omit `--user-data-dir`. Roost never copies or moves the folder: a copy would duplicate Desktop's sign-in, which Roost must not extract, and the copies would fight over one refresh token. The default alias then shares that instance, so launching either while it runs is refused. Remove and purge drop the link and never touch the borrowed folder; `--link --replace` deletes the profile's own Roost folder first, after confirmation.

## macOS amendment (2026-10-22)

Roost also launches Desktop profiles on macOS, directly executing `Contents/MacOS/Claude` inside `~/Applications/Claude.app` or `/Applications/Claude.app`. Direct execution passes the profile environment and arguments to a new process, bypassing Launch Services' reuse of an existing application instance. Roost does not copy, patch or re-sign Claude.app. `ROOST_DESKTOP_APP` selects an explicit absolute executable or app-bundle path, with no fallback on an invalid override. Linux retains PATH discovery for `claude-desktop`.

The conventional macOS data folder for `--link` and default aliases is `~/Library/Application Support/Claude`; XDG_CONFIG_HOME does not apply. The same per-registration folders, launch journal, Electron SingletonLock checks and purge protections apply on both platforms. Electron's [startup implementation](https://github.com/electron/electron/blob/main/shell/app/electron_main_delegate.cc) reads `--user-data-dir` on macOS as well as Linux. This remains an undocumented Claude interface.

The Linux Cowork helper observations above are not evidence of macOS behavior. macOS launch, environment forwarding, folder reuse, linking and running-instance protection are covered by disposable fake-app tests. The initial macOS multi-account workflow was reported working in manual use. Broader real-account authentication callbacks, Keychain behavior, and concurrent Cowork/computer use still require acceptance testing.

`--close` and the picker's `x` action remain Linux-only. Linux verifies the executable and signals a pinned pidfd. Until an equivalent macOS implementation exists, a live macOS instance must be quit from its own window or menu; Roost never sends a signal to a bare PID from an Electron lock or quits all instances by shared bundle ID.
