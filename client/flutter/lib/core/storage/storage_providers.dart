import 'package:consolecrypt/core/bridge/rust_app_services.dart';
import 'package:consolecrypt/core/storage/app_storage_paths.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// The exact paths selected before native startup, shared with local AI.
/// Tests can override this without inspecting or creating real user storage.
final appStoragePathsProvider = Provider<AppStoragePaths>(
  (ref) => RustAppServices.current?.storagePaths ?? const AppStoragePaths(),
);
