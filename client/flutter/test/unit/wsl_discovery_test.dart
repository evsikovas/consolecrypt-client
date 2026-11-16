import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  const codes = <String, WslDiscoveryStatus>{
    'executable_missing': WslDiscoveryStatus.executableMissing, 'spawn_failed': WslDiscoveryStatus.spawnFailed,
    'timed_out': WslDiscoveryStatus.timedOut, 'io_failed': WslDiscoveryStatus.ioFailed,
    'output_too_large': WslDiscoveryStatus.outputTooLarge, 'nonzero_exit': WslDiscoveryStatus.nonzeroExit,
    'empty': WslDiscoveryStatus.empty, 'available': WslDiscoveryStatus.available,
  };
  test('WSL discovery parses exact Native status and preserves existing local shell data', () {
    for (final entry in codes.entries) {
      final value = LocalShellProfiles.fromJson({'wsl_discovery': entry.key, 'unavailable_reason': null,
        'profiles': [{'id': 'windows:cmd', 'kind': 'cmd', 'label': 'Command Prompt', 'shell': 'cmd', 'distribution': null}]});
      expect(value.wslDiscovery, entry.value); expect(value.profiles.single.id, 'windows:cmd');
      expect(value.unavailableReason, isNull);
    }
  });
  test('WSL legacy null future and malformed status do not imply missing executable or empty distributions', () {
    for (final value in [null, 'future_state', 'read_failed', 'EMPTY', 0, true, <String, Object?>{}]) {
      final parsed = LocalShellProfiles.fromJson({'wsl_discovery': value, 'profiles': <Object?>[],
        'unavailable_reason': 'unsupported_platform'});
      expect(parsed.wslDiscovery, isNull); expect(parsed.unavailableReason, 'unsupported_platform');
    }
    expect(LocalShellProfiles.fromJson(<String, dynamic>{}).wslDiscovery, isNull);
  });
}
