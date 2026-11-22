import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/shared/ports.dart' show copyText;
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:flutter/services.dart';
import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

String plural(int n, String one, String few, String many, {required String locale}) {
  final word = Intl.pluralLogic(
    n,
    one: one,
    few: few,
    many: many,
    other: many,
    locale: locale,
    useExplicitNumberCases: false,
  );
  return '$n $word';
}

String two(int v) => v.toString().padLeft(2, '0');

String clock(DateTime t) => '${two(t.hour)}:${two(t.minute)}:${two(t.second)}';

/// RFC 3339 runtime timestamps in local time; anything else is shown verbatim.
String formatTimestamp(String? raw, {required AppLocalizations l10n}) {
  if (raw == null || raw.isEmpty) return l10n.automationResultMissing;
  final t = DateTime.tryParse(raw)?.toLocal();
  if (t == null) return raw;
  return '${two(t.day)}.${two(t.month)}.${t.year} ${clock(t)}';
}

String shortId(String id) {
  final bare = id.startsWith('sha256:') ? id.substring(7) : id;
  return bare.length <= 12 ? bare : bare.substring(0, 12);
}

class Panel extends StatelessWidget {
  const Panel({super.key, required this.child, this.padding});
  final Widget child;
  final EdgeInsetsGeometry? padding;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    return Container(
      padding: padding,
      clipBehavior: Clip.antiAlias,
      decoration: BoxDecoration(
        color: p.surface,
        borderRadius: BorderRadius.circular(14),
        border: Border.all(color: p.border),
      ),
      child: child,
    );
  }
}

class StatusDot extends StatelessWidget {
  const StatusDot({super.key, required this.color, this.glow = false});
  final Color color;
  final bool glow;

  @override
  Widget build(BuildContext context) => Container(
    width: 8,
    height: 8,
    decoration: BoxDecoration(
      color: color,
      shape: BoxShape.circle,
      boxShadow: glow ? [BoxShadow(color: color.withValues(alpha: .55), blurRadius: 8, spreadRadius: 1)] : null,
    ),
  );
}

class Pill extends StatelessWidget {
  const Pill(this.text, {super.key, required this.color, this.icon});
  final String text;
  final Color color;
  final IconData? icon;

  @override
  Widget build(BuildContext context) => Container(
    padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 3),
    decoration: BoxDecoration(
      color: color.withValues(alpha: .13),
      borderRadius: BorderRadius.circular(20),
      border: Border.all(color: color.withValues(alpha: .22)),
    ),
    child: Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        if (icon != null) ...[Icon(icon, size: 12, color: color), const SizedBox(width: 5)],
        Flexible(
          child: Text(
            text,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: TextStyle(color: color, fontSize: 12, fontWeight: FontWeight.w600),
          ),
        ),
      ],
    ),
  );
}

class GradientIcon extends StatelessWidget {
  const GradientIcon(this.icon, {super.key, this.size = 40, this.color});
  final IconData icon;
  final double size;
  final Color? color;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final tint = color;
    return Container(
      width: size,
      height: size,
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(size * .3),
        gradient: tint == null
            ? p.gradient
            : LinearGradient(
                begin: Alignment.topLeft,
                end: Alignment.bottomRight,
                colors: [tint.withValues(alpha: .26), tint.withValues(alpha: .08)],
              ),
        border: tint == null ? null : Border.all(color: tint.withValues(alpha: .22)),
      ),
      child: Icon(icon, size: size * .5, color: tint ?? contrastingText(p.accent)),
    );
  }
}

class StatCard extends StatelessWidget {
  const StatCard({super.key, required this.label, required this.value, required this.icon, required this.color});
  final String label;
  final String value;
  final IconData icon;
  final Color color;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    return Panel(
      padding: const EdgeInsets.all(16),
      child: Row(
        children: [
          GradientIcon(icon, color: color),
          const SizedBox(width: 14),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  label,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(color: p.muted, fontSize: 12.5),
                ),
                const SizedBox(height: 2),
                Text(
                  value,
                  maxLines: 1,
                  style: const TextStyle(fontSize: 24, height: 1.15, fontWeight: FontWeight.w700, letterSpacing: -.5),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class EmptyState extends StatelessWidget {
  const EmptyState({super.key, required this.icon, required this.title, this.hint, this.loading = false});
  final IconData icon;
  final String title;
  final String? hint;
  final bool loading;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    return Center(
      child: SingleChildScrollView(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (loading)
              const SizedBox(width: 28, height: 28, child: CircularProgressIndicator(strokeWidth: 2.5))
            else
              GradientIcon(icon, size: 52, color: p.muted),
            const SizedBox(height: 16),
            Text(
              title,
              textAlign: TextAlign.center,
              style: const TextStyle(fontSize: 15, fontWeight: FontWeight.w600),
            ),
            if (hint != null) ...[
              const SizedBox(height: 6),
              Text(
                hint!,
                textAlign: TextAlign.center,
                style: TextStyle(color: p.muted, fontSize: 13),
              ),
            ],
          ],
        ),
      ),
    );
  }
}

/// Short monospace ID that copies the full value on click.
class CopyId extends StatelessWidget {
  const CopyId(this.id, {super.key, this.label});
  final String id;
  final String? label;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    return Tooltip(
      message: context.l10n.wslcCopyFullID,
      child: InkWell(
        borderRadius: BorderRadius.circular(4),
        onTap: () => copyText(context, id, label: 'ID'), // l10n-ignore: identifier abbreviation
        child: Text(
          label ?? shortId(id),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: TextStyle(fontFamily: monoFont, fontFamilyFallback: monoFallback, fontSize: 11.5, color: p.muted),
        ),
      ),
    );
  }
}

class SearchField extends StatelessWidget {
  const SearchField({super.key, required this.controller, required this.hint, required this.onChanged});
  final TextEditingController controller;
  final String hint;
  final ValueChanged<String> onChanged;

  @override
  Widget build(BuildContext context) => TextField(
    controller: controller,
    onChanged: onChanged,
    decoration: InputDecoration(
      hintText: hint,
      prefixIcon: const Icon(Icons.search_rounded, size: 20),
      suffixIcon: controller.text.isEmpty
          ? null
          : IconButton(
              tooltip: context.l10n.sftpSearchClear,
              icon: const Icon(Icons.close_rounded, size: 18),
              onPressed: () {
                controller.clear();
                onChanged('');
              },
            ),
    ),
  );
}

/// Pill-shaped segmented filter with counters.
class FilterTabs<T> extends StatelessWidget {
  const FilterTabs({super.key, required this.value, required this.items, required this.onChanged});
  final T value;
  final List<(T, String, int?)> items;
  final ValueChanged<T> onChanged;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    return Container(
      padding: const EdgeInsets.all(3),
      decoration: BoxDecoration(
        color: p.surface,
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: p.border),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (final (key, label, count) in items)
            _FilterTab(label: label, count: count, selected: key == value, onTap: () => onChanged(key)),
        ],
      ),
    );
  }
}

class _FilterTab extends StatelessWidget {
  const _FilterTab({required this.label, required this.count, required this.selected, required this.onTap});
  final String label;
  final int? count;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    return Material(
      color: Colors.transparent,
      child: InkWell(
        borderRadius: BorderRadius.circular(7),
        onTap: onTap,
        child: AnimatedContainer(
          duration: const Duration(milliseconds: 160),
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 7),
          decoration: BoxDecoration(
            color: selected ? p.raised : Colors.transparent,
            borderRadius: BorderRadius.circular(7),
            border: Border.all(color: selected ? p.border : Colors.transparent),
          ),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(
                label,
                style: TextStyle(fontSize: 13, fontWeight: FontWeight.w600, color: selected ? p.text : p.muted),
              ),
              if (count != null) ...[
                const SizedBox(width: 6),
                Text(
                  '$count',
                  style: TextStyle(fontSize: 12, fontWeight: FontWeight.w600, color: selected ? p.accent : p.faint),
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }
}

class InfoTile extends StatelessWidget {
  const InfoTile({super.key, required this.label, required this.value, this.mono = false, this.trailing});
  final String label;
  final String value;
  final bool mono;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      decoration: BoxDecoration(
        color: p.raised.withValues(alpha: .55),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: p.border.withValues(alpha: .7)),
      ),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  label.toUpperCase(),
                  style: TextStyle(color: p.faint, fontSize: 10.5, fontWeight: FontWeight.w700, letterSpacing: .6),
                ),
                const SizedBox(height: 4),
                WslcSelectableText(
                  value,
                  minLines: 1,
                  maxLines: mono ? 3 : 4,
                  style: TextStyle(
                    fontSize: mono ? 12.5 : 13.5,
                    fontWeight: mono ? FontWeight.w400 : FontWeight.w500,
                    fontFamily: mono ? monoFont : null,
                    fontFamilyFallback: mono ? monoFallback : null,
                  ),
                ),
              ],
            ),
          ),
          ?trailing,
        ],
      ),
    );
  }
}

/// Lays tiles out in equal columns that adapt to the available width.
class TileGrid extends StatelessWidget {
  const TileGrid({super.key, required this.children, this.minTileWidth = 220});
  final List<Widget> children;
  final double minTileWidth;

  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, constraints) {
      const gap = 10.0;
      final columns = ((constraints.maxWidth + gap) / (minTileWidth + gap)).floor().clamp(1, 4);
      final width = (constraints.maxWidth - gap * (columns - 1)) / columns;
      return Wrap(
        spacing: gap,
        runSpacing: gap,
        children: [for (final child in children) SizedBox(width: width, child: child)],
      );
    },
  );
}

class CodeView extends StatelessWidget {
  const CodeView(this.text, {super.key});
  final String text;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    return DecoratedBox(
      decoration: BoxDecoration(
        color: p.terminal,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: p.border),
      ),
      child: Stack(
        children: [
          Positioned.fill(
            child: Scrollbar(
              child: SingleChildScrollView(
                padding: const EdgeInsets.fromLTRB(16, 14, 56, 14),
                child: WslcSelectableText(
                  text,
                  style: TextStyle(
                    fontFamily: monoFont,
                    fontFamilyFallback: monoFallback,
                    fontSize: 12.5,
                    height: 1.45,
                    color: p.terminalText,
                  ),
                ),
              ),
            ),
          ),
          Positioned(
            top: 8,
            right: 8,
            child: IconButton(
              tooltip: context.l10n.wslcCopy,
              style: IconButton.styleFrom(
                foregroundColor: p.terminalText,
                backgroundColor: Colors.white.withValues(alpha: .06),
              ),
              onPressed: () => WslcApiScope.copy(context, ClipboardData(text: text)),
              icon: const Icon(Icons.copy_rounded, size: 17),
            ),
          ),
        ],
      ),
    );
  }
}

/// Read-only JSON viewer for inspect documents.
Future<void> showInspectDialog(BuildContext context, String title, Future<String> Function() load) async {
  final notify = WslcApiScope.notifierOf(context);
  final String text;
  try {
    text = await load();
  } catch (e) {
    notify(errorText(e), error: true);
    return;
  }
  if (!context.mounted) return;
  await showWslcDialog<void>(
    context: context,
    builder: (context) => Dialog(
      insetPadding: const EdgeInsets.all(40),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 820, maxHeight: 640),
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  const GradientIcon(Icons.data_object_rounded, size: 34),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Text(
                      title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(fontSize: 17, fontWeight: FontWeight.w700),
                    ),
                  ),
                  IconButton(
                    tooltip: context.l10n.commonClose,
                    onPressed: () => Navigator.pop(context),
                    icon: const Icon(Icons.close_rounded),
                  ),
                ],
              ),
              const SizedBox(height: 14),
              Expanded(child: CodeView(text)),
            ],
          ),
        ),
      ),
    ),
  );
}
