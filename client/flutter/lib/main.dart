import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/core/bridge/rust_app_services.dart';
import 'package:consolecrypt/core/bridge/startup_error_app.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Entry point. Loads the Rust core (flutter_rust_bridge, ADR-0101 §8),
/// initializes app-core and reopens the last profile (locked), then starts
/// the UI on [RustAppServices] (`appServicesProvider`). If the native library
/// cannot be loaded or the core fails to start, a clear error screen is
/// shown — never a silent fallback to mocks.
///
/// `flutter run --dart-define=CC_MOCK=true` runs on the in-memory mock
/// backend instead.
Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  if (!useMockBackend) {
    try {
      await RustAppServices.open();
    } on Object catch (error) {
      runApp(StartupErrorApp(error: error));
      return;
    }
  }
  runApp(const ProviderScope(child: ConsoleCryptApp()));
}
