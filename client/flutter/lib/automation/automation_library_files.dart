import 'package:consolecrypt/automation/automation_file_tree.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:material_ui/material_ui.dart';

enum AutomationLibraryFilesView { files, playbooks, roles }

/// Only current local snapshot paths are classified. Opening never runs code,
/// reads credentials, downloads dependencies or creates a project.
class AutomationLibraryFiles extends StatefulWidget {
  const AutomationLibraryFiles({
    super.key,
    required this.scope,
    required this.projectId,
    required this.digest,
    required this.entries,
    required this.onRead,
    this.selectedPath,
    this.enabled = true,
    this.onMove,
    this.onContextMenu,
    this.onCreatePlaybook,
    this.onCreateRole,
    this.onAddDependency,
  });
  final Object scope;
  final String projectId, digest;
  final List<AutomationFileEntry> entries;
  final String? selectedPath;
  final bool enabled;
  final ValueChanged<String> onRead;
  final void Function(AutomationFileDrag, String)? onMove;
  final void Function(AutomationFileDrag, Offset)? onContextMenu;
  final VoidCallback? onCreatePlaybook, onCreateRole, onAddDependency;

  @override
  State<AutomationLibraryFiles> createState() => _AutomationLibraryFilesState();
}

class _AutomationLibraryFilesState extends State<AutomationLibraryFiles> {
  AutomationLibraryFilesView view = AutomationLibraryFilesView.files;
  @override
  void didUpdateWidget(AutomationLibraryFiles oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.projectId != widget.projectId || !identical(oldWidget.scope, widget.scope)) {
      view = AutomationLibraryFilesView.files;
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final yaml = RegExp(r'\.ya?ml$', caseSensitive: false);
    final roles =
        widget.entries
            .where(
              (entry) =>
                  !entry.directory &&
                  RegExp(r'^roles/[^/]+/tasks/main\.ya?ml$', caseSensitive: false).hasMatch(entry.path),
            )
            .toList()
          ..sort((a, b) => a.path.compareTo(b.path));
    final playbooks =
        widget.entries
            .where(
              (entry) =>
                  !entry.directory &&
                  yaml.hasMatch(entry.path) &&
                  !const {
                    'roles',
                    'collections',
                    'templates',
                    'files',
                    'host_vars',
                    'group_vars',
                    '.vault',
                  }.contains(entry.path.split('/').first) &&
                  !const {
                    'requirements.yml',
                    'requirements.yaml',
                    'hosts.yml',
                    'hosts.yaml',
                    'inventory.yml',
                    'inventory.yaml',
                  }.contains(entry.path),
            )
            .toList()
          ..sort((a, b) => a.path.compareTo(b.path));
    final paths = view == AutomationLibraryFilesView.roles ? roles : playbooks;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(8, 4, 8, 4),
          child: GlassSelect<AutomationLibraryFilesView>(
            key: const ValueKey('automation-library-file-kind'),
            value: view,
            expand: true,
            semanticLabel: l.automationLibrary,
            items: [
              GlassSelectItem(value: AutomationLibraryFilesView.files, label: l.automationLibraryFiles),
              GlassSelectItem(value: AutomationLibraryFilesView.playbooks, label: l.automationLibraryPlaybooks),
              GlassSelectItem(value: AutomationLibraryFilesView.roles, label: l.automationLibraryRoles),
            ],
            onChanged: (value) => setState(() => view = value),
          ),
        ),
        if (view != AutomationLibraryFilesView.files)
          Padding(
            padding: const EdgeInsets.fromLTRB(8, 0, 8, 4),
            child: Wrap(
              spacing: 6,
              runSpacing: 4,
              children: [
                if (view == AutomationLibraryFilesView.playbooks && widget.onCreatePlaybook != null)
                  GlassButton(
                    key: const ValueKey('automation-library-new-playbook'),
                    label: l.automationNewPlaybook,
                    icon: Icons.add,
                    onPressed: widget.enabled ? widget.onCreatePlaybook : null,
                  ),
                if (view == AutomationLibraryFilesView.roles && widget.onCreateRole != null)
                  GlassButton(
                    key: const ValueKey('automation-library-new-role'),
                    label: l.automationCreateRole,
                    icon: Icons.add,
                    onPressed: widget.enabled ? widget.onCreateRole : null,
                  ),
                if (view == AutomationLibraryFilesView.roles && widget.onAddDependency != null)
                  GlassButton(
                    key: const ValueKey('automation-library-add-dependency'),
                    label: l.automationDependencyTitle,
                    icon: Icons.extension_outlined,
                    onPressed: widget.enabled ? widget.onAddDependency : null,
                  ),
              ],
            ),
          ),
        if (view == AutomationLibraryFilesView.files)
          AutomationFileTree(
            key: ValueKey('automation-files-${widget.projectId}'),
            scope: widget.scope,
            projectId: widget.projectId,
            digest: widget.digest,
            entries: widget.entries,
            selectedPath: widget.selectedPath,
            enabled: widget.enabled,
            rootLabel: l.automationTreeRoot,
            emptyLabel: l.automationTreeEmpty,
            onRead: widget.onRead,
            onMove: widget.onMove,
            onContextMenu: widget.onContextMenu,
          )
        else ...[
          if (view == AutomationLibraryFilesView.roles)
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
              child: Text(l.automationLibraryRoleHint, style: Theme.of(context).textTheme.bodySmall),
            ),
          if (paths.isEmpty)
            Padding(
              padding: const EdgeInsets.all(12),
              child: Text(
                view == AutomationLibraryFilesView.roles ? l.automationLibraryNoRoles : l.automationLibraryNoPlaybooks,
              ),
            ),
          for (final entry in paths)
            GestureDetector(
              onSecondaryTapDown: !widget.enabled || widget.onContextMenu == null
                  ? null
                  : (details) => widget.onContextMenu!(
                      AutomationFileDrag(
                        scope: widget.scope,
                        projectId: widget.projectId,
                        digest: widget.digest,
                        path: entry.path,
                      ),
                      details.globalPosition,
                    ),
              child: Tooltip(
                message: entry.path,
                child: ListTile(
                  key: ValueKey('automation-library-item-${entry.path}'),
                  dense: true,
                  minTileHeight: 36,
                  selected: entry.path == widget.selectedPath,
                  leading: Icon(
                    view == AutomationLibraryFilesView.roles ? Icons.extension_outlined : Icons.description_outlined,
                    size: 18,
                  ),
                  title: Text(
                    view == AutomationLibraryFilesView.roles ? entry.path.split('/')[1] : entry.path,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                  onTap: widget.enabled ? () => widget.onRead(entry.path) : null,
                ),
              ),
            ),
        ],
      ],
    );
  }
}
