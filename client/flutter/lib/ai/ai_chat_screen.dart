import 'package:consolecrypt/ai/ai_chat_controller.dart';
import 'package:consolecrypt/ai/code_blocks.dart';
import 'package:consolecrypt/ai/terminal_attachment.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

class AiChatScreen extends ConsumerStatefulWidget {
  const AiChatScreen({super.key, this.embedded = false});
  final bool embedded;

  @override
  ConsumerState<AiChatScreen> createState() => _AiChatScreenState();
}

class _AiChatScreenState extends ConsumerState<AiChatScreen> with WidgetsBindingObserver {
  final _input = TextEditingController();
  final _scroll = ScrollController();
  final _inputFocus = FocusNode();
  TerminalSessionId? _selectionSession;

  void _focusAttachment() => WidgetsBinding.instance.addPostFrameCallback((_) {
    if (mounted && ref.read(terminalAttachmentProvider) != null) _inputFocus.requestFocus();
  });

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _focusAttachment();
  }

  @override
  void didChangeMetrics() {
    if (mounted) setState(() {});
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _input.dispose();
    _scroll.dispose();
    _inputFocus.dispose();
    super.dispose();
  }

  void _send() {
    if (_input.text.trim().isEmpty || ref.read(aiChatControllerProvider).isStreaming) return;
    final tab = ref.read(terminalTabsProvider).active;
    final attachment = ref.read(terminalAttachmentProvider);
    final selection = attachment?.sessionId == tab?.sessionId && attachment != null
        ? attachment.text
        : _selectionSession == tab?.sessionId
        ? tab?.selectedText
        : null;
    ref
        .read(aiChatControllerProvider.notifier)
        .send(
          _input.text,
          context: AiContextSelection(hostId: tab?.host.id, selectedTerminalText: selection),
        );
    _input.clear();
    ref.read(terminalAttachmentProvider.notifier).clear();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scroll.hasClients) _scroll.jumpTo(_scroll.position.maxScrollExtent);
    });
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final state = ref.watch(aiChatControllerProvider);
    final controller = ref.read(aiChatControllerProvider.notifier);
    final providers = ref.watch(aiProvidersProvider).value ?? const <AiProviderConfig>[];
    final providerId = controller.effectiveProviderId(providers);
    final provider = providers.where((p) => p.id == providerId).firstOrNull;
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final activeTab = ref.watch(terminalTabsProvider.select((state) => state.active));
    // The mobile Scaffold has already consumed the MediaQuery bottom inset.
    final keyboardOpen = View.of(context).viewInsets.bottom > 0;
    final compactKeyboard = MediaQuery.sizeOf(context).width < 600 && keyboardOpen;
    final attachment = ref.watch(terminalAttachmentProvider);
    ref.listen(terminalAttachmentProvider, (previous, next) {
      if (next != null) {
        _selectionSession = null;
        _focusAttachment();
      }
    });
    ref.listen(terminalTabsProvider.select((state) => state.active?.sessionId), (previous, next) {
      if (previous != next) setState(() => _selectionSession = null);
    });
    // Streaming answers repaint constantly: all chrome stays static here (§3).
    return GlassBlurSuppressor(
      budget: GlassScope.of(context).budget,
      child: PageScaffold(
        embedded: widget.embedded,
        title: l10n.navAiChat,
        subtitle: keyboardOpen ? null : l10n.aiChatSubtitle,
        actions: [
          if (providers.isNotEmpty && providerId != null)
            GlassSelect<ObjectId>(
              key: const ValueKey('ai-provider'),
              value: providerId,
              semanticLabel: l10n.aiChatProviderLabel,
              items: [for (final p in providers) GlassSelectItem(value: p.id, label: '${p.name} · ${p.chatModel}')],
              onChanged: controller.selectProvider,
            ),
          GlassIconButton(
            key: const ValueKey('ai-new-conversation'),
            tooltip: l10n.aiChatNewConversation,
            icon: Icons.refresh_rounded,
            onPressed: () {
              controller.clear();
              ref.read(terminalAttachmentProvider.notifier).clear();
            },
          ),
        ],
        body: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (provider != null)
              Padding(
                padding: const EdgeInsets.only(bottom: GlassSpacing.s8),
                child: Row(
                  children: [
                    Icon(
                      provider.isRemote ? Icons.public_rounded : Icons.computer_rounded,
                      size: 16,
                      color: tokens.secondaryLabel,
                    ),
                    const SizedBox(width: GlassSpacing.s6),
                    Expanded(
                      child: Text(
                        provider.isRemote
                            ? l10n.aiChatRemoteProviderNotice(
                                provider.privacyProfile.localized(l10n),
                                provider.privacyProfile.localizedDescription(l10n),
                              )
                            : l10n.aiChatLocalModelNotice,
                        style: t.callout.copyWith(color: tokens.secondaryLabel),
                      ),
                    ),
                  ],
                ),
              )
            else
              Padding(
                padding: const EdgeInsets.only(bottom: GlassSpacing.s8),
                child: InfoBanner(
                  tone: BannerTone.warning,
                  message: l10n.aiChatNoProviderBanner,
                  action: TextButton(onPressed: () => context.go(AppRoutes.settings), child: Text(l10n.navSettings)),
                ),
              ),
            Expanded(
              child: ContentSurface(
                padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s12),
                child: state.messages.isEmpty && !state.isStreaming
                    ? EmptyState(
                        icon: Icons.auto_awesome_rounded,
                        title: l10n.aiChatEmptyTitle,
                        message: l10n.aiChatEmptyMessage,
                      )
                    : RepaintBoundary(
                        child: ScrollEdgeEffect(
                          bottom: true,
                          extent: 12,
                          child: ListView(
                            controller: _scroll,
                            padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s8),
                            children: [
                              for (final (i, m) in state.messages.indexed)
                                _Bubble(message: m, stopped: state.stopped.contains(i)),
                              if (state.isStreaming)
                                _Bubble(
                                  message: ChatMessage.now(
                                    ChatRole.assistant,
                                    state.streaming!.isEmpty ? '…' : state.streaming!,
                                  ),
                                  streaming: true,
                                ),
                            ],
                          ),
                        ),
                      ),
              ),
            ),
            if (state.error case final error?)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s6),
                child: _errorBanner(l10n, error, state.errorDetail),
              ),
            if (state.lastReport != null && !state.isStreaming)
              Padding(
                padding: const EdgeInsets.only(top: GlassSpacing.s6),
                child: Text(
                  describeSanitization(l10n, state.lastReport!),
                  style: t.callout.copyWith(color: tokens.secondaryLabel),
                ),
              ),
            const SizedBox(height: GlassSpacing.s8),
            Row(
              crossAxisAlignment: CrossAxisAlignment.end,
              children: [
                Expanded(
                  child: CallbackShortcuts(
                    bindings: {const SingleActivator(LogicalKeyboardKey.enter): _send},
                    child: TextField(
                      key: const ValueKey('ai-input'),
                      controller: _input,
                      focusNode: _inputFocus,
                      minLines: 1,
                      maxLines: 6,
                      decoration: InputDecoration(
                        hintText: MediaQuery.sizeOf(context).width < 600
                            ? l10n.mobileMessageHint
                            : l10n.aiChatInputHint,
                      ),
                    ),
                  ),
                ),
                const SizedBox(width: GlassSpacing.s8),
                if (state.isStreaming)
                  GlassButton(
                    key: const ValueKey('ai-stop'),
                    size: GlassControlSize.lg,
                    onPressed: controller.stop,
                    icon: Icons.stop_rounded,
                    label: l10n.aiChatStop,
                  )
                else
                  GlassButton.prominent(
                    key: const ValueKey('ai-send'),
                    size: GlassControlSize.lg,
                    onPressed: _send,
                    icon: Icons.send_rounded,
                    label: l10n.aiChatSend,
                  ),
              ],
            ),
            if (attachment != null)
              Padding(
                padding: const EdgeInsets.only(top: GlassSpacing.s8),
                child: ContentSurface(
                  key: const ValueKey('ai-terminal-attachment'),
                  padding: EdgeInsets.symmetric(
                    horizontal: GlassSpacing.s8,
                    vertical: compactKeyboard ? 0 : GlassSpacing.s8,
                  ),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Row(
                        children: [
                          const Icon(Icons.terminal_rounded, size: 16),
                          const SizedBox(width: GlassSpacing.s6),
                          Expanded(
                            child: Text(
                              l10n.aiTerminalAttachment(attachment.hostLabel),
                              style: t.callout,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                            ),
                          ),
                          GlassIconButton(
                            key: const ValueKey('ai-terminal-attachment-remove'),
                            icon: Icons.close_rounded,
                            tooltip: l10n.aiRemoveAttachment,
                            size: 28,
                            onPressed: () => ref.read(terminalAttachmentProvider.notifier).clear(),
                          ),
                        ],
                      ),
                      if (!compactKeyboard)
                        Text(
                          attachment.text,
                          key: const ValueKey('ai-terminal-attachment-text'),
                          maxLines: 3,
                          overflow: TextOverflow.ellipsis,
                          style: t.mono.copyWith(fontSize: 11),
                        ),
                    ],
                  ),
                ),
              )
            else if (activeTab != null)
              ListenableBuilder(
                // xterm selection changes do not update terminalTabsProvider.
                // Rebuild only this control, including while the shell is idle.
                listenable: activeTab.controller,
                builder: (context, _) => _selectionControl(activeTab),
              )
            else
              _selectionControl(null),
          ],
        ),
      ),
    );
  }

  Widget _selectionControl(TerminalTab? tab) {
    final hasSelection = tab?.selectedText != null;
    final tokens = GlassTokens.of(context);
    return CheckboxListTile(
      key: const ValueKey('ai-attach-selection'),
      contentPadding: EdgeInsets.zero,
      controlAffinity: ListTileControlAffinity.leading,
      dense: true,
      value: hasSelection && _selectionSession == tab?.sessionId,
      onChanged: hasSelection
          ? (value) => setState(() => _selectionSession = value == true ? tab!.sessionId : null)
          : null,
      title: Text(
        hasSelection ? context.l10n.aiChatIncludeSelection : context.l10n.aiChatSelectToInclude,
        style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
      ),
    );
  }
}

/// Localized error; the provider's raw diagnostic (if any) is secondary.
Widget _errorBanner(AppLocalizations l10n, AiChatError error, String? detail) => switch (error) {
  AiChatError.noProvider => InfoBanner(tone: BannerTone.danger, message: l10n.aiChatErrorNoProvider),
  AiChatError.requestFailed when detail != null && detail.isNotEmpty => InfoBanner(
    tone: BannerTone.danger,
    title: l10n.aiChatErrorRequestFailed,
    message: detail,
  ),
  AiChatError.requestFailed => InfoBanner(tone: BannerTone.danger, message: l10n.aiChatErrorRequestFailed),
};

class _Bubble extends StatelessWidget {
  const _Bubble({required this.message, this.streaming = false, this.stopped = false});

  final ChatMessage message;
  final bool streaming;

  /// The answer was cut short by Stop; a localized marker is appended.
  final bool stopped;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final user = message.role == ChatRole.user;
    final shape = GlassRadii.shape(tokens.radii.card);
    return Align(
      alignment: user ? AlignmentDirectional.centerEnd : AlignmentDirectional.centerStart,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 760),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s6),
          child: DecoratedBox(
            decoration: ShapeDecoration(
              color: user ? tokens.rowSelection : tokens.surfaces.contentSolid,
              shape: user ? shape : shape.copyWith(side: BorderSide(color: tokens.surfaces.hairlineCard)),
            ),
            child: Padding(
              padding: const EdgeInsets.all(GlassSpacing.s12),
              child: user
                  ? SelectableText(message.content)
                  : Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        AnswerView(
                          text: stopped ? '${message.content} ${context.l10n.aiChatStoppedSuffix}' : message.content,
                        ),
                        if (streaming) const LinearProgressIndicator(minHeight: 2),
                      ],
                    ),
            ),
          ),
        ),
      ),
    );
  }
}
