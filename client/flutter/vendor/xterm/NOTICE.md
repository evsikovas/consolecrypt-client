# xterm.dart 4.0.0

This directory contains the library sources, package metadata and MIT license
from the published xterm 4.0.0 package by TerminalStudio / xuty.

- Published source: https://pub.dev/packages/xterm/versions/4.0.0
- Upstream repository: https://github.com/TerminalStudio/xterm.dart
- Copyright and license: see `LICENSE`; the upstream MIT grant is preserved.

ConsoleCrypt maintains one narrow parser patch in
`lib/src/core/escape/parser.dart`: check the number of parameters before reading
SGR 38/48 extended foreground/background colors, and accept only 0..255 RGB
channels and indexed palette values. An incomplete or out-of-range color
directive is ignored, keeping earlier valid styling and later terminal text
usable. Valid sequences retain the upstream behavior. This avoids parser and
palette renderer range exceptions from untrusted terminal output without a
blanket exception handler or modification of the Flutter/Dart package cache.

Regression tests live in the application at
`test/unit/terminal_output_test.dart`. All other library files are copied
unchanged from the published package. When replacing this fork with an upstream
release, retain these tests and check the output scanner's xterm-specific CSI,
OSC and charset token boundary rules as well.
