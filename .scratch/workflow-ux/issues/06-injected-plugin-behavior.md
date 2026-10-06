# Investigate injected plugin behavior

Type: research
Labels: workflow-ux
Status: open
Assignee: unassigned
Parent: [Make Roost simpler to work with day to day](../map.md)
Blocked by: none

## Question

Before the plugin store is built, settle what ADR 0001 leaves open, using disposable config directories only: how `name@inline` plugins from `CLAUDE_CODE_PLUGIN_DIRS` interact with a profile's own `enabledPlugins` and same-named native installs; where plugin user config and `${CLAUDE_PLUGIN_DATA}` live for injected plugins; which directory inside the store to inject per plugin and how that survives `claude plugin update` and the 14-day orphan sweep; whether claude.ai organization marketplaces work in an account-less store. Record findings in `../research/` and update the ADR's status.

## Comments
