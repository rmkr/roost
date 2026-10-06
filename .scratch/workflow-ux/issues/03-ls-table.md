# Render `roost ls` as a worktrunk-style table

Type: task
Labels: workflow-ux
Status: claimed
Assignee: Claude subagent ticket-03
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: 01, 10

## Question

Implement the table and `--full` from [design: roost ls](../design.md#roost-ls): aligned columns, color only on a terminal, summary line naming hidden columns, plain output when piped, `--json` unchanged apart from spec-approved fields. `--full` runs bounded, allowlisted status probes in parallel. Leave the `@`/`^` markers, Last used and Sets columns to their own tickets, and design the renderer so those columns slot in.

## Comments

- 2026-10-06 (Claude subagent ticket-03): Implemented on branch `ticket/03-ls-table`. `ls` table columns Profile, Kind, Token (`✓`/`–`/`?`), Launchers (`ready` or worst required condition; `not_required` when none required); `--full` adds Login (`yes`/`no`), Auth, Claude dir (`—` not probed, `?` failed/unreported). Summary `○ N profiles · M upstream · hidden: Path[, …]`; `Table::width` hides columns from the right to fit `platform::stdout_width()` (terminal only; piped output keeps every column). `--full` prepares probes under the lock, drops the Store, then runs `launch::probe_all` in parallel threads; failures leave probe fields null with a `NAME: …` warning, exit 0. JSON: `data.project` and ProfileRecord `sets`/`last_used`/`selected`/`most_recent` are spec-shaped placeholders (`null`, `[]`, `false`) in `Store::profiles()` for 04/05 to fill; `probe` is null unless `--full`. For 04/05: add the marker via `.marker(...)` and Sets/Last used via `.column(...)` between Launchers and the `if full` block in `table::profiles`; append the `@`/`^` legend to the summary text. Not resolved: awaiting review.
