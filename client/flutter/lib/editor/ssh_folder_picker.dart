import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/sftp_browser_service.dart';
import 'package:material_ui/material_ui.dart';
import 'package:path/path.dart' as p;

class SshFolderPicker extends StatefulWidget {
  const SshFolderPicker({
    super.key,
    required this.session,
    required this.browser,
    required this.home,
    required this.host,
  });
  final SftpSessionId session;
  final SftpBrowserService browser;
  final String home, host;
  @override
  State<SshFolderPicker> createState() => _SshFolderPickerState();
}

class _SshFolderPickerState extends State<SshFolderPicker> {
  late final address = TextEditingController(text: widget.home);
  late String current = widget.home;
  List<RemoteFileInfo> folders = [];
  bool busy = false;
  Object? failure;

  @override
  void initState() {
    super.initState();
    navigate(current);
  }

  Future<void> navigate(String input, {bool select = false}) async {
    if (busy) return;
    setState(() {
      busy = true;
      failure = null;
    });
    try {
      final path = await widget.browser.resolveDirectory(widget.session, input, base: current);
      final items = await widget.browser.listDirectory(widget.session, path);
      if (!mounted) return;
      if (select) {
        Navigator.pop(context, path);
        return;
      }
      setState(() {
        current = path;
        address.text = path;
        folders =
            items
                .where(
                  (e) => e.kind == RemoteEntryKind.directory && e.name != '.' && e.name != '..' && e.name != '.git',
                )
                .toList()
              ..sort((a, b) => a.name.toLowerCase().compareTo(b.name.toLowerCase()));
      });
    } catch (e) {
      if (mounted) setState(() => failure = e);
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  @override
  void dispose() {
    address.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return AlertDialog(
      title: Text(widget.host),
      content: SizedBox(
        width: 480,
        height: 320,
        child: Column(
          children: [
            TextField(
              controller: address,
              readOnly: busy,
              decoration: InputDecoration(
                labelText: l.editorProjectFolder,
                suffixIcon: IconButton(
                  icon: const Icon(Icons.arrow_upward),
                  tooltip: p.posix.dirname(current),
                  onPressed: busy || current == '/' ? null : () => navigate(p.posix.dirname(current)),
                ),
              ),
              onSubmitted: busy ? null : (path) => navigate(path),
            ),
            if (busy) const LinearProgressIndicator(minHeight: 2),
            if (failure != null)
              Text(errorMessage(l, failure!), style: TextStyle(color: Theme.of(context).colorScheme.error)),
            Expanded(
              child: ListView.builder(
                itemCount: folders.length,
                itemBuilder: (context, index) {
                  final folder = folders[index];
                  return ListTile(
                    dense: true,
                    leading: const Icon(Icons.folder_outlined, size: 18),
                    title: Text(folder.name),
                    onTap: busy ? null : () => navigate(folder.path),
                  );
                },
              ),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: Text(l.commonCancel)),
        FilledButton(onPressed: busy ? null : () => navigate(address.text, select: true), child: Text(l.commonOpen)),
      ],
    );
  }
}
