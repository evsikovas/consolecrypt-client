import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

final class _Version {
  const _Version(this.id, this.name, this.digest, this.createdAt, this.bytes);
  final String id, name, digest;
  final int createdAt, bytes;
  factory _Version.parse(AutomationJson row) {
    final id = row['id'],
        name = row['name'],
        digest = row['digest'],
        created = row['created_at_ms'],
        bytes = row['bytes'];
    if (id is! String ||
        !RegExp(r'^[a-zA-Z0-9_-]{1,128}$').hasMatch(id) ||
        name is! String ||
        name.trim().isEmpty ||
        name.runes.length > 120 ||
        digest is! String ||
        !RegExp(r'^[0-9a-f]{64}$').hasMatch(digest) ||
        created is! int ||
        created < 1 ||
        created > 8640000000000000 ||
        bytes is! int ||
        bytes < 0) {
      throw const FormatException('version');
    }
    return _Version(id, name, digest, created, bytes);
  }
}

/// Native owns immutable snapshot bytes and exact-digest saves. This dialog
/// only lists records and asks Native to open an explicitly chosen copy.
class AutomationVersionsDialog extends StatefulWidget {
  const AutomationVersionsDialog({
    super.key,
    required this.project,
    required this.digest,
    required this.service,
    required this.originalCurrent,
  });
  final AutomationProject project;
  final String digest;
  final AutomationProjectVersionService service;
  final bool Function() originalCurrent;
  @override
  State<AutomationVersionsDialog> createState() => _AutomationVersionsDialogState();
}

class _AutomationVersionsDialogState extends State<AutomationVersionsDialog> {
  final versionName = TextEditingController(), projectName = TextEditingController();
  List<_Version> versions = const [];
  String? selected;
  bool busy = false, failed = false;
  bool get current => mounted && widget.originalCurrent();
  @override
  void initState() {
    super.initState();
    reload();
  }

  @override
  void dispose() {
    versionName.dispose();
    projectName.dispose();
    super.dispose();
  }

  Future<void> reload() async {
    if (!current || busy) return;
    setState(() {
      busy = true;
      failed = false;
    });
    try {
      final raw = await widget.service.projectVersions(widget.project.id);
      if (!current) return;
      if (raw.length > 256) throw const FormatException('versions');
      final rows = raw.map(_Version.parse).toList();
      if (rows.map((r) => r.id).toSet().length != rows.length) throw const FormatException('versions');
      rows.sort((a, b) => b.createdAt.compareTo(a.createdAt));
      setState(() {
        versions = rows;
        if (!rows.any((r) => r.id == selected)) selected = null;
      });
    } catch (_) {
      if (current) setState(() => failed = true);
    } finally {
      if (current) setState(() => busy = false);
    }
  }

  Future<void> save() async {
    if (!current || busy || versionName.text.trim().isEmpty || !RegExp(r'^[0-9a-f]{64}$').hasMatch(widget.digest)) {
      return;
    }
    final name = versionName.text.trim();
    setState(() {
      busy = true;
      failed = false;
    });
    try {
      await widget.service.saveProjectVersion(widget.project.id, digest: widget.digest, name: name);
      if (!current) return;
      versionName.clear();
      setState(() => busy = false);
      await reload();
    } catch (_) {
      if (current) {
        setState(() {
          failed = true;
          busy = false;
        });
      }
    }
  }

  Future<void> open() async {
    final id = selected, name = projectName.text.trim();
    if (!current || busy || id == null || name.isEmpty || !versions.any((r) => r.id == id)) return;
    setState(() {
      busy = true;
      failed = false;
    });
    try {
      final copy = await widget.service.openProjectVersion(widget.project.id, id, name: name);
      if (!mounted || !widget.originalCurrent()) return;
      if (copy.id.isEmpty || copy.id == widget.project.id || copy.readOnly) throw const FormatException('version_copy');
      Navigator.pop(context, copy);
    } catch (_) {
      if (current) {
        setState(() {
          failed = true;
          busy = false;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final date = DateFormat.yMd(Localizations.localeOf(context).toString()).add_Hm();
    return GlassDialog(
      title: l.automationVersionsTitle,
      icon: Icons.history,
      width: 700,
      content: SizedBox(
        height: (MediaQuery.sizeOf(context).height * .62).clamp(200.0, 520.0),
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(l.automationVersionsHint),
              const SizedBox(height: 8),
              TextField(
                key: const ValueKey('automation-version-name'),
                controller: versionName,
                maxLength: 120,
                enabled: !busy,
                decoration: InputDecoration(labelText: l.automationVersionsName),
                onChanged: (_) => setState(() {}),
              ),
              Align(
                alignment: AlignmentDirectional.centerStart,
                child: GlassButton(
                  key: const ValueKey('automation-version-save'),
                  label: l.automationVersionsSave,
                  icon: Icons.save_outlined,
                  onPressed:
                      current &&
                          !busy &&
                          versionName.text.trim().isNotEmpty &&
                          RegExp(r'^[0-9a-f]{64}$').hasMatch(widget.digest)
                      ? save
                      : null,
                ),
              ),
              if (busy) const LinearProgressIndicator(minHeight: 2),
              if (failed)
                Padding(padding: const EdgeInsets.symmetric(vertical: 8), child: Text(l.automationVersionsUnavailable)),
              SizedBox(
                height: (MediaQuery.sizeOf(context).height * .27).clamp(100.0, 220.0),
                child: versions.isEmpty
                    ? Center(child: Text(l.automationVersionsEmpty))
                    : ListView.builder(
                        key: const ValueKey('automation-version-list'),
                        itemCount: versions.length,
                        itemBuilder: (context, index) {
                          final row = versions[index];
                          return Tooltip(
                            message: row.digest,
                            child: ListTile(
                              key: ValueKey('automation-version-${row.id}'),
                              dense: true,
                              selected: selected == row.id,
                              title: Text(row.name, maxLines: 1, overflow: TextOverflow.ellipsis),
                              subtitle: Text(
                                '${date.format(DateTime.fromMillisecondsSinceEpoch(row.createdAt).toLocal())} · ${formatBytes(l, row.bytes)}',
                              ),
                              onTap: busy
                                  ? null
                                  : () => setState(() {
                                      selected = row.id;
                                      projectName.text = '${widget.project.name ?? ''} - ${row.name}'.trim();
                                    }),
                            ),
                          );
                        },
                      ),
              ),
              if (selected != null)
                TextField(
                  key: const ValueKey('automation-version-project-name'),
                  controller: projectName,
                  maxLength: 120,
                  enabled: !busy,
                  decoration: InputDecoration(labelText: l.automationVersionsProjectName),
                  onChanged: (_) => setState(() {}),
                ),
            ],
          ),
        ),
      ),
      leadingAction: GlassButton(
        key: const ValueKey('automation-version-refresh'),
        label: l.commonRetry,
        icon: Icons.refresh,
        onPressed: current && !busy ? reload : null,
      ),
      secondaryActions: [GlassButton(label: l.commonClose, onPressed: () => Navigator.pop(context))],
      primaryAction: GlassButton(
        key: const ValueKey('automation-version-open'),
        label: l.automationVersionsOpen,
        onPressed:
            current &&
                !busy &&
                selected != null &&
                projectName.text.trim().isNotEmpty &&
                projectName.text.trim().runes.length <= 120
            ? open
            : null,
      ),
    );
  }
}
