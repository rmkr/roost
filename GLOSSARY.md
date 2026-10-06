# Claude profile management

Language for managing Claude profiles alongside existing managers and installations.

## Language

**Profile**:
A Claude configuration and state context. Multiple profiles may use the same account.
_Avoid_: Account, login

**Account**:
The external identity Claude authenticates with. A selected profile alone does not establish the effective account.
_Avoid_: Profile

**Launcher**:
A command that selects a profile and starts Claude.
_Avoid_: Installation, account

**Default alias**:
A pass-through command for ordinary Claude in the caller's environment. It does not provide a separate profile context.
_Avoid_: Isolated default profile

**Owned profile**:
An isolated profile created under this manager's ownership.
_Avoid_: Imported profile, adopted profile

**Registered upstream profile**:
An upstream-managed profile associated with this manager for launch at its existing location. Registration does not transfer ownership.
_Avoid_: Owned profile, migrated profile

**Registration**:
This manager's association of a name with a profile or default alias and its launchers.
_Avoid_: Account creation, ownership transfer

**Retained profile**:
An owned profile whose registration was removed while its data remained available for explicit reuse.
_Avoid_: Deleted profile

**Manager token**:
A subscription OAuth token stored by a profile manager for use at launch. It is distinct from Claude-managed login credentials.
_Avoid_: Login, account

**Session**:
A Claude conversation that can be started and later resumed. A profile may contain multiple sessions.
_Avoid_: Profile, account

**Manager storage**:
The manager's own records, owned profiles, and launchers. Registration does not make upstream profile data part of that ownership.
_Avoid_: User home, Claude installation

**Selected profile**:
The profile Roost launches for a project when none is named. A project without one asks the user to choose.
_Avoid_: Current profile, active profile, default profile

**Shared set**:
A named collection of skills or plugins, installed once outside any profile, that owned profiles can subscribe to.
_Avoid_: Shared profile, global config, bundle

**Plugin store**:
The manager's own single installation of shared plugins, from which shared sets draw their plugins. It belongs to no profile and no account.
_Avoid_: Shared profile, plugin cache

**Instruction fragment**:
A self-contained file of Claude instructions on one topic that a shared set can carry.
_Avoid_: Shared CLAUDE.md, snippet, partial
