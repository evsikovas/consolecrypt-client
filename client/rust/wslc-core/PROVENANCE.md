# WSL Containers integration

Imported from the user's WSLc Desk 5.2.0 source, commit
`99550621ea72897a1f15ba867e8f316fdad16324`:
<https://repo.evsikov.net/Vibecodes/wslc-desk>.

The original AGPL-3.0-only license is preserved in LICENSE. This is part of the
ConsoleCrypt application, not a second executable. The old generated FFI code
is not included or loaded. ConsoleCrypt owns the bridge, profile/lock lifecycle,
encrypted settings, UI theme, notifications and clipboard policy.

The original command validation, full resource IDs, bounded processes, mutation
verification, version-specific parsers, Compose planner and fixture tests are
retained. YAML parsing uses the maintained YAML organization `yaml_serde` fork.
Never automatically update the user's WSL installation or start/delete their
containers as a discovery step.
