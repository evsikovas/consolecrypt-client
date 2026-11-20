/// Cleanup that must happen only AFTER native accepted an application exit.
/// In particular, a second lifecycle listener must not stop local AI while the
/// first listener is still asking whether running jobs should finish.
final class ApprovedExitActions {
  final _actions = <Object, Future<void> Function()>{};
  Future<void>? _running;

  void Function() register(Future<void> Function() action) {
    final key = Object();
    _actions[key] = action;
    return () => _actions.remove(key);
  }

  Future<void> run() => _running ??= _run().whenComplete(() => _running = null);

  Future<void> _run() async {
    for (final entry in _actions.entries.toList()) {
      if (!_actions.containsKey(entry.key)) continue;
      await entry.value();
      _actions.remove(entry.key);
    }
  }
}
