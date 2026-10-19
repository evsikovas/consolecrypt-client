# portable-pty Windows cursor inheritance patch

Source: official crates.io `portable-pty` 0.9.0 archive:
https://static.crates.io/crates/portable-pty/portable-pty-0.9.0.crate

Archive SHA-256: `b4a596a2b3d2752d94f51fac2d4a96737b8705dddd311a32b9af47211f08671e`.
The archive matches the checksum previously pinned in ConsoleCrypt's Cargo.lock.
Upstream commit: `f8921727a11b9f8b073e8c24821d72fd41283500`, directory `pty`.
License: MIT, retained verbatim in `portable-pty/LICENSE.md`.

The sole source change removes `PSEUDOCONSOLE_INHERIT_CURSOR` from the flags
passed to Windows `CreatePseudoConsole` in `src/win/psuedocon.rs`. ConsoleCrypt
opens a new empty terminal and does not inherit a parent console's cursor.
The resize quirk and Win32 input flags are unchanged. Unix code, process
creation, handle ownership, I/O and termination code are unchanged. All other
published files are retained unchanged except the omitted Cargo VCS metadata;
`Cargo.toml.orig` retains the original source manifest.

Microsoft documents that the inheritance flag requires a cursor-position
query/response and can deadlock resize or close if no terminal responds:
https://learn.microsoft.com/en-us/windows/console/createpseudoconsole
https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session

The Windows diagnostic run 39328314055 demonstrated this boundary: PowerShell
and cmd round trips passed with a terminal responder, but closing after the
initial unanswered query with a queued resize exceeded the 3-second deadline.
The regression must also pass without a terminal reader or responder after
this patch. Remove this vendored patch when upstream exposes equivalent
cursor-inheritance configuration suitable for a fresh terminal.
