import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:material_ui/material_ui.dart';

/// Text viewer with an explicit edit mode; saving writes the file back through `wslc exec`.
Future<void> showFileEditor(BuildContext context, String containerId, String path) => showWslcDialog<void>(
  context: context,
  barrierDismissible: false,
  builder: (_) => _FileEditor(containerId: containerId, path: path),
);

class _FileEditor extends StatefulWidget {
  const _FileEditor({required this.containerId, required this.path});
  final String containerId;
  final String path;
  @override
  State<_FileEditor> createState() => _FileEditorState();
}

class _FileEditorState extends State<_FileEditor> {
  late final api = WslcApiScope.of(context);

  final text = TextEditingController();
  String? original;
  String? error;
  bool editing = false;
  bool saving = false;

  bool get dirty => original != null && text.text != original;

  @override
  void initState() {
    super.initState();
    load();
  }

  Future<void> load() async {
    try {
      final content = await api.readContainerText(containerId: widget.containerId, path: widget.path);
      if (!mounted) return;
      setState(() {
        original = content;
        text.text = content;
      });
    } catch (e) {
      if (mounted) setState(() => error = errorText(e));
    }
  }

  Future<void> save() async {
    setState(() => saving = true);
    final messenger = ScaffoldMessenger.of(context);
    try {
      await api.writeContainerText(containerId: widget.containerId, path: widget.path, content: text.text);
      if (!mounted) return;
      setState(() {
        original = text.text;
        editing = false;
        saving = false;
      });
      messenger.showSnackBar(SnackBar(content: Text('Сохранено: ${widget.path}')));
    } catch (e) {
      if (mounted) setState(() => saving = false);
      messenger.showSnackBar(SnackBar(content: Text(errorText(e))));
    }
  }

  Future<void> close() async {
    if (dirty) {
      final discard = await showWslcDialog<bool>(
        context: context,
        builder: (context) => AlertDialog(
          title: const Text('Закрыть без сохранения?'),
          content: const Text('Изменения в файле будут потеряны.'),
          actions: [
            TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('Вернуться')),
            FilledButton(onPressed: () => Navigator.pop(context, true), child: const Text('Закрыть')),
          ],
        ),
      );
      if (discard != true) return;
    }
    if (mounted) Navigator.pop(context);
  }

  @override
  void dispose() {
    text.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final lines = original == null ? 0 : '\n'.allMatches(text.text).length + 1;
    return Dialog(
      insetPadding: const EdgeInsets.all(32),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 980, maxHeight: 780),
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(
                children: [
                  const GradientIcon(Icons.description_outlined, size: 34),
                  const SizedBox(width: 12),
                  Expanded(
                    child: WslcSelectableText(
                      widget.path,
                      maxLines: 1,
                      style: const TextStyle(
                        fontFamily: monoFont,
                        fontFamilyFallback: monoFallback,
                        fontSize: 15,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                  ),
                  if (original != null && !editing)
                    OutlinedButton.icon(
                      onPressed: () => setState(() => editing = true),
                      icon: const Icon(Icons.edit_rounded, size: 18),
                      label: const Text('Редактировать'),
                    ),
                  if (editing) ...[
                    TextButton(
                      onPressed: saving
                          ? null
                          : () => setState(() {
                              text.text = original!;
                              editing = false;
                            }),
                      child: const Text('Отменить правки'),
                    ),
                    const SizedBox(width: 6),
                    FilledButton.icon(
                      onPressed: saving || !dirty ? null : save,
                      icon: saving
                          ? const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2))
                          : const Icon(Icons.save_rounded, size: 18),
                      label: const Text('Сохранить'),
                    ),
                  ],
                  const SizedBox(width: 6),
                  IconButton(
                    tooltip: 'Закрыть',
                    onPressed: saving ? null : close,
                    icon: const Icon(Icons.close_rounded),
                  ),
                ],
              ),
              const SizedBox(height: 14),
              Expanded(
                child: original == null
                    ? Panel(
                        child: EmptyState(
                          icon: Icons.error_outline_rounded,
                          loading: error == null,
                          title: error == null ? 'Чтение файла…' : 'Файл нельзя открыть как текст',
                          hint: error,
                        ),
                      )
                    : DecoratedBox(
                        decoration: BoxDecoration(
                          color: p.terminal,
                          borderRadius: BorderRadius.circular(12),
                          border: Border.all(color: editing ? p.accent : p.border),
                        ),
                        child: TextField(
                          controller: text,
                          readOnly: !editing,
                          expands: true,
                          maxLines: null,
                          onChanged: (_) => setState(() {}),
                          style: TextStyle(
                            fontFamily: monoFont,
                            fontFamilyFallback: monoFallback,
                            fontSize: 12.5,
                            height: 1.45,
                            color: p.terminalText,
                          ),
                          decoration: const InputDecoration(
                            filled: false,
                            border: InputBorder.none,
                            enabledBorder: InputBorder.none,
                            focusedBorder: InputBorder.none,
                            contentPadding: EdgeInsets.all(14),
                          ),
                        ),
                      ),
              ),
              const SizedBox(height: 8),
              Text(
                original == null
                    ? 'Текстовые файлы UTF-8 до 2 МБ'
                    : '${plural(lines, 'строка', 'строки', 'строк')}'
                          '${editing ? ' · режим правки' : ' · только просмотр'}'
                          '${dirty ? ' · есть несохранённые изменения' : ''}',
                style: TextStyle(color: dirty ? p.warning : p.faint, fontSize: 12),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
