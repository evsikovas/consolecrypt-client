import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/features/containers/container_actions.dart';
import 'package:consolecrypt/wslc/features/containers/file_editor.dart';
import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/shared/ports.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:material_ui/material_ui.dart';

String joinPath(String dir, String name) => dir.endsWith('/') ? '$dir$name' : '$dir/$name';

String parentPath(String path) {
  if (path == '/') return '/';
  final trimmed = path.endsWith('/') ? path.substring(0, path.length - 1) : path;
  final i = trimmed.lastIndexOf('/');
  return i <= 0 ? '/' : trimmed.substring(0, i);
}

/// Read-only browser of a running container's filesystem, like Docker Desktop "Files".
class FilesPanel extends StatefulWidget {
  const FilesPanel({super.key, required this.containerId, required this.running, this.initialPath = '/'});
  final String containerId;
  final bool running;
  final String initialPath;
  @override
  State<FilesPanel> createState() => _FilesPanelState();
}

class _FilesPanelState extends State<FilesPanel> {
  late final api = WslcApiScope.of(context);

  late String path = widget.initialPath;
  List<FileEntry>? entries;
  String? error;
  int _request = 0;

  @override
  void initState() {
    super.initState();
    if (widget.running) open(path);
  }

  @override
  void didUpdateWidget(covariant FilesPanel old) {
    super.didUpdateWidget(old);
    if (widget.running && !old.running) open(path);
  }

  Future<void> open(String target) async {
    final request = ++_request;
    setState(() {
      path = target;
      error = null;
    });
    try {
      final result = await api.listContainerFiles(containerId: widget.containerId, path: target);
      if (mounted && request == _request) setState(() => entries = result);
    } catch (e) {
      if (mounted && request == _request) {
        setState(() {
          entries = null;
          error = errorText(e);
        });
      }
    }
  }

  Future<void> download(BuildContext context, String file) async {
    final messenger = ScaffoldMessenger.of(context);
    try {
      final saved = await api.downloadContainerFile(containerId: widget.containerId, path: file);
      if (saved != null) {
        messenger.showSnackBar(SnackBar(content: Text('Сохранено: $saved')));
      }
    } catch (e) {
      messenger.showSnackBar(SnackBar(content: Text(errorText(e))));
    }
  }

  Future<void> upload(BuildContext context) async {
    final messenger = ScaffoldMessenger.of(context);
    try {
      final local = await api.pickFile(title: 'Загрузить файл в контейнер');
      if (local == null || !context.mounted) return;
      final name = local.split(RegExp(r'[\\/]')).last;
      final target = joinPath(path, name);
      if (entries?.any((e) => e.name == name) ?? false) {
        final ok = await confirmDanger(
          context,
          icon: Icons.upload_file_rounded,
          title: 'Заменить файл?',
          message: '$target уже существует и будет перезаписан.',
          action: 'Заменить',
        );
        if (!ok) return;
      }
      await api.uploadContainerFile(containerId: widget.containerId, localPath: local, targetPath: target);
      messenger.showSnackBar(SnackBar(content: Text('Загружено: $target')));
      await open(path);
    } catch (e) {
      messenger.showSnackBar(SnackBar(content: Text(errorText(e))));
    }
  }

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    if (!widget.running) {
      return const Panel(
        child: EmptyState(
          icon: Icons.folder_off_outlined,
          title: 'Файлы доступны у запущенного контейнера',
          hint: 'Запустите контейнер, чтобы просмотреть его файловую систему',
        ),
      );
    }
    final parts = path.split('/').where((s) => s.isNotEmpty).toList();
    final list = entries;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            IconButton(
              tooltip: 'На уровень выше',
              onPressed: path == '/' ? null : () => open(parentPath(path)),
              icon: const Icon(Icons.arrow_upward_rounded),
            ),
            Expanded(
              child: Container(
                padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
                decoration: BoxDecoration(
                  color: p.surface,
                  borderRadius: BorderRadius.circular(10),
                  border: Border.all(color: p.border),
                ),
                // Long paths keep their tail in view; short ones stay left-aligned.
                child: LayoutBuilder(
                  builder: (context, box) => SingleChildScrollView(
                    scrollDirection: Axis.horizontal,
                    reverse: true,
                    child: ConstrainedBox(
                      constraints: BoxConstraints(minWidth: box.maxWidth),
                      child: Row(
                        children: [
                          _Crumb(label: '/', onTap: () => open('/')),
                          for (var i = 0; i < parts.length; i++) ...[
                            if (i > 0) Text(' / ', style: TextStyle(color: p.faint)),
                            _Crumb(label: parts[i], onTap: () => open('/${parts.take(i + 1).join('/')}')),
                          ],
                        ],
                      ),
                    ),
                  ),
                ),
              ),
            ),
            const SizedBox(width: 6),
            IconButton(tooltip: 'Обновить', onPressed: () => open(path), icon: const Icon(Icons.refresh_rounded)),
            IconButton(
              tooltip: 'Копировать путь',
              onPressed: () => copyText(context, path),
              icon: const Icon(Icons.content_copy_rounded, size: 20),
            ),
            IconButton(
              tooltip: 'Загрузить файл с компьютера сюда',
              onPressed: () => upload(context),
              icon: const Icon(Icons.upload_file_rounded),
            ),
            IconButton(
              tooltip: 'Открыть терминал в этом каталоге',
              onPressed: () => openContainerTerminal(context, widget.containerId, workdir: path),
              icon: const Icon(Icons.terminal_rounded),
            ),
          ],
        ),
        const SizedBox(height: 10),
        Expanded(
          child: Panel(
            child: list == null
                ? EmptyState(
                    icon: Icons.error_outline_rounded,
                    loading: error == null,
                    title: error == null ? 'Чтение каталога…' : 'Не удалось прочитать каталог',
                    hint: error,
                  )
                : list.isEmpty
                ? const EmptyState(icon: Icons.folder_open_rounded, title: 'Каталог пуст')
                : ListView.builder(
                    padding: const EdgeInsets.symmetric(vertical: 4),
                    itemCount: list.length,
                    itemBuilder: (context, i) {
                      final e = list[i];
                      final full = joinPath(path, e.name);
                      return Material(
                        color: Colors.transparent,
                        child: InkWell(
                          hoverColor: p.raised.withValues(alpha: .7),
                          onTap: e.isDir ? () => open(full) : () => showFileEditor(context, widget.containerId, full),
                          child: Padding(
                            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 7),
                            child: Row(
                              children: [
                                Icon(
                                  e.isDir ? Icons.folder_rounded : Icons.description_outlined,
                                  size: 19,
                                  color: e.isDir ? p.warning : p.muted,
                                ),
                                const SizedBox(width: 12),
                                Expanded(
                                  child: Text(
                                    e.name,
                                    maxLines: 1,
                                    overflow: TextOverflow.ellipsis,
                                    style: TextStyle(
                                      fontFamily: monoFont,
                                      fontFamilyFallback: monoFallback,
                                      fontSize: 13,
                                      color: e.isDir ? p.text : p.muted,
                                    ),
                                  ),
                                ),
                                if (e.isLink) Pill('ссылка', color: p.accentEnd),
                                if (!e.isDir)
                                  IconButton(
                                    tooltip: 'Скачать на компьютер',
                                    visualDensity: VisualDensity.compact,
                                    onPressed: () => download(context, full),
                                    icon: const Icon(Icons.download_rounded, size: 17),
                                  ),
                                IconButton(
                                  tooltip: 'Копировать путь',
                                  visualDensity: VisualDensity.compact,
                                  onPressed: () => copyText(context, full),
                                  icon: const Icon(Icons.content_copy_rounded, size: 16),
                                ),
                              ],
                            ),
                          ),
                        ),
                      );
                    },
                  ),
          ),
        ),
      ],
    );
  }
}

class _Crumb extends StatelessWidget {
  const _Crumb({required this.label, required this.onTap});
  final String label;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) => InkWell(
    borderRadius: BorderRadius.circular(4),
    onTap: onTap,
    child: Padding(
      padding: const EdgeInsets.symmetric(horizontal: 3, vertical: 2),
      child: Text(
        label,
        style: const TextStyle(
          fontFamily: monoFont,
          fontFamilyFallback: monoFallback,
          fontSize: 13,
          fontWeight: FontWeight.w600,
        ),
      ),
    ),
  );
}
