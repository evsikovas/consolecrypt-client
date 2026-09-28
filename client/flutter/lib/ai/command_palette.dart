import 'dart:async';

import 'package:consolecrypt/ai/code_blocks.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/risk_badge.dart';
import 'package:consolecrypt/snippets/run_flow.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

/// Open-palette marker per root navigator (not a global: a torn-down
/// navigator must not leave a stale "open" flag behind).
final Expando<bool> _paletteOpen = Expando<bool>('command palette open'); // l10n-ignore: Expando debug name

/// Cmd/Ctrl+K overlay: search snippets, quick actions, AI command
/// generation. Results are only inserted/run after an explicit action.
Future<void> showCommandPalette(BuildContext context, {String? selectedText, ObjectId? hostId}) async {
  final navigator = Navigator.of(context, rootNavigator: true);
  if (_paletteOpen[navigator] ?? false) return;
  _paletteOpen[navigator] = true;
  try {
    // `glass.thick`, live backdrop (refractive on macOS), one overlay level.
    await showGlassDialog<void>(
      context,
      builder: (_) => CommandPalette(selectedText: selectedText, hostId: hostId),
    );
  } finally {
    _paletteOpen[navigator] = false;
  }
}

/// Default AI provider (flagged default, else first).
AiProviderConfig? defaultProvider(List<AiProviderConfig> providers) =>
    providers.where((p) => p.isDefault).firstOrNull ?? providers.firstOrNull;

/// Why AI generation stopped; rendered as localized text.
enum _AiError { noProvider, failed }

/// Result groups of the palette (caption headers, §4.8).
enum _Group { ai, snippets, actions }

final class _Item {
  const _Item({
    required this.group,
    required this.icon,
    required this.title,
    required this.onSelect,
    this.subtitle,
    this.trailing,
    this.key,
  });

  final _Group group;
  final IconData icon;
  final String title;
  final String? subtitle;
  final Widget? trailing;
  final VoidCallback onSelect;
  final Key? key;
}

class CommandPalette extends ConsumerStatefulWidget {
  const CommandPalette({super.key, this.selectedText, this.hostId});

  /// Allowed AI context (§14): selected terminal text, host id.
  final String? selectedText;
  final ObjectId? hostId;

  @override
  ConsumerState<CommandPalette> createState() => _CommandPaletteState();
}

class _CommandPaletteState extends ConsumerState<CommandPalette> {
  final _input = TextEditingController();
  final _focus = FocusNode();
  List<SnippetSearchHit> _hits = const [];
  int _selected = 0;
  Timer? _debounce;

  // AI generation state.
  StreamSubscription<AiStreamEvent>? _generation;
  String _streamed = '';
  GeneratedCommand? _command;
  RiskAssessment? _risk;
  SanitizationReport? _report;
  _AiError? _aiError;

  /// Raw provider diagnostic for [_AiError.failed] (never localized).
  String? _aiErrorDetail;
  bool _generating = false;

  @override
  void initState() {
    super.initState();
    unawaited(_search(''));
  }

  @override
  void dispose() {
    _debounce?.cancel();
    unawaited(_generation?.cancel());
    _input.dispose();
    _focus.dispose();
    super.dispose();
  }

  Future<void> _search(String q) async {
    final hits = await ref.read(snippetServiceProvider).search(q, limit: 8);
    if (mounted) {
      setState(() {
        _hits = hits;
        _selected = 0;
      });
    }
  }

  void _onChanged(String q) {
    setState(() {
      _command = null;
      _streamed = '';
      _aiError = null;
    });
    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 120), () => _search(q));
  }

  void _close() => Navigator.of(context).pop();

  Future<void> _generate(String request) async {
    final provider = defaultProvider(ref.read(aiProvidersProvider).value ?? const []);
    if (provider == null) {
      setState(() => _aiError = _AiError.noProvider);
      return;
    }
    await _generation?.cancel();
    setState(() {
      _generating = true;
      _streamed = '';
      _command = null;
      _risk = null;
      _aiError = null;
    });
    _generation = ref
        .read(aiServiceProvider)
        .generateCommand(
          providerId: provider.id,
          request: request,
          context: AiContextSelection(hostId: widget.hostId, selectedTerminalText: widget.selectedText),
        )
        .listen((event) async {
          switch (event) {
            case AiDelta(:final text):
              setState(() => _streamed += text);
            case AiCompleted(:final command, :final report):
              RiskAssessment? risk;
              if (command != null) {
                risk = await ref
                    .read(snippetServiceProvider)
                    .assessRisk(command.command, declared: command.suggestedRisk, source: SnippetSource.ai);
              }
              if (mounted) {
                setState(() {
                  _command = command;
                  _risk = risk;
                  _report = report;
                  _generating = false;
                });
              }
            case AiFailed(:final message):
              setState(() {
                _aiError = _AiError.failed;
                _aiErrorDetail = message;
                _generating = false;
              });
          }
        });
  }

  List<_Item> _items() {
    final l10n = context.l10n;
    final q = _input.text.trim();
    final router = GoRouter.of(context);
    final items = <_Item>[
      if (q.isNotEmpty)
        _Item(
          group: _Group.ai,
          key: const ValueKey('palette-generate'),
          icon: Icons.auto_awesome_rounded,
          title: l10n.paletteGenerateCommand(q),
          subtitle: l10n.paletteGenerateSubtitle,
          onSelect: () => _generate(q),
        ),
      for (final h in _hits)
        _Item(
          group: _Group.snippets,
          key: ValueKey('palette-snippet-${h.snippet.name}'),
          icon: h.matchKind == SearchMatchKind.semantic ? Icons.psychology_rounded : Icons.code_rounded,
          title: h.snippet.name,
          subtitle: h.snippet.template,
          trailing: RiskBadge(risk: h.snippet.riskLevel, dense: true),
          onSelect: () async {
            final ran = await runSnippetFlow(context, ref, h.snippet);
            if (ran && mounted) _close();
          },
        ),
    ];
    final actions = <_Item>[
      _Item(
        group: _Group.actions,
        icon: Icons.add_rounded,
        title: l10n.paletteNewHost,
        onSelect: () {
          _close();
          router.go(AppRoutes.newHost);
        },
      ),
      _Item(
        group: _Group.actions,
        icon: Icons.code_rounded,
        title: l10n.navSnippets,
        onSelect: () {
          ref.read(workspaceToolsProvider.notifier).open(WorkspaceTool.snippets);
          _close();
        },
      ),
      _Item(
        group: _Group.actions,
        icon: Icons.auto_awesome_rounded,
        title: l10n.paletteOpenAiChat,
        onSelect: () {
          ref.read(workspaceToolsProvider.notifier).open(WorkspaceTool.ai);
          _close();
        },
      ),
      _Item(
        group: _Group.actions,
        icon: Icons.settings_rounded,
        title: l10n.navSettings,
        onSelect: () {
          _close();
          router.go(AppRoutes.settings);
        },
      ),
      _Item(
        group: _Group.actions,
        icon: Icons.lock_rounded,
        title: l10n.paletteLockVault,
        onSelect: () {
          final vault = ref.read(vaultServiceProvider);
          _close();
          unawaited(vault.lock());
        },
      ),
    ];
    return [...items, ...actions.where((a) => q.isEmpty || a.title.toLowerCase().contains(q.toLowerCase()))];
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent event, List<_Item> items) {
    if (event is! KeyDownEvent && event is! KeyRepeatEvent) return KeyEventResult.ignored;
    if (event.logicalKey == LogicalKeyboardKey.arrowDown) {
      setState(() => _selected = (_selected + 1).clamp(0, items.length - 1));
      return KeyEventResult.handled;
    }
    if (event.logicalKey == LogicalKeyboardKey.arrowUp) {
      setState(() => _selected = (_selected - 1).clamp(0, items.length - 1));
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  String _groupLabel(AppLocalizations l10n, _Group g) => switch (g) {
    _Group.ai => l10n.paletteGroupAi,
    _Group.snippets => l10n.navSnippets,
    _Group.actions => l10n.paletteGroupActions,
  };

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final l10n = context.l10n;
    final items = _items();
    final showAi = _generating || _command != null || _aiError != null || _streamed.isNotEmpty;
    final selected = _selected.clamp(0, items.isEmpty ? 0 : items.length - 1);
    // Rows grouped under caption headers, in item order.
    final rows = <Widget>[];
    _Group? group;
    for (final (i, item) in items.indexed) {
      if (item.group != group) {
        group = item.group;
        rows.add(_GroupHeader(label: _groupLabel(l10n, item.group)));
      }
      rows.add(
        _PaletteRow(
          item: item,
          selected: i == selected,
          onHover: () {
            if (_selected != i) setState(() => _selected = i);
          },
        ),
      );
    }
    final search = SizedBox(
      height: 44,
      child: DecoratedBox(
        decoration: ShapeDecoration(color: tokens.surfaces.fillField, shape: const StadiumBorder()),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s16),
          child: Row(
            children: [
              Icon(Icons.search_rounded, size: 20, color: tokens.secondaryLabel),
              const SizedBox(width: GlassSpacing.s8),
              Expanded(
                child: Focus(
                  onKeyEvent: (node, event) => _onKey(node, event, items),
                  child: TextField(
                    key: const ValueKey('palette-input'),
                    controller: _input,
                    focusNode: _focus,
                    autofocus: true,
                    style: tokens.typography.body.copyWith(fontSize: 17, height: 22 / 17, color: tokens.palette.label),
                    cursorColor: tokens.palette.accent,
                    decoration: InputDecoration.collapsed(
                      hintText: widget.selectedText == null ? l10n.paletteSearchHint : l10n.paletteAskSelectionHint,
                      hintStyle: tokens.typography.body.copyWith(fontSize: 17, color: tokens.secondaryLabel),
                    ).copyWith(filled: false),
                    onChanged: _onChanged,
                    onSubmitted: (_) {
                      if (items.isNotEmpty) items[selected].onSelect();
                      _focus.requestFocus();
                    },
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
    final body = Material(
      type: MaterialType.transparency,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          search,
          if (showAi) _aiPanel(context),
          if (rows.isNotEmpty)
            Flexible(
              child: ScrollEdgeEffect(
                bottom: true,
                extent: 16,
                child: ListView(
                  shrinkWrap: true,
                  padding: const EdgeInsets.only(top: GlassSpacing.s4, bottom: GlassSpacing.s8),
                  children: rows,
                ),
              ),
            ),
        ],
      ),
    );
    final shape = GlassRadii.shape(tokens.radii.palette);
    const padding = EdgeInsets.all(GlassSpacing.s8);
    final animation = GlassDialogScope.maybeOf(context)?.animation;
    final Widget surface = animation == null
        ? GlassSurface(variant: GlassVariant.thick, shape: shape, padding: padding, child: body)
        : AnimatedBuilder(
            animation: animation,
            builder: (context, child) {
              final reverse = animation.status == AnimationStatus.reverse;
              final presence = (reverse ? GlassMotion.dismiss : GlassMotion.appear).transform(animation.value);
              return GlassSurface(
                variant: GlassVariant.thick,
                shape: shape,
                padding: padding,
                backdrop: BackdropMode.live,
                overlay: true,
                presence: presence,
                child: child!,
              );
            },
            child: body,
          );
    final maxHeight = MediaQuery.sizeOf(context).height * 0.6;
    // The route pads 24; the palette sits 96 from the window top (§4.8).
    return Align(
      alignment: Alignment.topCenter,
      child: Padding(
        padding: const EdgeInsets.only(top: 72),
        child: ConstrainedBox(
          constraints: BoxConstraints(maxWidth: 640, maxHeight: maxHeight),
          child: Semantics(
            scopesRoute: true,
            namesRoute: true,
            explicitChildNodes: true,
            label: l10n.commandPalette,
            child: KeyedSubtree(key: const ValueKey('command-palette'), child: surface),
          ),
        ),
      ),
    );
  }

  Widget _aiPanel(BuildContext context) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final command = _command;
    final secondary = t.callout.copyWith(color: tokens.secondaryLabel);
    final danger = t.callout.copyWith(color: tokens.palette.danger);
    return Padding(
      padding: const EdgeInsets.fromLTRB(GlassSpacing.s8, GlassSpacing.s12, GlassSpacing.s8, GlassSpacing.s4),
      child: Column(
        key: const ValueKey('palette-ai-panel'),
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Icon(Icons.auto_awesome_rounded, size: 18, color: tokens.palette.accent),
              const SizedBox(width: GlassSpacing.s6),
              Text(l10n.paletteAiSuggestion, style: t.bodyEmph.copyWith(color: tokens.palette.label)),
              const Spacer(),
              if (_risk != null) RiskBadge(risk: _risk!.effective),
            ],
          ),
          const SizedBox(height: GlassSpacing.s8),
          if (_aiError case final error?) ...[
            Text(switch (error) {
              _AiError.noProvider => l10n.paletteErrorNoProvider,
              _AiError.failed => l10n.aiChatErrorRequestFailed,
            }, style: danger),
            if (error == _AiError.failed && (_aiErrorDetail?.isNotEmpty ?? false)) Text(_aiErrorDetail!, style: danger),
          ] else if (command == null)
            Text(
              _streamed.isEmpty ? l10n.paletteThinking : _streamed,
              style: t.body.copyWith(color: tokens.palette.label),
            )
          else ...[
            CommandActionsCard(command: command.command, suggestedRisk: command.suggestedRisk, onDone: _close),
            const SizedBox(height: GlassSpacing.s6),
            Text(command.explanation, style: secondary),
            if (_risk != null && _risk!.effective != command.suggestedRisk)
              Text(
                l10n.paletteRiskMismatch(command.suggestedRisk.localized(l10n), _risk!.effective.localized(l10n)),
                style: danger,
              ),
            if (_report != null) Text(describeSanitization(l10n, _report!), style: secondary),
          ],
        ],
      ),
    );
  }
}

class _GroupHeader extends StatelessWidget {
  const _GroupHeader({required this.label});

  final String label;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return Padding(
      padding: const EdgeInsets.fromLTRB(GlassSpacing.s12, GlassSpacing.s8, GlassSpacing.s12, GlassSpacing.s2),
      child: Semantics(
        header: true,
        child: Text(label, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
      ),
    );
  }
}

/// A result row (§4.8): 40 high, concentric radius (22 − 8 = 14), icon,
/// title (body/600), subtitle (callout), trailing risk badge. Selected: the
/// accent at α .14 / .22 with a 3 px accent leading bar.
class _PaletteRow extends StatelessWidget {
  const _PaletteRow({required this.item, required this.selected, required this.onHover});

  final _Item item;
  final bool selected;
  final VoidCallback onHover;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final radius = GlassRadii.concentric(tokens.radii.palette, GlassSpacing.s8);
    return Semantics(
      button: true,
      selected: selected,
      label: item.title,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => onHover(),
        child: GestureDetector(
          key: item.key,
          behavior: HitTestBehavior.opaque,
          onTap: item.onSelect,
          child: ExcludeSemantics(
            child: DecoratedBox(
              decoration: ShapeDecoration(
                color: selected
                    ? tokens.palette.accentFill.withValues(alpha: tokens.isDark ? 0.22 : 0.14)
                    : const Color(0x00000000),
                shape: GlassRadii.shape(radius),
              ),
              child: SizedBox(
                height: 40,
                child: Row(
                  children: [
                    SizedBox(
                      width: 3,
                      height: 20,
                      child: selected
                          ? DecoratedBox(
                              decoration: ShapeDecoration(color: tokens.palette.accent, shape: const StadiumBorder()),
                            )
                          : null,
                    ),
                    const SizedBox(width: GlassSpacing.s8),
                    Icon(item.icon, size: 18, color: selected ? tokens.palette.accent : tokens.secondaryLabel),
                    const SizedBox(width: GlassSpacing.s12),
                    Expanded(
                      child: Column(
                        mainAxisAlignment: MainAxisAlignment.center,
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            item.title,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: t.bodyEmph.copyWith(color: tokens.palette.label, height: 1.25),
                          ),
                          if (item.subtitle != null)
                            Text(
                              item.subtitle!,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: t.callout.copyWith(color: tokens.secondaryLabel, height: 1.25),
                            ),
                        ],
                      ),
                    ),
                    if (item.trailing != null) ...[const SizedBox(width: GlassSpacing.s8), item.trailing!],
                    const SizedBox(width: GlassSpacing.s8),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
