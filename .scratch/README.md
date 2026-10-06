# Local Markdown tracker

Each effort has a `map.md` and numbered tickets in `issues/`. The [Rust port decision map](rust-port/map.md) is complete; its accepted handoff is the [Roost specification](rust-port/spec.md). The [workflow UX map](workflow-ux/map.md) is open again for shared agents, commands, styles and settings.

## Wayfinding operations

- Identity: a ticket's two-digit filename prefix; reference tickets by linked title in prose.
- Metadata: `Type`, `Labels`, `Status`, `Assignee`, `Parent`, and `Blocked by` lines near the top. Types are `research`, `prototype`, `grilling`, or `task`.
- Status: `open` is unclaimed, `claimed` is in progress, and `resolved` is closed. An unclaimed ticket has `Assignee: unassigned`.
- Blocking: `Blocked by: 01, 02` means both tickets must be resolved; `none` has no dependencies.
- Frontier: inspect the effort's `issues/` files; choose the lowest numbered open, unassigned ticket whose blockers are resolved.
- Claim: set the assignee and `Status: claimed` before work. Recheck the current file before saving; another session may have claimed it.
- Resolve: append a dated resolution under `## Answer`, set `Status: resolved`, and add a linked, one-line gist to the map's `Decisions so far`. Keep the detailed answer in the ticket and link separate research assets.
- Conversation: append under `## Comments`; keep the original question intact.
- Dependencies: create new tickets first, then wire their blockers. Move newly specified questions out of the map's fog section.

Wayfinder sessions resolve at most one decision ticket; research tickets may run in parallel. Human decisions require the human's answers. The map is complete when its destination is satisfied, every in-scope ticket is resolved, and no in-scope fog remains. The final specification belongs at `spec.md` within the effort directory.
