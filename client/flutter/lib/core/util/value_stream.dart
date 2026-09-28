import 'dart:async';

/// Holds a current value and broadcasts changes. Every subscriber first
/// receives the current value, then updates — the contract all `watch*`
/// service streams follow (mirrors how app-core will push state snapshots
/// over flutter_rust_bridge `StreamSink`s).
class ValueStreamController<T> {
  ValueStreamController(this._value);

  T _value;
  final StreamController<T> _changes = StreamController<T>.broadcast();

  T get value => _value;

  set value(T next) {
    _value = next;
    if (!_changes.isClosed) {
      _changes.add(next);
    }
  }

  /// Re-emits the current value (after in-place mutation of a collection).
  void notify() => value = _value;

  Stream<T> get stream => Stream<T>.multi((controller) {
    controller.add(_value);
    final sub = _changes.stream.listen(controller.add, onError: controller.addError, onDone: controller.close);
    controller.onCancel = sub.cancel;
  });

  Future<void> close() => _changes.close();
}
