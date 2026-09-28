import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/snippets/run_flow.dart';
import 'package:consolecrypt/snippets/snippet_editor.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// A segment of an AI answer: prose or a fenced code block.
sealed class AnswerSegment {
  const AnswerSegment();
}

final class TextSegment extends AnswerSegment {
  const TextSegment(this.text);

  final String text;
}

final class CodeSegment extends AnswerSegment {
  const CodeSegment(this.code, {this.language});

  final String code;
  final String? language;
}

/// Splits markdown-ish text on ``` fences (unit-tested). An unterminated
/// fence (still streaming) is treated as code.
List<AnswerSegment> parseAnswer(String text) {
  final segments = <AnswerSegment>[];
  final fence = RegExp(r'```([\w+-]*)[ \t]*\n?');
  var index = 0;
  while (index < text.length) {
    final open = fence.firstMatch(text.substring(index));
    if (open == null) {
      segments.add(TextSegment(text.substring(index)));
      break;
    }
    final before = text.substring(index, index + open.start);
    if (before.trim().isNotEmpty) segments.add(TextSegment(before));
    final codeStart = index + open.end;
    final close = text.indexOf('```', codeStart);
    final lang = open.group(1)!.isEmpty ? null : open.group(1);
    if (close < 0) {
      segments.add(CodeSegment(text.substring(codeStart).trimRight(), language: lang));
      break;
    }
    segments.add(CodeSegment(text.substring(codeStart, close).trimRight(), language: lang));
    index = close + 3;
  }
  return segments.where((s) => s is! TextSegment || s.text.trim().isNotEmpty).toList();
}

/// Command card with Copy / Insert / Run… / Save as snippet. Never runs by
/// itself: Run goes through the confirmation dialog (§16).
class CommandActionsCard extends ConsumerWidget {
  const CommandActionsCard({required this.command, super.key, this.suggestedRisk = RiskLevel.unknown, this.onDone});

  final String command;

  /// The model's own guess (a hint; local rules decide).
  final RiskLevel suggestedRisk;

  /// Called after an action that should close the surrounding overlay.
  final VoidCallback? onDone;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    final hasTab = ref.watch(terminalTabsProvider).active != null;
    // Opaque `surface.inset` block (also inside the palette's glass: a fill,
    // never glass on glass); actions are plain buttons.
    return ContentSurface(
      kind: ContentSurfaceKind.inset,
      padding: const EdgeInsets.fromLTRB(GlassSpacing.s12, GlassSpacing.s12, GlassSpacing.s8, GlassSpacing.s6),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SelectableText(command, style: tokens.typography.mono.copyWith(color: tokens.palette.label)),
          const SizedBox(height: GlassSpacing.s6),
          Wrap(
            spacing: GlassSpacing.s2,
            runSpacing: GlassSpacing.s2,
            children: [
              GlassButton.plain(
                size: GlassControlSize.sm,
                onPressed: () => copyPlainWithNotice(context, ref, command, what: l10n.copyWhatCommand),
                icon: Icons.copy_rounded,
                label: l10n.commonCopy,
              ),
              GlassButton.plain(
                key: const ValueKey('ai-insert'),
                size: GlassControlSize.sm,
                onPressed: hasTab
                    ? () {
                        ref.read(terminalTabsProvider.notifier).insertIntoActive(command);
                        onDone?.call();
                      }
                    : null,
                icon: Icons.keyboard_return_rounded,
                label: l10n.codeBlocksInsert,
              ),
              GlassButton.plain(
                key: const ValueKey('ai-run'),
                size: GlassControlSize.sm,
                onPressed: () async {
                  // Run on top of the overlay, then close it (its context
                  // must stay alive while the confirmation is shown).
                  final ran = await runCommandFlow(
                    context,
                    ref,
                    command: command,
                    declared: suggestedRisk,
                    source: SnippetSource.ai,
                  );
                  if (ran) onDone?.call();
                },
                icon: Icons.play_arrow_rounded,
                label: l10n.codeBlocksRun,
              ),
              GlassButton.plain(
                key: const ValueKey('ai-save-snippet'),
                size: GlassControlSize.sm,
                onPressed: () async {
                  final draft = await runWithFeedback(
                    context,
                    () => ref.read(aiServiceProvider).draftSnippet(command: command),
                  );
                  if (draft == null || !context.mounted) return;
                  final saved = await showSnippetEditor(context, draft: draft);
                  if (saved != null && context.mounted) {
                    showSnack(context, context.l10n.codeBlocksSnippetSaved(saved.name), tone: GlassTone.success);
                  }
                },
                icon: Icons.bookmark_add_rounded,
                label: l10n.codeBlocksSaveAsSnippet,
              ),
            ],
          ),
        ],
      ),
    );
  }
}

/// Renders an answer with code blocks turned into [CommandActionsCard]s.
class AnswerView extends StatelessWidget {
  const AnswerView({required this.text, super.key});

  final String text;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final s in parseAnswer(text))
          Padding(
            padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s4),
            child: switch (s) {
              TextSegment(:final text) => SelectableText(text.trim()),
              CodeSegment(:final code) => CommandActionsCard(command: code),
            },
          ),
      ],
    );
  }
}

/// Privacy line for an AI answer (§15).
String describeSanitization(AppLocalizations l10n, SanitizationReport r) {
  final profile = r.profile.localized(l10n);
  if (!r.remote) return l10n.codeBlocksProcessedLocally(profile);
  return l10n.codeBlocksSanitized(r.redactions, profile);
}
