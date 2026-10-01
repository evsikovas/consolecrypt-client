import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:material_ui/material_ui.dart';

const _title = 'ConsoleCrypt could not start its secure core'; // l10n-ignore: shown before localization loads
const _body =
    'Your data was not touched. Reinstall the app or rebuild it with the Rust toolchain installed, then try again.'; // l10n-ignore: shown before localization loads

/// Shown instead of the app when the native core cannot be loaded or
/// initialized (never a silent fallback to the mock backend).
///
/// Localization is not available this early (no settings / locale yet), so
/// the texts are English. // TODO(l10n): move to ARB via
/// `lookupAppLocalizations(PlatformDispatcher.locale)`; next: add
/// `startupFailed*` keys.
class StartupErrorApp extends StatelessWidget {
  const StartupErrorApp({required this.error, super.key});

  final Object error;

  String get _details {
    final e = toAppException(error);
    return '${e.code.name}: ${e.message}';
  }

  @override
  Widget build(BuildContext context) => MaterialApp(
    debugShowCheckedModeBanner: false,
    home: Scaffold(
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 560),
          child: Padding(
            padding: const EdgeInsets.all(32),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Icon(Icons.error_outline, size: 40),
                const SizedBox(height: 16),
                const Text(_title, style: TextStyle(fontSize: 20, fontWeight: FontWeight.w600)),
                const SizedBox(height: 12),
                const Text(_body),
                const SizedBox(height: 16),
                SelectableText(_details, style: const TextStyle(fontFamily: 'monospace', fontSize: 12)),
              ],
            ),
          ),
        ),
      ),
    ),
  );
}
