import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Takes an API key: the secure material (§4.4, §4.7).
Future<void> showAiProviderDialog(BuildContext context, {AiProviderConfig? provider}) =>
    showAppDialog<void>(context, secure: true, builder: (_) => AiProviderDialog(provider: provider));

/// Ollama / LM Studio / DeepSeek / OpenAI-compatible provider settings. The
/// API key is write-only: an existing key is never shown or returned.
class AiProviderDialog extends ConsumerStatefulWidget {
  const AiProviderDialog({super.key, this.provider});

  final AiProviderConfig? provider;

  @override
  ConsumerState<AiProviderDialog> createState() => _AiProviderDialogState();
}

class _AiProviderDialogState extends ConsumerState<AiProviderDialog> {
  late AiProviderConfig _config = widget.provider ?? AiProviderConfig.draft(AiProviderKind.ollama);
  late final TextEditingController _name = TextEditingController(text: _config.name);
  late final TextEditingController _baseUrl = TextEditingController(text: _config.baseUrl);
  late final TextEditingController _model = TextEditingController(text: _config.chatModel);
  late final TextEditingController _embedding = TextEditingController(text: _config.embeddingModel ?? '');
  late final TextEditingController _timeout = TextEditingController(text: '${_config.timeoutSecs}');
  final _apiKey = TextEditingController();
  bool _clearKey = false;
  AppException? _error;
  ProviderHealth? _health;
  bool _busy = false;

  @override
  void dispose() {
    for (final c in [_name, _baseUrl, _model, _embedding, _timeout, _apiKey]) {
      c.dispose();
    }
    super.dispose();
  }

  void _setKind(AiProviderKind kind) {
    final l10n = context.l10n;
    setState(() {
      _config = _config.copyWith(
        provider: kind,
        privacyProfile: kind.isLocalByDefault ? PrivacyProfile.local : PrivacyProfile.strict,
      );
      if (_name.text.isEmpty || AiProviderKind.values.any((k) => k.localized(l10n) == _name.text)) {
        _name.text = kind.localized(l10n);
      }
      _baseUrl.text = kind.defaultBaseUrl;
      _model.text = kind.defaultModel;
    });
  }

  AiProviderConfig _build() => _config.copyWith(
    name: _name.text.trim(),
    baseUrl: _baseUrl.text.trim(),
    chatModel: _model.text.trim(),
    embeddingModel: _embedding.text.trim().isEmpty ? null : _embedding.text.trim(),
    clearEmbeddingModel: _embedding.text.trim().isEmpty,
    timeoutSecs: int.tryParse(_timeout.text) ?? 60,
  );

  Future<void> _save({bool andTest = false}) async {
    setState(() {
      _busy = true;
      _error = null;
    });
    final key = _apiKey.text.isEmpty ? null : SecretText(_apiKey.text);
    try {
      final saved = await ref.read(aiServiceProvider).saveProvider(_build(), apiKey: key, clearApiKey: _clearKey);
      _apiKey.clear();
      _config = saved;
      if (andTest) {
        final health = await ref.read(aiServiceProvider).testProvider(saved.id);
        if (mounted) setState(() => _health = health);
      } else if (mounted) {
        closeDialog<void>(context);
      }
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      key?.wipe();
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final uri = Uri.tryParse(_baseUrl.text.trim());
    final insecureRemote =
        uri != null && uri.scheme == 'http' && !const {'localhost', '127.0.0.1', '::1'}.contains(uri.host);
    final remote = uri != null && !const {'localhost', '127.0.0.1', '::1'}.contains(uri.host);
    return GlassDialog(
      key: const ValueKey('provider-dialog'),
      icon: Icons.auto_awesome_rounded,
      title: widget.provider == null
          ? l10n.aiProviderDialogAddTitle
          : l10n.aiProviderDialogEditTitle(widget.provider!.name),
      width: 620,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            DropdownButtonFormField<AiProviderKind>(
              isExpanded: true,
              borderRadius: BorderRadius.circular(tokens.radii.menu),
              key: const ValueKey('provider-kind'),
              initialValue: _config.provider,
              decoration: InputDecoration(labelText: l10n.aiProviderDialogProviderLabel),
              items: [
                for (final k in AiProviderKind.values) DropdownMenuItem(value: k, child: Text(k.localized(l10n))),
              ],
              onChanged: (k) => k == null ? null : _setKind(k),
            ),
            const SizedBox(height: GlassSpacing.s12),
            TextField(
              controller: _name,
              decoration: InputDecoration(labelText: l10n.aiProviderDialogNameLabel),
            ),
            const SizedBox(height: GlassSpacing.s12),
            TextField(
              key: const ValueKey('provider-base-url'),
              controller: _baseUrl,
              keyboardType: TextInputType.url,
              decoration: InputDecoration(labelText: l10n.aiProviderDialogBaseUrlLabel),
              onChanged: (_) => setState(() {}),
            ),
            if (insecureRemote)
              Padding(
                padding: const EdgeInsets.only(top: GlassSpacing.s8),
                child: InfoBanner(tone: BannerTone.danger, message: l10n.aiProviderDialogInsecureHttp),
              ),
            const SizedBox(height: GlassSpacing.s12),
            Row(
              children: [
                Expanded(
                  child: TextField(
                    controller: _model,
                    decoration: InputDecoration(labelText: l10n.aiProviderDialogChatModelLabel),
                  ),
                ),
                const SizedBox(width: GlassSpacing.s12),
                Expanded(
                  child: TextField(
                    controller: _embedding,
                    decoration: InputDecoration(labelText: l10n.aiProviderDialogEmbeddingModelLabel),
                  ),
                ),
              ],
            ),
            const SizedBox(height: GlassSpacing.s12),
            SecretField(
              key: const ValueKey('provider-api-key'),
              controller: _apiKey,
              label: l10n.aiProviderDialogApiKeyLabel,
              hint: _config.hasApiKey ? l10n.aiProviderDialogApiKeyStoredHint : l10n.aiProviderDialogApiKeyOptionalHint,
              helper: l10n.aiProviderDialogApiKeyHelp,
            ),
            if (_config.hasApiKey)
              CheckboxListTile(
                contentPadding: EdgeInsets.zero,
                controlAffinity: ListTileControlAffinity.leading,
                value: _clearKey,
                onChanged: (v) => setState(() => _clearKey = v ?? false),
                title: Text(l10n.aiProviderDialogRemoveApiKey),
              ),
            const SizedBox(height: GlassSpacing.s12),
            Text(l10n.aiProviderDialogPrivacyProfileLabel, style: t.bodyEmph.copyWith(color: tokens.palette.label)),
            const SizedBox(height: GlassSpacing.s6),
            GlassSegmented<PrivacyProfile>(
              key: const ValueKey('provider-privacy'),
              inChrome: false,
              expand: true,
              segments: [for (final p in PrivacyProfile.values) GlassSegment(value: p, label: p.localized(l10n))],
              selected: _config.privacyProfile,
              onChanged: (p) => setState(() => _config = _config.copyWith(privacyProfile: p)),
            ),
            const SizedBox(height: GlassSpacing.s4),
            Text(
              _config.privacyProfile.localizedDescription(l10n),
              style: t.callout.copyWith(color: tokens.secondaryLabel),
            ),
            if (remote && _config.privacyProfile == PrivacyProfile.local)
              Padding(
                padding: const EdgeInsets.only(top: GlassSpacing.s8),
                child: InfoBanner(
                  tone: BannerTone.warning,
                  message: l10n.aiProviderDialogLocalProfileWarning(PrivacyProfile.local.localized(l10n)),
                ),
              ),
            const SizedBox(height: GlassSpacing.s12),
            Row(
              children: [
                SizedBox(
                  width: 140,
                  child: TextField(
                    controller: _timeout,
                    inputFormatters: [FilteringTextInputFormatter.digitsOnly],
                    decoration: InputDecoration(labelText: l10n.aiProviderDialogTimeoutLabel),
                  ),
                ),
                const SizedBox(width: GlassSpacing.s12),
                Expanded(
                  child: Wrap(
                    spacing: GlassSpacing.s8,
                    runSpacing: GlassSpacing.s6,
                    children: [
                      FilterChip(
                        label: Text(l10n.aiProviderDialogStreaming),
                        selected: _config.streaming,
                        onSelected: (v) => setState(() => _config = _config.copyWith(streaming: v)),
                      ),
                      FilterChip(
                        label: Text(l10n.aiProviderDialogToolCalling),
                        selected: _config.toolSupport,
                        onSelected: (v) => setState(() => _config = _config.copyWith(toolSupport: v)),
                      ),
                      FilterChip(
                        label: Text(l10n.aiProviderDialogDefault),
                        selected: _config.isDefault,
                        onSelected: (v) => setState(() => _config = _config.copyWith(isDefault: v)),
                      ),
                    ],
                  ),
                ),
              ],
            ),
            if (_health != null) ...[
              const SizedBox(height: GlassSpacing.s12),
              InfoBanner(
                tone: _health!.ok ? BannerTone.success : BannerTone.danger,
                // Localized status; the provider's own (English) diagnostic stays as detail.
                title: _health!.ok ? l10n.aiProviderDialogHealthOk : l10n.aiProviderDialogHealthFailed,
                message: [
                  _health!.message,
                  if (_health!.models.isNotEmpty) l10n.aiProviderDialogHealthModels(_health!.models.join(', ')),
                ].join('\n'),
              ),
            ],
            if (_error != null) ...[
              const SizedBox(height: GlassSpacing.s12),
              GateErrorText(text: errorMessage(l10n, _error!)),
            ],
          ],
        ),
      ),
      leadingAction: widget.provider == null
          ? null
          : GlassButton(
              key: const ValueKey('delete-provider'),
              style: GlassButtonStyle.destructiveQuiet,
              icon: Icons.delete_rounded,
              onPressed: _busy
                  ? null
                  : () async {
                      await ref.read(aiServiceProvider).deleteProvider(widget.provider!.id);
                      if (context.mounted) closeDialog<void>(context);
                    },
              label: l10n.commonDelete,
            ),
      secondaryActions: [
        GlassButton(onPressed: () => closeDialog<void>(context), label: l10n.commonCancel),
        GlassButton(
          key: const ValueKey('save-test-provider'),
          onPressed: _busy ? null : () => _save(andTest: true),
          label: l10n.aiProviderDialogSaveAndTest,
        ),
      ],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('save-provider'),
        busy: _busy,
        onPressed: _busy ? null : _save,
        label: l10n.commonSave,
      ),
    );
  }
}
