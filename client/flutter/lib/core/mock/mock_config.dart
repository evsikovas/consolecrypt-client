import 'dart:async';

/// Timing and seeding knobs of the in-memory backend.
///
/// Tests use [MockConfig.test] (all durations zero → only microtasks, so no
/// timers are left pending when a widget test ends).
final class MockConfig {
  const MockConfig({
    required this.latency,
    required this.kdfLatency,
    required this.streamStep,
    required this.transferTick,
    this.autoApproveAfter,
    this.seedDemoData = true,
    this.seedEditLeftovers = false,
  });

  /// Realistic delays for running the app on mocks.
  const MockConfig.demo()
    : latency = const Duration(milliseconds: 280),
      kdfLatency = const Duration(milliseconds: 700),
      streamStep = const Duration(milliseconds: 28),
      transferTick = const Duration(milliseconds: 150),
      autoApproveAfter = null,
      seedDemoData = true,
      seedEditLeftovers = true;

  /// Zero latency, deterministic, seeded.
  const MockConfig.test({this.seedDemoData = true, this.seedEditLeftovers = false})
    : latency = Duration.zero,
      kdfLatency = Duration.zero,
      streamStep = Duration.zero,
      transferTick = Duration.zero,
      autoApproveAfter = null;

  /// Network round trip.
  final Duration latency;

  /// Argon2id unlock / key generation.
  final Duration kdfLatency;

  /// Delay between streamed AI tokens.
  final Duration streamStep;

  /// Delay between transfer progress updates.
  final Duration transferTick;

  /// If set, a pending approval request of this device is approved
  /// automatically after this delay (demo of the waiting screen).
  final Duration? autoApproveAfter;

  final bool seedDemoData;

  /// Pretend the last run left SFTP edit sessions behind ("Recover unsaved
  /// edits?" after unlock). Demo only; tests opt in.
  final bool seedEditLeftovers;
}

/// `Future.delayed` that degrades to a microtask for zero durations.
Future<void> mockDelay(Duration duration) =>
    duration == Duration.zero ? Future<void>.value() : Future<void>.delayed(duration);
