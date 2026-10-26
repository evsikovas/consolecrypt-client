import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/storage/app_storage_paths.dart';
import 'package:material_ui/material_ui.dart';

/// Shown instead of the app when the native core cannot be loaded or
/// initialized (never a silent fallback to the mock backend). Settings are not
/// readable yet, so the texts follow the system language (Russian or English).
class StartupErrorApp extends StatelessWidget {
  const StartupErrorApp({required this.error, super.key});

  final Object error;

  String get _details {
    if (error case AppStorageException(:final message)) return message;
    final e = toAppException(error);
    return '${e.code.name}: ${e.message}';
  }

  @override
  Widget build(BuildContext context) => MaterialApp(
    debugShowCheckedModeBanner: false,
    localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
    supportedLocales: AppLocalizations.supportedLocales,
    home: Builder(builder: _page),
  );

  Widget _page(BuildContext context) {
    final l10n = context.l10n;
    return Scaffold(
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
                Text(l10n.startupFailedTitle, style: const TextStyle(fontSize: 20, fontWeight: FontWeight.w600)),
                const SizedBox(height: 12),
                Text(error is AppStorageException ? l10n.startupFailedStorageBody : l10n.startupFailedBody),
                const SizedBox(height: 16),
                SelectableText(_details, style: const TextStyle(fontFamily: 'monospace', fontSize: 12)),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
