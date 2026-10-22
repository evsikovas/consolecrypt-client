import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

/// Notifications use the glass toast (`showSnack` in
/// `lib/core/widgets/dialogs.dart`, LIQUID_GLASS_SPEC §4.11), never a raw
/// Material snackbar: those ignore the appearance settings and render as a
/// white Material bar.
void main() {
  test('no raw Material snackbars in lib/', () {
    final raw = RegExp(r'\bSnackBar\(|\bshowSnackBar\(|\bScaffoldMessenger\b');
    final offenders = <String>[];
    for (final file in Directory('lib').listSync(recursive: true).whereType<File>()) {
      if (!file.path.endsWith('.dart') || file.path.contains('/l10n/') || file.path.contains('/src/rust/')) continue;
      final lines = file.readAsLinesSync();
      for (var i = 0; i < lines.length; i++) {
        final line = lines[i].trimLeft();
        if (line.startsWith('//')) continue;
        if (raw.hasMatch(line)) offenders.add('${file.path}:${i + 1}: ${line.trim()}');
      }
    }
    expect(offenders, isEmpty, reason: 'Use showSnack(context, message, error: …) instead:\n${offenders.join('\n')}');
  });
}
