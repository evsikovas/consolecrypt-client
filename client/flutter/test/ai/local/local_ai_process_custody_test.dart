import 'dart:async';
import 'dart:io';

import 'package:consolecrypt/ai/local/local_ai_runtime.dart';
import 'package:consolecrypt/ai/local/src/local_ai_process.dart';
import 'package:flutter_test/flutter_test.dart';

class HeldChild extends Fake implements Process {
  final ended = Completer<int>();
  final signals = <ProcessSignal>[];
  void Function(ProcessSignal)? onKill;
  @override
  Future<int> get exitCode => ended.future;
  @override
  bool kill([ProcessSignal signal = ProcessSignal.sigterm]) {
    signals.add(signal);
    onKill?.call(signal);
    return false; // A kill request is never itself proof of exit.
  }
}

class DisposalRuntime extends LocalAiRuntime {
  DisposalRuntime(Directory root) : super(supportDirectory: root, platform: LocalAiPlatform.unsupported);
  bool fail = true;
  int stops = 0;
  Completer<void>? pending;
  @override
  Future<void> stop() {
    stops++;
    if (fail) return Future.error(const LocalAiException('runtime_stop_failed'));
    return pending?.future ?? Future.value();
  }
}

LocalAiOwnedProcess attach(
  LocalAiProcessCustody owner,
  HeldChild child, {
  void Function()? onExit,
  void Function()? onUnknown,
}) => owner.attach(
  child,
  onExit: onExit ?? () {},
  onUnknownExit: onUnknown ?? () {},
  grace: const Duration(milliseconds: 10),
  forceGrace: const Duration(milliseconds: 10),
);

void main() {
  test('real inherited dispose retries failed cleanup and coalesces until actual stop completion', () async {
    final root = await Directory.systemTemp.createTemp('cc-owned-dispose-');
    final runtime = DisposalRuntime(root);
    try {
      await expectLater(runtime.dispose(), throwsA(isA<LocalAiException>()));
      await expectLater(runtime.dispose(), throwsA(isA<LocalAiException>()));
      expect(runtime.stops, 2);
      expect(runtime.ensureReady, throwsA(isA<LocalAiException>()));
      runtime.fail = false;
      runtime.pending = Completer<void>();
      final first = runtime.dispose();
      final second = runtime.dispose();
      expect(identical(first, second), isTrue);
      var completed = false;
      unawaited(first.then((_) => completed = true));
      await Future<void>.delayed(Duration.zero);
      expect(completed, isFalse);
      runtime.pending!.complete();
      await Future.wait([first, second]);
      expect(completed, isTrue);
      expect(runtime.stops, 3);
      await runtime.dispose();
      expect(runtime.stops, 3);
    } finally {
      if (runtime.pending?.isCompleted == false) runtime.pending!.complete();
      await root.delete(recursive: true);
    }
  });

  for (final exitError in [false, true]) {
    test('late successful health cannot revive ${exitError ? 'unknown' : 'exited'} original child', () async {
      final owner = LocalAiProcessCustody();
      final child = HeldChild();
      final owned = attach(owner, child);
      expect(owner.isCurrentUsable(owned), isTrue);
      final health = Completer<bool>();
      var ready = false;
      final response = () async {
        if (await health.future) {
          // SAME production admission predicate after awaited health I/O.
          if (!owner.isCurrentUsable(owned)) return;
          ready = true;
        }
      }();
      if (exitError) {
        child.ended.completeError(StateError('owned exit observation error'));
      } else {
        child.ended.complete(0);
      }
      await Future<void>.delayed(Duration.zero);
      health.complete(true);
      await response;
      expect(ready, isFalse);
      expect(owner.isCurrentUsable(owned), isFalse);
      expect(owner.hasUnjoinedProcess, exitError);
    });
  }

  test('failed graceful and forced kill retain actual child and deny replacement until observed exit', () async {
    final owner = LocalAiProcessCustody();
    final child = HeldChild();
    var exits = 0;
    attach(owner, child, onExit: () => exits++);
    await expectLater(owner.stopAfter(null), throwsA(isA<LocalAiProcessStopFailed>()));
    expect(child.signals, [ProcessSignal.sigterm, ProcessSignal.sigkill]);
    expect(owner.hasUnjoinedProcess, isTrue);
    expect(exits, 0);
    expect(owner.requireStartable, throwsA(isA<LocalAiProcessStopFailed>()));
    child.ended.complete(-1);
    await child.exitCode;
    await Future<void>.delayed(Duration.zero);
    expect(owner.hasUnjoinedProcess, isFalse);
    expect(exits, 1);
    owner.requireStartable();
  });

  test('concurrent stop waits same child once and does not treat kill acknowledgement as joined', () async {
    final owner = LocalAiProcessCustody();
    final child = HeldChild();
    attach(owner, child);
    final first = owner.stopAfter(null);
    final second = owner.stopAfter(null);
    await Future<void>.delayed(Duration.zero);
    expect(owner.hasUnjoinedProcess, isTrue);
    expect(child.signals, [ProcessSignal.sigterm]);
    child.ended.complete(0);
    await Future.wait([first, second]);
    expect(owner.hasUnjoinedProcess, isFalse);
  });

  test('late original Process.start acknowledgement is joined before stop completes', () async {
    final owner = LocalAiProcessCustody();
    final startup = Completer<void>();
    var stopped = false;
    final stopping = owner.stopAfter(startup.future).then((_) => stopped = true);
    await Future<void>.delayed(Duration.zero);
    expect(stopped, isFalse);
    final child = HeldChild();
    child.onKill = (_) {
      if (!child.ended.isCompleted) child.ended.complete(-1);
    };
    attach(owner, child);
    startup.complete();
    await stopping;
    expect(child.signals, [ProcessSignal.sigterm]);
    expect(owner.hasUnjoinedProcess, isFalse);
  });

  test('failed original startup cleanup cannot become successful stopped state', () async {
    final owner = LocalAiProcessCustody();
    final startup = Completer<void>();
    final stopping = expectLater(owner.stopAfter(startup.future), throwsA(isA<LocalAiProcessStopFailed>()));
    final child = HeldChild();
    final owned = attach(owner, child);
    await expectLater(owner.retire(owned), throwsA(isA<TimeoutException>()));
    startup.completeError(StateError('owned startup cleanup failed'));
    await stopping;
    expect(owner.hasUnjoinedProcess, isTrue);
    expect(owner.requireStartable, throwsA(isA<LocalAiProcessStopFailed>()));
    child.ended.complete(-1);
    await child.exitCode;
    await Future<void>.delayed(Duration.zero);
    await owner.stopAfter(null);
    expect(owner.hasUnjoinedProcess, isFalse);
  });

  test('exit observation error retains unknown process and never certifies a new start', () async {
    final owner = LocalAiProcessCustody();
    final child = HeldChild();
    var unknown = 0;
    var exits = 0;
    attach(owner, child, onUnknown: () => unknown++, onExit: () => exits++);
    child.ended.completeError(StateError('owned exit observation unavailable'));
    await Future<void>.delayed(Duration.zero);
    expect(unknown, 1);
    expect(exits, 0);
    expect(owner.hasUnjoinedProcess, isTrue);
    await expectLater(owner.stopAfter(null), throwsA(isA<LocalAiProcessStopFailed>()));
    expect(owner.requireStartable, throwsA(isA<LocalAiProcessStopFailed>()));
  });

  test('old completed retirement cannot release a replacement child slot', () async {
    final owner = LocalAiProcessCustody();
    final first = HeldChild();
    final owned = attach(owner, first);
    first.ended.complete(0);
    await first.exitCode;
    await Future<void>.delayed(Duration.zero);
    final second = HeldChild();
    attach(owner, second);
    await owner.retire(owned);
    expect(owner.hasUnjoinedProcess, isTrue);
    expect(owner.requireStartable, throwsA(isA<LocalAiProcessStopFailed>()));
    second.ended.complete(0);
    await second.exitCode;
    await Future<void>.delayed(Duration.zero);
  });

  test('actual owned finite child exits and joins through production custody', () async {
    final String executable;
    final List<String> args;
    if (Platform.isWindows) {
      executable =
          '${Platform.environment['SystemRoot'] ?? r'C:\Windows'}/System32/WindowsPowerShell/v1.0/powershell.exe';
      args = ['-NoLogo', '-NoProfile', '-NonInteractive', '-Command', 'Start-Sleep -Seconds 30'];
    } else {
      executable = '/bin/sleep';
      args = ['30'];
    }
    final child = await Process.start(executable, args, runInShell: false);
    unawaited(child.stdout.drain<void>());
    unawaited(child.stderr.drain<void>());
    final owner = LocalAiProcessCustody();
    var exited = false;
    owner.attach(child, onExit: () => exited = true, onUnknownExit: () => fail('real owned exit unavailable'));
    try {
      await owner.stopAfter(null).timeout(const Duration(seconds: 10));
      await child.exitCode;
      expect(exited, isTrue);
      expect(owner.hasUnjoinedProcess, isFalse);
      owner.requireStartable();
    } finally {
      child.kill(ProcessSignal.sigkill);
      await child.exitCode.timeout(const Duration(seconds: 5));
    }
  });
}
