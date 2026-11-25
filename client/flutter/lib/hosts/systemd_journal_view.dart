part of 'systemd_services.dart';

class _UnitOutput extends StatelessWidget {
  const _UnitOutput({required this.future, required this.empty});
  final Future<Json> future;
  final String empty;

  @override
  Widget build(BuildContext context) => FutureBuilder<Json>(
    future: future,
    builder: (context, snapshot) {
      if (snapshot.connectionState != ConnectionState.done) return const LinearProgressIndicator();
      final l = context.l10n;
      if (snapshot.hasError) return SelectableText(errorMessage(l, snapshot.error!));
      final reply = snapshot.requireData;
      final error = _failure(l, reply);
      final output = (reply['output'] as String? ?? '').trimRight();
      return SelectableText(
        error ?? (output.isEmpty ? empty : output),
        style: GlassTokens.of(context).typography.callout.copyWith(fontFamily: 'monospace'),
      );
    },
  );
}

enum _LogFilter { all, errors, warnings, information, debug }

class _JournalSection extends StatefulWidget {
  const _JournalSection({required this.journal, super.key});
  final Future<_Journal> journal;
  @override
  State<_JournalSection> createState() => _JournalSectionState();
}

class _JournalSectionState extends State<_JournalSection> {
  final search = TextEditingController();
  _LogFilter level = _LogFilter.all;
  ServiceLogEntry? expanded;
  @override
  void dispose() {
    search.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return FutureBuilder<_Journal>(
      future: widget.journal,
      builder: (context, snapshot) {
        if (snapshot.connectionState != ConnectionState.done && !snapshot.hasData) {
          return const SliverToBoxAdapter(child: LinearProgressIndicator());
        }
        if (snapshot.hasError) return SliverToBoxAdapter(child: SelectableText(errorMessage(l, snapshot.error!)));
        final data = snapshot.requireData;
        final failure = _failure(l, data.reply);
        if (failure != null) return SliverToBoxAdapter(child: SelectableText(failure));
        final hint = (data.reply['diagnostic'] as String? ?? '').trim();
        final query = search.text.trim().toLowerCase();
        final rows = data.entries.where((e) {
          final matches = switch (level) {
            _LogFilter.all => true,
            _LogFilter.errors => e.level != null && e.level! <= 3,
            _LogFilter.warnings => e.level == 4,
            _LogFilter.information => e.level == 5 || e.level == 6,
            _LogFilter.debug => e.level == 7,
          };
          return matches &&
              (query.isEmpty || e.raw.toLowerCase().contains(query) || e.process.toLowerCase().contains(query));
        }).toList();
        final t = GlassTokens.of(context);
        final date = DateFormat('yyyy-MM-dd HH:mm:ss', l.localeName);
        return SliverMainAxisGroup(
          slivers: [
            SliverToBoxAdapter(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (hint.isNotEmpty) SelectableText(hint),
                  if (data.entries.isNotEmpty)
                    Row(
                      children: [
                        Expanded(
                          child: GlassField(
                            key: const ValueKey('systemd-log-search'),
                            controller: search,
                            search: true,
                            placeholder: l.systemdLogSearch,
                            onChanged: (_) => setState(() {}),
                          ),
                        ),
                        const SizedBox(width: GlassSpacing.s6),
                        Flexible(
                          child: GlassSelect<_LogFilter>(
                            value: level,
                            semanticLabel: l.kubeLogLevel,
                            key: const ValueKey('systemd-log-level'),
                            items: [
                              GlassSelectItem(value: _LogFilter.all, label: l.kubeLogAllLevels),
                              GlassSelectItem(value: _LogFilter.errors, label: l.systemdErrors),
                              GlassSelectItem(value: _LogFilter.warnings, label: l.systemdWarnings),
                              GlassSelectItem(value: _LogFilter.information, label: l.systemdInformation),
                              GlassSelectItem(value: _LogFilter.debug, label: l.systemdDebug),
                            ],
                            onChanged: (v) => setState(() => level = v),
                          ),
                        ),
                      ],
                    ),
                  if (rows.isEmpty)
                    Padding(
                      padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s8),
                      child: Text(data.entries.isEmpty ? l.systemdNoEntries : l.systemdNoMatchingLogs),
                    ),
                ],
              ),
            ),
            SliverList.builder(
              itemCount: rows.length,
              itemBuilder: (_, i) {
                final e = rows[i];
                final color = switch (e.level) {
                  0 || 1 || 2 || 3 => t.palette.danger,
                  4 => t.palette.warning,
                  5 || 6 => t.palette.accent,
                  _ => t.secondaryLabel,
                };
                const labels = ['EMERG', 'ALERT', 'CRIT', 'ERROR', 'WARN', 'NOTICE', 'INFO', 'DEBUG'];
                final open = identical(expanded, e) || (e.cursor != null && e.cursor == expanded?.cursor);
                return Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    const Divider(height: GlassSpacing.s8),
                    Material(
                      type: MaterialType.transparency,
                      child: InkWell(
                        onTap: () => setState(() => expanded = open ? null : e),
                        child: Padding(
                          padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s4),
                          child: Row(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Expanded(
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Wrap(
                                      spacing: GlassSpacing.s6,
                                      children: [
                                        if (e.level case final priority?)
                                          Text(
                                            labels[priority],
                                            style: t.typography.caption.copyWith(
                                              color: color,
                                              fontWeight: FontWeight.bold,
                                            ),
                                          ),
                                        if (e.time case final timestamp?)
                                          Text(
                                            '${date.format(timestamp)} UTC',
                                            style: t.typography.caption.copyWith(color: t.secondaryLabel),
                                          ),
                                        if (e.process.isNotEmpty)
                                          Text(
                                            e.process,
                                            style: t.typography.caption.copyWith(color: t.secondaryLabel),
                                          ),
                                      ],
                                    ),
                                    if (!open || e.raw != e.summary)
                                      Text(
                                        e.summary,
                                        maxLines: open ? null : 3,
                                        overflow: open ? null : TextOverflow.ellipsis,
                                        style: t.typography.callout,
                                      ),
                                  ],
                                ),
                              ),
                              Icon(open ? Icons.expand_less : Icons.expand_more, size: 16, color: t.secondaryLabel),
                            ],
                          ),
                        ),
                      ),
                    ),
                    if (open) ...[
                      for (final item in e.fields.entries)
                        Padding(
                          padding: const EdgeInsets.only(bottom: GlassSpacing.s4),
                          child: SelectableText.rich(
                            TextSpan(
                              style: t.typography.callout,
                              children: [
                                TextSpan(
                                  text: '${item.key}  ',
                                  style: TextStyle(color: t.secondaryLabel, fontWeight: FontWeight.bold),
                                ),
                                TextSpan(text: item.value),
                              ],
                            ),
                          ),
                        ),
                      if (e.raw != e.summary)
                        Text(l.systemdOriginal, style: t.typography.caption.copyWith(color: t.secondaryLabel)),
                      SelectableText(e.raw, style: t.typography.callout.copyWith(fontFamily: 'monospace')),
                    ],
                  ],
                );
              },
            ),
          ],
        );
      },
    );
  }
}
