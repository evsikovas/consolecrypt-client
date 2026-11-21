import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/shared/ports.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:material_ui/material_ui.dart';

final _secretKey = RegExp(r'PASS|SECRET|TOKEN|KEY|CREDENTIAL|AUTH|PRIVATE|CERT', caseSensitive: false);

bool looksSecret(String key) => _secretKey.hasMatch(key);

/// Container environment with values hidden until asked, one by one or all at once.
class EnvPanel extends StatefulWidget {
  const EnvPanel({super.key, required this.containerId});
  final String containerId;
  @override
  State<EnvPanel> createState() => _EnvPanelState();
}

class _EnvPanelState extends State<EnvPanel> {
  late final api = WslcApiScope.of(context);

  List<EnvEntry>? entries;
  String? error;
  final search = TextEditingController();
  final Set<String> shown = {};
  bool showAll = false;
  bool hideInherited = false;

  @override
  void initState() {
    super.initState();
    load();
  }

  Future<void> load() async {
    try {
      final result = await api.containerEnv(containerId: widget.containerId);
      if (mounted) setState(() => entries = result);
    } catch (e) {
      if (mounted) setState(() => error = errorText(e));
    }
  }

  @override
  void dispose() {
    search.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final all = entries;
    if (all == null) {
      return Panel(
        child: EmptyState(
          icon: Icons.error_outline_rounded,
          loading: error == null,
          title: error == null ? 'Чтение окружения…' : 'Не удалось прочитать окружение',
          hint: error,
        ),
      );
    }
    final query = search.text.trim().toLowerCase();
    final rows = all
        .where((e) => !hideInherited || !e.fromImage)
        .where(
          (e) =>
              query.isEmpty ||
              e.key.toLowerCase().contains(query) ||
              (showAll || shown.contains(e.key)) && e.value.toLowerCase().contains(query),
        )
        .toList();
    final inherited = all.where((e) => e.fromImage).length;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Expanded(
              child: SearchField(
                controller: search,
                hint: 'Поиск по имени переменной',
                onChanged: (_) => setState(() {}),
              ),
            ),
            const SizedBox(width: 10),
            FilterChip(
              label: Text('Скрыть из образа ($inherited)'),
              selected: hideInherited,
              onSelected: (v) => setState(() => hideInherited = v),
            ),
            const SizedBox(width: 8),
            FilterChip(
              avatar: Icon(showAll ? Icons.visibility_off_rounded : Icons.visibility_rounded, size: 16),
              label: Text(showAll ? 'Скрыть значения' : 'Показать все'),
              selected: showAll,
              onSelected: (v) => setState(() {
                showAll = v;
                if (!v) shown.clear();
              }),
            ),
            IconButton(
              tooltip: 'Скопировать как .env',
              onPressed: rows.isEmpty
                  ? null
                  : () => copyText(
                      context,
                      rows.map((e) => '${e.key}=${e.value}').join('\n'),
                      label: '.env (${rows.length})',
                    ),
              icon: const Icon(Icons.content_copy_rounded, size: 20),
            ),
          ],
        ),
        const SizedBox(height: 10),
        Expanded(
          child: Panel(
            child: rows.isEmpty
                ? EmptyState(
                    icon: Icons.tune_rounded,
                    title: all.isEmpty ? 'Переменных окружения нет' : 'Ничего не найдено',
                  )
                : ListView.separated(
                    padding: const EdgeInsets.symmetric(vertical: 4),
                    itemCount: rows.length,
                    separatorBuilder: (_, _) =>
                        Divider(height: 1, indent: 14, endIndent: 14, color: p.border.withValues(alpha: .6)),
                    itemBuilder: (context, i) {
                      final e = rows[i];
                      final visible = showAll || shown.contains(e.key);
                      final secret = looksSecret(e.key);
                      return Padding(
                        padding: const EdgeInsets.fromLTRB(14, 6, 6, 6),
                        child: Row(
                          children: [
                            SizedBox(
                              width: 240,
                              child: Row(
                                children: [
                                  if (secret) ...[
                                    Icon(Icons.lock_rounded, size: 14, color: p.warning),
                                    const SizedBox(width: 6),
                                  ],
                                  Flexible(
                                    child: WslcSelectableText(
                                      e.key,
                                      maxLines: 1,
                                      style: const TextStyle(
                                        fontFamily: monoFont,
                                        fontFamilyFallback: monoFallback,
                                        fontWeight: FontWeight.w600,
                                        fontSize: 13,
                                      ),
                                    ),
                                  ),
                                ],
                              ),
                            ),
                            Expanded(
                              child: visible
                                  ? WslcSelectableText(
                                      e.value.isEmpty ? '(пусто)' : e.value,
                                      maxLines: 3,
                                      minLines: 1,
                                      style: TextStyle(
                                        fontFamily: monoFont,
                                        fontFamilyFallback: monoFallback,
                                        fontSize: 12.5,
                                        color: e.value.isEmpty ? p.faint : p.muted,
                                      ),
                                    )
                                  : Text('••••••••', style: TextStyle(color: p.faint, letterSpacing: 2)),
                            ),
                            if (e.fromImage)
                              Padding(
                                padding: const EdgeInsets.only(right: 4),
                                child: Pill('из образа', color: p.muted),
                              ),
                            IconButton(
                              tooltip: visible ? 'Скрыть' : 'Показать',
                              onPressed: showAll
                                  ? null
                                  : () => setState(() => visible ? shown.remove(e.key) : shown.add(e.key)),
                              icon: Icon(visible ? Icons.visibility_off_rounded : Icons.visibility_rounded, size: 18),
                            ),
                            IconButton(
                              tooltip: 'Скопировать значение',
                              onPressed: () => copyText(context, e.value, label: 'значение ${e.key}'),
                              icon: const Icon(Icons.content_copy_rounded, size: 17),
                            ),
                          ],
                        ),
                      );
                    },
                  ),
          ),
        ),
        const SizedBox(height: 6),
        Row(
          children: [
            Icon(Icons.privacy_tip_outlined, size: 15, color: p.faint),
            const SizedBox(width: 6),
            Expanded(
              child: Text(
                'Значения читаются только при открытии вкладки и не сохраняются. '
                'Переменные с замком похожи на секреты.',
                style: TextStyle(color: p.faint, fontSize: 12),
              ),
            ),
          ],
        ),
      ],
    );
  }
}
