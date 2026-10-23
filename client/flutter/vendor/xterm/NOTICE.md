# xterm.dart 4.0.0

This directory contains the library sources, package metadata and MIT license
from the published xterm 4.0.0 package by TerminalStudio / xuty.

- Published source: https://pub.dev/packages/xterm/versions/4.0.0
- Upstream repository: https://github.com/TerminalStudio/xterm.dart
- Copyright and license: see `LICENSE`; the upstream MIT grant is preserved.

ConsoleCrypt maintains two narrow patches. The parser patch is in
`lib/src/core/escape/parser.dart`: check the number of parameters before reading
SGR 38/48 extended foreground/background colors, and accept only 0..255 RGB
channels and indexed palette values. An incomplete or out-of-range color
directive is ignored, keeping earlier valid styling and later terminal text
usable. Valid sequences retain the upstream behavior. This avoids parser and
palette renderer range exceptions from untrusted terminal output without a
blanket exception handler or modification of the Flutter/Dart package cache.

A second patch routes the copy shortcut through the application:
`lib/src/ui/shortcut/actions.dart` and `lib/src/terminal_view.dart` take an
`onCopy` callback, and the package no longer writes the clipboard itself. The
application sends the selection through its protected clipboard (kept out of
clipboard history and other devices; a detected secret is auto-cleared), the
same path as the terminal context menu.

Regression tests live in the application at
`test/unit/terminal_output_test.dart` and
`test/unit/clipboard_routing_test.dart`. All other library files are copied
unchanged from the published package. When replacing this fork with an upstream
release, retain these tests and check the output scanner's xterm-specific CSI,
OSC and charset token boundary rules as well.
