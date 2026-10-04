import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Content padding of shell pages (§2.6): 24, 16 in compact windows.
double pagePadding(BuildContext context) =>
    MediaQuery.sizeOf(context).width < GlassSizes.compactBreakpoint ? GlassSpacing.pageCompact : GlassSpacing.page;

/// Standard page layout inside the app shell (content layer, §3): the page
/// title provides a clear content hierarchy beneath the compact toolbar,
/// followed by a subtitle, actions and the body. Pages are
/// transparent (the ambient backdrop shows through); lists and forms sit on
/// [ContentSurface] cards.
class PageScaffold extends StatelessWidget {
  const PageScaffold({
    required this.title,
    required this.body,
    super.key,
    this.subtitle,
    this.actions = const [],
    this.scrollable = false,
    this.maxWidth,
    this.embedded = false,
  });

  /// Page heading and accessible region name.
  final String title;
  final String? subtitle;
  final List<Widget> actions;
  final Widget body;
  final bool scrollable;
  final double? maxWidth;
  final bool embedded;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final pad = embedded ? 12.0 : pagePadding(context);
    Widget content = Padding(padding: EdgeInsets.fromLTRB(pad, 0, pad, pad), child: body);
    if (maxWidth != null) {
      content = Align(
        alignment: Alignment.topLeft,
        child: ConstrainedBox(
          constraints: BoxConstraints(maxWidth: maxWidth!),
          child: content,
        ),
      );
    }
    if (scrollable) content = ScrollEdgeEffect(child: SingleChildScrollView(child: content));
    return Semantics(
      container: true,
      explicitChildNodes: true,
      label: title,
      child: Material(
        type: MaterialType.transparency,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (embedded)
              Padding(
                padding: EdgeInsets.fromLTRB(pad, 12, pad, 12),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    if (subtitle != null) ...[
                      Text(subtitle!, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
                      const SizedBox(height: 10),
                    ],
                    if (actions.isNotEmpty)
                      Wrap(spacing: 8, runSpacing: 8, crossAxisAlignment: WrapCrossAlignment.center, children: actions),
                  ],
                ),
              )
            else if (MediaQuery.sizeOf(context).width >= 600 || MediaQuery.viewInsetsOf(context).bottom == 0)
              Padding(
                // Keep the page heading and its actions clear of the shell
                // toolbar, with an equal gap before the first content row.
                padding: EdgeInsets.fromLTRB(pad, GlassSpacing.s24, pad, GlassSpacing.s24),
                // Actions may take at most 60 % of the width and wrap onto more lines,
                // so long (e.g. Russian) button labels never squeeze the subtitle.
                child: LayoutBuilder(
                  builder: (context, constraints) => constraints.maxWidth < 600
                      ? Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            Semantics(
                              header: true,
                              child: Text(title, style: tokens.typography.title1.copyWith(color: tokens.palette.label)),
                            ),
                            if (subtitle != null) ...[
                              const SizedBox(height: 4),
                              Text(subtitle!, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
                            ],
                            if (actions.isNotEmpty) ...[
                              const SizedBox(height: 12),
                              Wrap(spacing: 8, runSpacing: 8, children: actions),
                            ],
                          ],
                        )
                      : Row(
                          children: [
                            Expanded(
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  Semantics(
                                    header: true,
                                    child: Text(
                                      title,
                                      style: tokens.typography.title1.copyWith(color: tokens.palette.label),
                                    ),
                                  ),
                                  if (subtitle != null) ...[
                                    const SizedBox(height: GlassSpacing.s4),
                                    Text(
                                      subtitle!,
                                      style: tokens.typography.body.copyWith(color: tokens.secondaryLabel),
                                    ),
                                  ],
                                ],
                              ),
                            ),
                            if (actions.isNotEmpty) ...[
                              const SizedBox(width: GlassSpacing.s12),
                              ConstrainedBox(
                                constraints: BoxConstraints(maxWidth: constraints.maxWidth * 0.6),
                                child: Wrap(
                                  spacing: GlassSpacing.s8,
                                  runSpacing: GlassSpacing.s8,
                                  alignment: WrapAlignment.end,
                                  crossAxisAlignment: WrapCrossAlignment.center,
                                  children: actions,
                                ),
                              ),
                            ],
                          ],
                        ),
                ),
              ),
            Expanded(child: content),
          ],
        ),
      ),
    );
  }
}

/// Content card with a heading (§4.6): [ContentSurface], title3 heading,
/// optional secondary icon, subtitle and trailing action.
class SectionCard extends StatelessWidget {
  const SectionCard({required this.title, required this.child, super.key, this.subtitle, this.trailing, this.icon});

  final String title;
  final String? subtitle;
  final Widget? trailing;
  final IconData? icon;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return ContentSurface(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              if (icon != null) ...[
                Icon(icon, size: GlassSizes.iconToolbar, color: tokens.secondaryLabel),
                const SizedBox(width: GlassSpacing.s8),
              ],
              Expanded(
                child: Semantics(
                  header: true,
                  child: Text(title, style: tokens.typography.title3.copyWith(color: tokens.palette.label)),
                ),
              ),
              if (MediaQuery.sizeOf(context).width >= 600) ?trailing,
            ],
          ),
          if (trailing != null && MediaQuery.sizeOf(context).width < 600)
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: Align(alignment: Alignment.centerLeft, child: trailing),
            ),
          if (subtitle != null)
            Padding(
              padding: const EdgeInsets.only(top: GlassSpacing.s4),
              child: Text(subtitle!, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
            ),
          const SizedBox(height: GlassSpacing.s12),
          child,
        ],
      ),
    );
  }
}

/// A scrolling list on a content card (§4.6): rows inset 4 inside the card,
/// hairline separators, soft fades at the scroll edges. The list gets its
/// own [RepaintBoundary] (§6.6).
class ContentList extends StatelessWidget {
  const ContentList({
    required this.itemCount,
    required this.itemBuilder,
    super.key,
    this.separated = true,
    this.controller,
    this.shrinkWrap = false,
  });

  final int itemCount;
  final IndexedWidgetBuilder itemBuilder;
  final bool separated;
  final ScrollController? controller;
  final bool shrinkWrap;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    const padding = EdgeInsets.symmetric(vertical: GlassSpacing.s4);
    final list = separated
        ? ListView.separated(
            controller: controller,
            shrinkWrap: shrinkWrap,
            padding: padding,
            itemCount: itemCount,
            separatorBuilder: (_, _) => Padding(
              padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s12),
              child: Divider(height: 1, color: tokens.surfaces.separator),
            ),
            itemBuilder: itemBuilder,
          )
        : ListView.builder(
            controller: controller,
            shrinkWrap: shrinkWrap,
            padding: padding,
            itemCount: itemCount,
            itemBuilder: itemBuilder,
          );
    return ContentSurface(
      padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s4),
      child: RepaintBoundary(child: ScrollEdgeEffect(bottom: true, extent: 12, child: list)),
    );
  }
}

/// Empty state (§4.6): large title, body and at most one prominent action.
/// No illustrations; a quiet tertiary glyph anchors the text. [compact] is
/// for dialogs and side panes.
class EmptyState extends StatelessWidget {
  const EmptyState({
    required this.icon,
    required this.title,
    super.key,
    this.message,
    this.action,
    this.compact = false,
  });

  final IconData icon;
  final String title;
  final String? message;
  final Widget? action;
  final bool compact;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    return Center(
      child: SingleChildScrollView(
        padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s8, horizontal: GlassSpacing.s16),
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 440),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(icon, size: compact ? 32 : 44, color: tokens.palette.tertiary),
              const SizedBox(height: GlassSpacing.s12),
              Text(
                title,
                style: (compact ? t.title3 : t.largeTitle).copyWith(color: tokens.palette.label),
                textAlign: TextAlign.center,
              ),
              if (message != null) ...[
                const SizedBox(height: GlassSpacing.s6),
                Text(
                  message!,
                  textAlign: TextAlign.center,
                  style: t.body.copyWith(color: tokens.secondaryLabel),
                ),
              ],
              if (action != null) ...[const SizedBox(height: GlassSpacing.s20), action!],
            ],
          ),
        ),
      ),
    );
  }
}

/// Renders an [AsyncValue] with consistent loading/error states.
class AsyncValueView<T> extends StatelessWidget {
  const AsyncValueView({required this.value, required this.data, super.key});

  final AsyncValue<T> value;
  final Widget Function(T data) data;

  @override
  Widget build(BuildContext context) {
    return switch (value) {
      AsyncValue(:final value?, hasValue: true) => data(value),
      AsyncError(:final error) => Center(child: Text(errorMessage(context.l10n, error))),
      _ => const Center(child: CircularProgressIndicator()),
    };
  }
}

enum BannerTone { info, warning, danger, success }

extension BannerToneGlass on BannerTone {
  GlassTone get glassTone => switch (this) {
    BannerTone.info => GlassTone.info,
    BannerTone.warning => GlassTone.warning,
    BannerTone.danger => GlassTone.danger,
    BannerTone.success => GlassTone.success,
  };
}

/// Inline callout (never a modal) for warnings and explanations (§4.1
/// banners): content layer, not glass — the role colour at α .12, `r.card`,
/// an icon in the role colour, the text in the label colour and an optional
/// action below it.
class InfoBanner extends StatelessWidget {
  const InfoBanner({required this.message, super.key, this.tone = BannerTone.info, this.title, this.action, this.icon});

  final String message;
  final String? title;
  final BannerTone tone;
  final Widget? action;
  final IconData? icon;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final glassTone = tone.glassTone;
    final roleColor = p.tone(glassTone);
    final defaultIcon = switch (tone) {
      BannerTone.info => Icons.info_rounded,
      BannerTone.warning => Icons.warning_rounded,
      BannerTone.danger => Icons.gpp_bad_rounded,
      BannerTone.success => Icons.check_circle_rounded,
    };
    final t = tokens.typography;
    final ic = tokens.highContrast;
    return DecoratedBox(
      decoration: ShapeDecoration(
        color: Color.alphaBlend(p.toneFill(glassTone).withValues(alpha: 0.12), tokens.surfaces.content),
        shape: GlassRadii.shape(
          tokens.radii.card,
        ).copyWith(side: BorderSide(color: ic ? roleColor.withValues(alpha: 0.6) : roleColor.withValues(alpha: 0.18))),
      ),
      child: Padding(
        padding: const EdgeInsets.all(GlassSpacing.s12),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Icon(icon ?? defaultIcon, color: roleColor, size: 20),
            const SizedBox(width: GlassSpacing.s12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  if (title != null) ...[
                    Text(title!, style: t.bodyEmph.copyWith(color: p.label)),
                    const SizedBox(height: GlassSpacing.s2),
                  ],
                  Text(message, style: t.body.copyWith(color: p.label)),
                  // Below the text, so long labels never overflow narrow cards.
                  if (action != null) Align(alignment: AlignmentDirectional.centerEnd, child: action),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class StatusDot extends StatelessWidget {
  const StatusDot({required this.color, super.key, this.size = 8});

  final Color color;
  final double size;

  @override
  Widget build(BuildContext context) => SizedBox(
    width: size,
    height: size,
    child: DecoratedBox(
      decoration: BoxDecoration(color: color, shape: BoxShape.circle),
    ),
  );
}

/// "Label: value" row for detail panes.
class LabeledValue extends StatelessWidget {
  const LabeledValue({required this.label, required this.value, super.key, this.trailing, this.labelWidth = 160});

  final String label;
  final Widget value;
  final Widget? trailing;
  final double labelWidth;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    if (MediaQuery.sizeOf(context).width < 600) {
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: 8),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(label, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
            const SizedBox(height: 6),
            DefaultTextStyle.merge(
              style: tokens.typography.body.copyWith(color: tokens.palette.label),
              child: value,
            ),
            if (trailing != null) Align(alignment: Alignment.centerLeft, child: trailing),
          ],
        ),
      );
    }
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: labelWidth,
            child: Padding(
              padding: const EdgeInsets.only(top: 1),
              child: Text(label, style: tokens.typography.body.copyWith(color: tokens.secondaryLabel)),
            ),
          ),
          Expanded(
            child: DefaultTextStyle.merge(
              style: tokens.typography.body.copyWith(color: tokens.palette.label),
              child: value,
            ),
          ),
          ?trailing,
        ],
      ),
    );
  }
}

/// Tag capsules (§4.6): caption on `surface.inset`.
class TagChips extends StatelessWidget {
  const TagChips({required this.tags, super.key});

  final List<String> tags;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return Wrap(
      spacing: GlassSpacing.s4,
      runSpacing: GlassSpacing.s4,
      children: [
        for (final t in tags)
          DecoratedBox(
            decoration: ShapeDecoration(color: tokens.surfaces.inset, shape: const StadiumBorder()),
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s8, vertical: 1),
              child: Text(t, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
            ),
          ),
      ],
    );
  }
}

/// Free-form tag editor: type + Enter to add, click × to remove.
class TagEditor extends StatefulWidget {
  const TagEditor({required this.tags, required this.onChanged, super.key, this.label});

  final List<String> tags;
  final ValueChanged<List<String>> onChanged;

  /// Defaults to the localized "Tags".
  final String? label;

  @override
  State<TagEditor> createState() => _TagEditorState();
}

class _TagEditorState extends State<TagEditor> {
  final _controller = TextEditingController();

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _add() {
    final tag = _controller.text.trim().toLowerCase();
    _controller.clear();
    if (tag.isEmpty || widget.tags.contains(tag)) return;
    widget.onChanged([...widget.tags, tag]);
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        TextField(
          controller: _controller,
          decoration: InputDecoration(
            labelText: widget.label ?? context.l10n.tagsLabel,
            hintText: context.l10n.tagsHint,
            suffixIcon: IconButton(icon: const Icon(Icons.add_rounded), tooltip: context.l10n.tagsAdd, onPressed: _add),
          ),
          onSubmitted: (_) => _add(),
        ),
        if (widget.tags.isNotEmpty) ...[
          const SizedBox(height: GlassSpacing.s8),
          Wrap(
            spacing: GlassSpacing.s6,
            runSpacing: GlassSpacing.s6,
            children: [
              for (final t in widget.tags)
                InputChip(label: Text(t), onDeleted: () => widget.onChanged([...widget.tags]..remove(t))),
            ],
          ),
        ],
      ],
    );
  }
}
