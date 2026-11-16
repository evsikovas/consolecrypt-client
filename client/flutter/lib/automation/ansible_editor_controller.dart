import 'dart:convert';

import 'package:consolecrypt/automation/ansible_editor_folding.dart';
import 'package:consolecrypt/automation/ansible_editor_syntax.dart';
import 'package:flutter/widgets.dart';

class _History {
  _History(this.value, Set<int> folds) : folds = Set.of(folds);
  final TextEditingValue value;
  final Set<int> folds;
}

/// The supplied controller is the only canonical document. Projection markers
/// and fold operations never become source/save bytes. Dispose on mask/source
/// controller identity changes; no history survives that security boundary.
class AnsibleEditorDocument extends ChangeNotifier {
  AnsibleEditorDocument(TextEditingController source, {this.readOnly = false, this.onChanged}) : _source = source {
    _lastSource = source.value;
    projection = AnsibleProjection(source.text, const [], const {});
    _rebuild();
    source.addListener(_external);
    display.addListener(_displayChanged);
  }
  TextEditingController? _source;
  TextEditingController get source => _source!;
  final display = AnsibleSyntaxController();
  final collapsed = <int>{};
  final _undo = <_History>[], _redo = <_History>[];
  late TextEditingValue _lastSource;
  late TextEditingValue _lastDisplay;
  late AnsibleProjection projection;
  List<AnsibleFoldRegion> regions = const [];
  bool readOnly, _writing = false, _disposed = false;
  bool get disposed => _disposed;
  int revision = 0;
  bool editRejected = false;
  ValueChanged<String>? onChanged;
  bool get canUndo => !readOnly && _undo.isNotEmpty;
  bool get canRedo => !readOnly && _redo.isNotEmpty;
  void _remember(List<_History> history, _History value) {
    history.add(value);
    var size = history.fold<int>(0, (sum, item) => sum + item.value.text.length);
    while (history.length > 100 || size > 2 * 1024 * 1024) {
      size -= history.removeAt(0).value.text.length;
    }
  }

  void _external() {
    if (_writing || _disposed) return;
    if (source.text != _lastSource.text) {
      revision++;
      if (source.text.isEmpty) {
        _undo.clear();
        _redo.clear();
      } else {
        _remember(_undo, _History(_lastSource, collapsed));
        _redo.clear();
      }
      collapsed.clear();
    } else {
      for (final region in regions) {
        if (source.selection.isValid &&
            (source.selection.start >= region.hiddenStart && source.selection.start < region.hiddenEnd)) {
          collapsed.remove(region.headerOffset);
        }
      }
    }
    _lastSource = source.value;
    _rebuild();
    notifyListeners();
  }

  TextSelection get selectedSource {
    final selection = display.selection;
    if (!selection.isValid) return const TextSelection.collapsed(offset: 0);
    if (selection.start == 0 && selection.end == display.text.length && !selection.isCollapsed) {
      return TextSelection(
        baseOffset: selection.baseOffset == 0 ? 0 : source.text.length,
        extentOffset: selection.extentOffset == 0 ? 0 : source.text.length,
      );
    }
    final start = projection.toSource(selection.start), end = projection.toSource(selection.end, downstream: true);
    return TextSelection(
      baseOffset: selection.baseOffset <= selection.extentOffset ? start : end,
      extentOffset: selection.baseOffset <= selection.extentOffset ? end : start,
    );
  }

  String get selectedText {
    if (_disposed) return '';
    final selection = selectedSource;
    return source.text.substring(selection.start, selection.end);
  }

  void selectAll() {
    display.selection = TextSelection(baseOffset: 0, extentOffset: display.text.length);
  }

  void _rebuild() {
    regions = ansibleFoldRegions(source.text);
    collapsed.retainWhere((offset) => regions.any((region) => region.headerOffset == offset));
    projection = AnsibleProjection(source.text, regions, collapsed);
    final selection = source.selection;
    final visible = selection.isValid
        ? TextSelection(
            baseOffset: projection.toVisible(selection.baseOffset),
            extentOffset: projection.toVisible(selection.extentOffset),
          )
        : const TextSelection.collapsed(offset: 0);
    final composing = source.value.composing;
    _writing = true;
    display.value = TextEditingValue(
      text: projection.text,
      selection: visible,
      composing: composing.isValid && !composing.isCollapsed
          ? TextRange(start: projection.toVisible(composing.start), end: projection.toVisible(composing.end))
          : TextRange.empty,
    );
    _lastDisplay = display.value;
    _writing = false;
  }

  bool toggleFold(int originalLine) {
    if (source.value.composing.isValid && !source.value.composing.isCollapsed) return false;
    final region = regions.where((region) => region.headerLine == originalLine).firstOrNull;
    if (region == null) return false;
    if (!collapsed.remove(region.headerOffset)) collapsed.add(region.headerOffset);
    _rebuild();
    notifyListeners();
    return true;
  }

  void collapseAll() {
    if (source.value.composing.isValid && !source.value.composing.isCollapsed) return;
    collapsed.addAll(regions.map((region) => region.headerOffset));
    _rebuild();
    notifyListeners();
  }

  void expandAll() {
    collapsed.clear();
    _rebuild();
    notifyListeners();
  }

  void reset() {
    _undo.clear();
    _redo.clear();
    collapsed.clear();
    _rebuild();
    notifyListeners();
  }

  void _syncSelection(TextSelection selection) {
    _writing = true;
    source.selection = selection;
    _lastSource = source.value;
    _writing = false;
  }

  void _displayChanged() {
    if (_writing || _disposed) return;
    final next = display.value, previous = _lastDisplay;
    if (next.text == previous.text) {
      final marker = next.selection.isCollapsed ? projection.markerAt(next.selection.extentOffset) : null;
      final selection = selectedSource;
      if (marker != null) {
        collapsed.remove(marker.fold!.headerOffset);
        _syncSelection(TextSelection.collapsed(offset: marker.sourceStart));
        _rebuild();
      } else {
        _syncSelection(selection);
        _lastDisplay = next;
      }
      notifyListeners();
      return;
    }
    if (readOnly) {
      _rebuild();
      return;
    }
    final selection = previous.selection;
    if (selection.isValid &&
        !selection.isCollapsed &&
        next.text.startsWith(previous.text.substring(0, selection.start)) &&
        next.text.endsWith(previous.text.substring(selection.end))) {
      final end = next.text.length - (previous.text.length - selection.end);
      if (end >= selection.start) {
        final insert = next.text.substring(selection.start, end);
        final range = _sourceRange(selection, previous.text.length);
        _apply(range.start, range.end, insert);
        return;
      }
    }
    var start = 0, oldEnd = previous.text.length, newEnd = next.text.length;
    while (start < oldEnd && start < newEnd && previous.text.codeUnitAt(start) == next.text.codeUnitAt(start)) {
      start++;
    }
    while (oldEnd > start &&
        newEnd > start &&
        previous.text.codeUnitAt(oldEnd - 1) == next.text.codeUnitAt(newEnd - 1)) {
      oldEnd--;
      newEnd--;
    }
    final touched = projection.segments
        .where(
          (segment) =>
              segment.fold != null &&
              (start >= segment.start && start < segment.end ||
                  oldEnd > start && start < segment.end && oldEnd >= segment.start),
        )
        .toList();
    if (touched.isNotEmpty) {
      for (final marker in touched) {
        collapsed.remove(marker.fold!.headerOffset);
      }
      _syncSelection(TextSelection.collapsed(offset: touched.first.sourceStart));
      _rebuild();
      notifyListeners();
      return;
    }
    final from = projection.toSource(start),
        to = projection.toSource(oldEnd, downstream: true),
        insert = next.text.substring(start, newEnd);
    int map(int offset) {
      if (offset <= start) return projection.toSource(offset);
      if (offset >= newEnd) {
        return projection.toSource(offset - (newEnd - start) + (oldEnd - start), downstream: true) +
            insert.length -
            (to - from);
      }
      return from + offset - start;
    }

    final mapped = next.selection.isValid
        ? TextSelection(baseOffset: map(next.selection.baseOffset), extentOffset: map(next.selection.extentOffset))
        : TextSelection.collapsed(offset: from + insert.length);
    final composing = next.composing.isValid && !next.composing.isCollapsed
        ? TextRange(start: map(next.composing.start), end: map(next.composing.end))
        : TextRange.empty;
    _apply(from, to, insert, selection: mapped, composing: composing);
  }

  TextRange _sourceRange(TextSelection selection, int length) {
    if (selection.start == 0 && selection.end == length) return TextRange(start: 0, end: source.text.length);
    return TextRange(
      start: projection.toSource(selection.start),
      end: projection.toSource(selection.end, downstream: true),
    );
  }

  bool replaceSelection(String insert) {
    if (readOnly || _disposed) return false;
    final range = _sourceRange(
      display.selection.isValid ? display.selection : const TextSelection.collapsed(offset: 0),
      display.text.length,
    );
    return _apply(range.start, range.end, insert);
  }

  bool _apply(int start, int end, String insert, {TextSelection? selection, TextRange composing = TextRange.empty}) {
    final text = source.text.replaceRange(start, end, insert);
    if (text.length > 1024 * 1024 || utf8.encode(text).length > 1024 * 1024) {
      editRejected = true;
      _rebuild();
      notifyListeners();
      return false;
    }
    _remember(_undo, _History(source.value, collapsed));
    _redo.clear();
    final keep = <int>{}, delta = insert.length - (end - start);
    for (final region in regions) {
      if (!collapsed.contains(region.headerOffset)) continue;
      if (start < region.hiddenEnd && end > region.headerOffset ||
          start >= region.headerOffset && start < region.hiddenEnd) {
        continue;
      }
      keep.add(end <= region.headerOffset ? region.headerOffset + delta : region.headerOffset);
    }
    collapsed
      ..clear()
      ..addAll(keep);
    _writing = true;
    source.value = TextEditingValue(
      text: text,
      selection: selection ?? TextSelection.collapsed(offset: start + insert.length),
      composing: composing,
    );
    _lastSource = source.value;
    _writing = false;
    editRejected = false;
    revision++;
    _rebuild();
    notifyListeners();
    onChanged?.call(source.text);
    return true;
  }

  void undo() => _restore(_undo, _redo);
  void redo() => _restore(_redo, _undo);
  void _restore(List<_History> from, List<_History> to) {
    if (readOnly || from.isEmpty) return;
    final previous = from.removeLast();
    _remember(to, _History(source.value, collapsed));
    _writing = true;
    source.value = previous.value.copyWith(composing: TextRange.empty);
    _lastSource = source.value;
    _writing = false;
    revision++;
    collapsed
      ..clear()
      ..addAll(previous.folds);
    _rebuild();
    notifyListeners();
    onChanged?.call(source.text);
  }

  @override
  void dispose() {
    _disposed = true;
    source.removeListener(_external);
    _source = null;
    display.removeListener(_displayChanged);
    _undo.clear();
    _redo.clear();
    collapsed.clear();
    regions = const [];
    projection = AnsibleProjection('', const [], const {});
    _lastSource = _lastDisplay = TextEditingValue.empty;
    display.clear();
    display.dispose();
    super.dispose();
  }
}

/// Optional command handle for the root-owned settings menu. No text getters.
class AnsibleEditorViewController extends ChangeNotifier {
  AnsibleEditorDocument? _document;
  bool get canUndo => _document?.canUndo ?? false;
  bool get canRedo => _document?.canRedo ?? false;
  void collapseAll() => _document?.collapseAll();
  void expandAll() => _document?.expandAll();
  void undo() => _document?.undo();
  void redo() => _document?.redo();
  void reset() => _document?.reset();
  void attach(AnsibleEditorDocument? document) {
    _document?.removeListener(notifyListeners);
    _document = document;
    _document?.addListener(notifyListeners);
  }

  @override
  void dispose() {
    attach(null);
    super.dispose();
  }
}
