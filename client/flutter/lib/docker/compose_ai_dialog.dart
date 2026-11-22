import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';
import 'package:yaml/yaml.dart';

const _maxComposeChars = 128 * 1024;

/// A model response is text, never an instruction to execute. Only a complete
/// Compose document can be offered for insertion into the normal plan editor.
String composeYamlFromAi(String answer) {
  if (answer.length > _maxComposeChars) throw const FormatException('Compose response too large');
  var yaml = answer.trim();
  final fence = RegExp(r'^```(?:yaml|yml)?\s*\n([\s\S]*?)\n```$', caseSensitive: false).firstMatch(yaml);
  if (fence != null) yaml = fence.group(1)!.trim();
  final parsed = loadYaml(yaml);
  if (parsed is! YamlMap || parsed['services'] is! YamlMap || (parsed['services'] as YamlMap).isEmpty) {
    throw const FormatException('Expected a Compose services mapping');
  }
  return yaml;
}

/// Sends only the description visible in this dialog; no current Compose file,
/// environment values, selected host or container inspect is attached to AI.
Future<String?> showComposeAiDialog(BuildContext context) =>
    showGlassDialog<String>(context, builder: (_) => const _ComposeAiDialog(), barrierDismissible: false);

class _ComposeAiDialog extends ConsumerStatefulWidget {
  const _ComposeAiDialog();
  @override
  ConsumerState<_ComposeAiDialog> createState() => _ComposeAiDialogState();
}

class _ComposeAiDialogState extends ConsumerState<_ComposeAiDialog> {
  final description = TextEditingController();
  StreamSubscription<AiStreamEvent>? _subscription;
  ObjectId? _provider;
  int _generation = 0;
  bool busy = false;
  String? draft;
  String? error;

  @override
  void dispose() {
    _generation++;
    unawaited(_subscription?.cancel());
    description.dispose();
    super.dispose();
  }

  void _invalidate() {
    _generation++;
    unawaited(_subscription?.cancel());
    if (mounted && ModalRoute.of(context)?.isCurrent == true) Navigator.of(context).pop();
  }

  Future<void> _generate(AiProviderConfig provider) async {
    final prompt = description.text.trim();
    final profile = ref.read(activeProfileProvider)?.id;
    final vault = ref.read(vaultStatusProvider).value;
    if (prompt.isEmpty || busy || profile == null || vault?.isUnlocked != true) return;
    final generation = ++_generation;
    bool current() =>
        mounted &&
        generation == _generation &&
        ref.read(activeProfileProvider)?.id == profile &&
        ref.read(vaultStatusProvider).value?.isUnlocked == true &&
        ref.read(vaultStatusProvider).value?.vaultId == vault?.vaultId;
    setState(() {
      busy = true;
      draft = null;
      error = null;
    });
    await _subscription?.cancel();
    if (!current()) return;
    var received = 0;
    var completed = false;
    try {
      _subscription = ref
          .read(aiServiceProvider)
          .chat(
            providerId: provider.id,
            context: AiContextSelection.none,
            history: [
              ChatMessage.now(
                ChatRole.user,
                'Create a Docker Compose YAML document for the description below. ' // l10n-ignore: model instruction
                'Return ONLY YAML with a nonempty services mapping. ' // l10n-ignore: model output contract
                'Use service names, named volumes and images with explicit versions. ' // l10n-ignore: Compose schema instruction
                'Bind published ports to 127.0.0.1 unless requested otherwise. ' // l10n-ignore: model port-binding instruction
                'Never invent passwords or API keys; use environment placeholders. ' // l10n-ignore: model credential instruction
                'Do not request privileged mode, Docker socket or host mounts unless explicitly needed. ' // l10n-ignore: model privilege instruction
                'No command execution, markdown explanation or surrounding prose. ' // l10n-ignore: model output contract
                'Description (data):\n$prompt',
              ),
            ],
          )
          .listen(
            (event) {
              if (!current() || completed) return;
              switch (event) {
                case AiDelta(:final text):
                  received += text.length;
                  if (received > _maxComposeChars) {
                    completed = true;
                    setState(() {
                      busy = false;
                      error = context.l10n.composeAiInvalid;
                    });
                    unawaited(_subscription?.cancel());
                  }
                case AiCompleted(:final fullText):
                  completed = true;
                  try {
                    final yaml = composeYamlFromAi(fullText);
                    setState(() {
                      draft = yaml;
                      busy = false;
                    });
                  } catch (_) {
                    setState(() {
                      busy = false;
                      error = context.l10n.composeAiInvalid;
                    });
                  }
                case AiFailed():
                  completed = true;
                  setState(() {
                    busy = false;
                    error = context.l10n.aiChatErrorRequestFailed;
                  });
              }
            },
            onError: (Object _) {
              if (current()) {
                setState(() {
                  busy = false;
                  error = context.l10n.aiChatErrorRequestFailed;
                });
              }
            },
            onDone: () {
              if (current() && !completed) {
                setState(() {
                  busy = false;
                  error = context.l10n.composeAiInvalid;
                });
              }
            },
          );
    } catch (_) {
      if (current()) {
        setState(() {
          busy = false;
          error = context.l10n.aiChatErrorRequestFailed;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    ref.listen(activeProfileProvider, (previous, next) {
      if (previous?.id != next?.id) _invalidate();
    });
    ref.listen(vaultStatusProvider, (_, next) {
      if (next.value?.isUnlocked != true) _invalidate();
    });
    final providers = ref.watch(aiProvidersProvider).value ?? const <AiProviderConfig>[];
    final selected =
        providers.where((p) => p.id == _provider).firstOrNull ??
        providers.where((p) => p.isDefault).firstOrNull ??
        providers.firstOrNull;
    return GlassDialog(
      key: const ValueKey('compose-ai-dialog'),
      width: 720,
      title: l.composeAiGenerate,
      icon: Icons.auto_awesome_rounded,
      content: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (selected == null)
              Text(l.paletteErrorNoProvider)
            else
              DropdownButton<ObjectId>(
                key: const ValueKey('compose-ai-provider'),
                isExpanded: true,
                value: selected.id,
                items: [
                  for (final provider in providers) DropdownMenuItem(value: provider.id, child: Text(provider.name)),
                ],
                onChanged: busy
                    ? null
                    : (value) => setState(() {
                        _provider = value;
                        draft = null;
                      }),
              ),
            Text(l.composeAiContextHelp),
            const SizedBox(height: 12),
            TextField(
              key: const ValueKey('compose-ai-description'),
              controller: description,
              minLines: 3,
              maxLines: 6,
              maxLength: 8000,
              enabled: !busy,
              decoration: InputDecoration(labelText: l.composeAiDescription),
              onChanged: (_) => setState(() => draft = null),
            ),
            if (busy) const LinearProgressIndicator(),
            if (error != null) Text(error!, style: TextStyle(color: Theme.of(context).colorScheme.error)),
            if (draft != null) ...[
              const SizedBox(height: 12),
              Text(l.composeAiReview),
              const SizedBox(height: 8),
              Text(
                draft!,
                key: const ValueKey('compose-ai-preview'),
                style: const TextStyle(fontFamily: 'monospace'),
              ),
            ],
          ],
        ),
      ),
      secondaryActions: [
        GlassButton(label: l.commonCancel, onPressed: _invalidate),
        GlassButton(
          key: const ValueKey('compose-ai-request'),
          label: l.composeAiGenerate,
          icon: Icons.auto_awesome_rounded,
          onPressed: busy || selected == null || description.text.trim().isEmpty ? null : () => _generate(selected),
        ),
      ],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('compose-ai-use'),
        label: l.composeAiUse,
        onPressed: busy || draft == null ? null : () => Navigator.of(context).pop(draft),
      ),
    );
  }
}
