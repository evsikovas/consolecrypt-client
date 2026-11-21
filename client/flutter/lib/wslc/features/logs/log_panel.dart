import 'dart:async';
import 'dart:collection';

import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/features/logs/log_format.dart';
import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:flutter/rendering.dart' show ScrollDirection;
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';


const _maxLines = 10000;
const _maxBytes = 10 * 1024 * 1024;
const _reading = 'Чтение';

/// UTF-8 length without allocating an encoded copy of every line.
int utf8Length(String text) {
  var n = 0;
  for (var i = 0; i < text.length; i++) {
    final unit = text.codeUnitAt(i);
    if (unit < 0x80) {
      n += 1;
    } else if (unit < 0x800) {
      n += 2;
    } else if (unit >= 0xd800 && unit <= 0xdbff && i + 1 < text.length) {
      n += 4;
      i++;
    } else {
      n += 3;
    }
  }
  return n;
}

class _Line {
  _Line(this.entry, {required this.stamped}) : bytes = utf8Length(entry.text) + 1;
  final LogEntry entry;
  final int bytes;

  /// Read with `--timestamps`; only then a leading time is the runtime's, not the app's.
  final bool stamped;
  String? _lower;
  String get lower => _lower ??= entry.text.toLowerCase();
  (String, String)? _split;
  bool _splitDone = false;

  /// `--timestamps` prefix in local time and the message after it.
  (String, String)? get split {
    if (!_splitDone) {
      _split = stamped ? splitTimestamp(entry.text) : null;
      _splitDone = true;
    }
    return _split;
  }

  String get message => split?.$2 ?? entry.text;
  LogLevel? _level;
  bool _levelDone = false;
  LogLevel? get level {
    if (!_levelDone) {
      _level = detectLevel(message);
      _levelDone = true;
    }
    return _level;
  }

  String? _pretty;
  bool _prettyDone = false;
  String? get pretty {
    if (!_prettyDone) {
      _pretty = prettyJson(message);
      _prettyDone = true;
    }
    return _pretty;
  }
}

enum LogPeriod { tail, minutes5, hour, day, all }

enum LogStream { all, stdout, stderr }

class LogPanel extends StatefulWidget {
  const LogPanel({super.key, required this.containerId, required this.active});
  final String containerId;
  final bool active;
  @override
  State<LogPanel> createState() => _LogPanelState();
}

class _LogPanelState extends State<LogPanel> {
  late final api = WslcApiScope.of(context);

  final lines = ListQueue<_Line>();

  /// Lines passing the stream filter and search, maintained incrementally.
  ListQueue<_Line>? matches;
  String query = '';
  LogStream streamFilter = LogStream.all;
  LogPeriod period = LogPeriod.tail;
  bool timestamps = false;
  bool formatJson = false;
  final search = TextEditingController();
  final scroll = ScrollController();
  BigInt? subscription;
  Timer? timer;
  bool fetching = false;
  bool follow = true;
  bool starting = false;
  bool restartRequested = false;
  int bytes = 0;
  int evicted = 0;
  BigInt dropped = BigInt.zero;
  BigInt truncated = BigInt.zero;
  String status = 'Подключение…';

  @override
  void initState() {
    super.initState();
    if (widget.active) unawaited(start());
  }

  @override
  void didUpdateWidget(covariant LogPanel old) {
    super.didUpdateWidget(old);
    if (old.active != widget.active) {
      if (widget.active) {
        if (starting) restartRequested = true;
        unawaited(start());
      } else {
        unawaited(stop());
      }
    }
  }

  LogOptions get options {
    String ago(Duration d) => (DateTime.now().subtract(d).millisecondsSinceEpoch ~/ 1000).toString();
    return switch (period) {
      LogPeriod.tail => LogOptions(tail: 1000, timestamps: timestamps),
      LogPeriod.minutes5 => LogOptions(since: ago(const Duration(minutes: 5)), timestamps: timestamps),
      LogPeriod.hour => LogOptions(since: ago(const Duration(hours: 1)), timestamps: timestamps),
      LogPeriod.day => LogOptions(since: ago(const Duration(days: 1)), timestamps: timestamps),
      // Everything the buffer can hold: the newest 10 000 lines.
      LogPeriod.all => LogOptions(since: '1', tail: _maxLines, timestamps: timestamps),
    };
  }

  bool get filtering => query.isNotEmpty || streamFilter != LogStream.all;

  bool passes(_Line l) =>
      (streamFilter == LogStream.all || l.entry.stream == streamFilter.name) &&
      (query.isEmpty || l.lower.contains(query));

  void rebuildMatches() {
    matches = filtering ? ListQueue.of(lines.where(passes)) : null;
  }

  void resetBuffer() {
    lines.clear();
    matches?.clear();
    bytes = 0;
    evicted = 0;
  }

  Future<void> start() async {
    if (starting || subscription != null) return;
    starting = true;
    // A reopened tab starts a fresh tail/follow stream; never silently append duplicate history.
    resetBuffer();
    dropped = BigInt.zero;
    truncated = BigInt.zero;
    try {
      final id = await api.subscribeLogs(containerId: widget.containerId, options: options);
      if (!mounted || !widget.active) {
        await api.unsubscribeLogs(subscriptionId: id);
        return;
      }
      subscription = id;
      timer = Timer.periodic(const Duration(milliseconds: 100), (_) => unawaited(fetch()));
      await fetch();
    } catch (e) {
      if (mounted) setState(() => status = errorText(e));
    } finally {
      starting = false;
      if (restartRequested) {
        restartRequested = false;
        if (mounted && widget.active && subscription == null) {
          unawaited(start());
        }
      }
    }
  }

  /// History options changed: reconnect with a fresh buffer.
  Future<void> restart() async {
    await stop();
    if (!mounted) return;
    setState(() {
      status = 'Подключение…';
      resetBuffer();
    });
    await start();
  }

  void append(LogEntry entry) {
    final line = _Line(entry, stamped: timestamps);
    lines.add(line);
    bytes += line.bytes;
    if (filtering && passes(line)) matches!.add(line);
    while (lines.length > _maxLines || bytes > _maxBytes) {
      final gone = lines.removeFirst();
      bytes -= gone.bytes;
      evicted++;
      final m = matches;
      if (m != null && m.isNotEmpty && identical(m.first, gone)) {
        m.removeFirst();
      }
    }
  }

  Future<void> fetch() async {
    final id = subscription;
    if (fetching || id == null) return;
    fetching = true;
    try {
      final batch = await api.takeLogBatch(subscriptionId: id);
      if (!mounted || subscription != id) return;
      final changed =
          batch.entries.isNotEmpty ||
          batch.status != status ||
          batch.droppedTotal != dropped ||
          batch.truncatedTotal != truncated;
      // Idle polls do not rebuild the view.
      if (changed) {
        setState(() {
          batch.entries.forEach(append);
          status = batch.status;
          dropped = batch.droppedTotal;
          truncated = batch.truncatedTotal;
        });
      }
      if (batch.status != _reading && batch.entries.isEmpty) await stop();
      if (follow && batch.entries.isNotEmpty) scrollToEnd();
    } catch (e) {
      await stop();
      if (mounted) setState(() => status = errorText(e));
    } finally {
      fetching = false;
    }
  }

  void scrollToEnd() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && scroll.hasClients) {
        scroll.jumpTo(scroll.position.maxScrollExtent);
      }
    });
  }

  void setQuery(String value) {
    final q = value.trim().toLowerCase();
    if (q == query) return;
    setState(() {
      query = q;
      rebuildMatches();
    });
  }

  Future<void> stop() async {
    timer?.cancel();
    timer = null;
    final id = subscription;
    subscription = null;
    if (id != null) await api.unsubscribeLogs(subscriptionId: id);
  }

  String get loadedText => lines.map((r) => r.entry.text).join('\n');

  Future<void> save() async {
    final messenger = ScaffoldMessenger.of(context);
    try {
      final path = await api.saveLoadedLogs(text: loadedText);
      if (path != null) {
        messenger.showSnackBar(SnackBar(content: Text('Сохранено: $path')));
      }
    } catch (e) {
      messenger.showSnackBar(SnackBar(content: Text(errorText(e))));
    }
  }

  @override
  void dispose() {
    unawaited(stop());
    search.dispose();
    scroll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final source = matches ?? lines;
    final live = status == _reading;
    final baseStyle = TextStyle(
      fontFamily: monoFont,
      fontFamilyFallback: monoFallback,
      fontSize: 12.5,
      height: 1.45,
      color: p.terminalText,
    );
    final levelColors = {
      LogLevel.error: const Color(0xffff8a8a),
      LogLevel.warn: const Color(0xfffbbf24),
      LogLevel.info: p.terminalText,
      LogLevel.debug: p.terminalText.withValues(alpha: .55),
    };
    final stampStyle = baseStyle.copyWith(color: p.terminalText.withValues(alpha: .4));
    final highlight = TextStyle(backgroundColor: p.warning.withValues(alpha: .35), color: Colors.white);
    return Column(
      children: [
        Row(
          children: [
            Expanded(
              child: SearchField(controller: search, hint: 'Поиск в загруженных логах', onChanged: setQuery),
            ),
            const SizedBox(width: 10),
            _ToolbarToggle(
              selected: follow,
              icon: follow ? Icons.vertical_align_bottom_rounded : Icons.pause_rounded,
              label: follow ? 'Следить' : 'Пауза',
              tooltip: follow ? 'Приостановить автопрокрутку' : 'Включить автопрокрутку',
              onTap: () {
                setState(() => follow = !follow);
                if (follow) scrollToEnd();
              },
            ),
            const SizedBox(width: 4),
            IconButton(
              tooltip: 'Очистить отображение',
              onPressed: () => setState(resetBuffer),
              icon: const Icon(Icons.clear_all_rounded),
            ),
            IconButton(
              tooltip: 'Скопировать загруженные логи',
              onPressed: () => WslcApiScope.copy(context, ClipboardData(text: loadedText)),
              icon: const Icon(Icons.copy_all_rounded),
            ),
            IconButton(
              tooltip: 'Сохранить загруженные логи',
              onPressed: save,
              icon: const Icon(Icons.save_alt_rounded),
            ),
          ],
        ),
        const SizedBox(height: 8),
        Align(
          alignment: Alignment.centerLeft,
          child: SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: Row(
              children: [
                FilterTabs<LogPeriod>(
                  value: period,
                  onChanged: (v) {
                    setState(() => period = v);
                    unawaited(restart());
                  },
                  items: const [
                    (LogPeriod.tail, '1000 строк', null),
                    (LogPeriod.minutes5, '5 мин', null),
                    (LogPeriod.hour, '1 час', null),
                    (LogPeriod.day, '24 часа', null),
                    (LogPeriod.all, 'Всё', null),
                  ],
                ),
                const SizedBox(width: 10),
                FilterTabs<LogStream>(
                  value: streamFilter,
                  onChanged: (v) => setState(() {
                    streamFilter = v;
                    rebuildMatches();
                  }),
                  items: const [
                    (LogStream.all, 'Все потоки', null),
                    (LogStream.stdout, 'stdout', null),
                    (LogStream.stderr, 'stderr', null),
                  ],
                ),
                const SizedBox(width: 10),
                FilterChip(
                  label: const Text('Время'),
                  tooltip: 'Метки времени runtime в локальном времени',
                  selected: timestamps,
                  onSelected: (v) {
                    setState(() => timestamps = v);
                    unawaited(restart());
                  },
                ),
                const SizedBox(width: 8),
                FilterChip(
                  label: const Text('JSON'),
                  tooltip: 'Форматировать строки-объекты JSON',
                  selected: formatJson,
                  onSelected: (v) => setState(() => formatJson = v),
                ),
              ],
            ),
          ),
        ),
        const SizedBox(height: 10),
        Expanded(
          child: Container(
            decoration: BoxDecoration(
              color: p.terminal,
              borderRadius: BorderRadius.circular(12),
              border: Border.all(color: p.border),
            ),
            clipBehavior: Clip.antiAlias,
            child: NotificationListener<UserScrollNotification>(
              // Scrolling up pauses follow, like a terminal.
              onNotification: (n) {
                if (follow &&
                    n.direction == ScrollDirection.forward &&
                    scroll.hasClients &&
                    scroll.position.extentAfter > 40) {
                  setState(() => follow = false);
                }
                return false;
              },
              child: source.isEmpty
                  ? Center(
                      child: Text(
                        filtering ? 'Совпадений нет' : status,
                        style: baseStyle.copyWith(color: p.terminalText.withValues(alpha: .5)),
                      ),
                    )
                  : SelectionArea(
                      child: Scrollbar(
                        controller: scroll,
                        child: ListView.builder(
                          controller: scroll,
                          padding: const EdgeInsets.symmetric(vertical: 8),
                          itemCount: source.length,
                          itemBuilder: (context, index) {
                            final line = source.elementAt(index);
                            final entry = line.entry;
                            final style = baseStyle.copyWith(color: levelColors[line.level] ?? p.terminalText);
                            final suffix = entry.truncated ? ' … [усечено]' : '';
                            final body = formatJson ? line.pretty ?? line.message : line.message;
                            final stamp = line.split?.$1;
                            final lower = body.toLowerCase();
                            return Container(
                              padding: const EdgeInsets.fromLTRB(12, 1, 14, 1),
                              decoration: entry.stream == 'stderr'
                                  ? BoxDecoration(
                                      border: Border(
                                        left: BorderSide(color: p.warning.withValues(alpha: .7), width: 2),
                                      ),
                                    )
                                  : null,
                              child: Text.rich(
                                TextSpan(
                                  style: style,
                                  children: [
                                    if (stamp != null) TextSpan(text: '$stamp  ', style: stampStyle),
                                    ...query.isEmpty
                                        ? [TextSpan(text: body)]
                                        : _highlighted(body, lower, query, highlight),
                                    TextSpan(text: suffix),
                                  ],
                                ),
                              ),
                            );
                          },
                        ),
                      ),
                    ),
            ),
          ),
        ),
        const SizedBox(height: 8),
        Row(
          children: [
            StatusDot(color: live ? p.success : p.faint, glow: live),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                [
                  status,
                  plural(lines.length, 'строка', 'строки', 'строк'),
                  if (filtering) 'показано: ${source.length}',
                  'вытеснено: ${BigInt.from(evicted) + dropped}',
                  'усечено: $truncated',
                ].join('  ·  '),
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: p.muted, fontSize: 12),
              ),
            ),
            Tooltip(
              message: 'Логи могут содержать секреты',
              child: Icon(Icons.privacy_tip_outlined, size: 15, color: p.faint),
            ),
          ],
        ),
      ],
    );
  }
}

List<TextSpan> _highlighted(String text, String lower, String query, TextStyle highlight) {
  final spans = <TextSpan>[];
  var start = 0;
  // Lowercasing can change length for rare characters; fall back to plain text then.
  if (lower.length != text.length) return [TextSpan(text: text)];
  while (true) {
    final i = lower.indexOf(query, start);
    if (i < 0) break;
    if (i > start) spans.add(TextSpan(text: text.substring(start, i)));
    spans.add(TextSpan(text: text.substring(i, i + query.length), style: highlight));
    start = i + query.length;
  }
  if (start < text.length) spans.add(TextSpan(text: text.substring(start)));
  return spans;
}

class _ToolbarToggle extends StatelessWidget {
  const _ToolbarToggle({
    required this.selected,
    required this.icon,
    required this.label,
    required this.tooltip,
    required this.onTap,
  });
  final bool selected;
  final IconData icon;
  final String label;
  final String tooltip;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final color = selected ? p.success : p.muted;
    return Tooltip(
      message: tooltip,
      child: Material(
        color: selected ? p.success.withValues(alpha: .12) : p.surface,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(10),
          side: BorderSide(color: selected ? p.success.withValues(alpha: .3) : p.border),
        ),
        child: InkWell(
          borderRadius: BorderRadius.circular(10),
          onTap: onTap,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(icon, size: 17, color: color),
                const SizedBox(width: 6),
                Text(
                  label,
                  style: TextStyle(color: color, fontSize: 13, fontWeight: FontWeight.w600),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
