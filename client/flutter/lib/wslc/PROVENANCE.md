# WSLc workspace UI

Adapted from Alexander Evsikov's WSLc Desk, commit `99550621ea72897a1f15ba867e8f316fdad16324` (AGPL-3.0-only).
The ConsoleCrypt license applies; original sources: https://repo.evsikov.net/Vibecodes/wslc-desk.
Only feature widgets, helpers, controller and JSON DTOs are imported. The old
FRB bindings, RustLib initializer, MaterialApp, hotkey scope, theme application,
tray handler and executable entry point are not used. Native operations use
ConsoleCrypt's shared core runtime and a profile-scoped native lease.
