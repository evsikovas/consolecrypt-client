import 'dart:math' as math;

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:material_ui/material_ui.dart';

class DockerPortPages<T> extends StatefulWidget {
  const DockerPortPages({required this.ports, required this.buildPage, this.enabled = true, super.key});
  final List<T> ports;
  final Widget Function(List<T>) buildPage;
  final bool enabled;

  @override
  State<DockerPortPages<T>> createState() => _DockerPortPagesState<T>();
}

class _DockerPortPagesState<T> extends State<DockerPortPages<T>> {
  static const _pageSize = 20;
  int _page = 0;
  int get _lastPage => widget.ports.isEmpty ? 0 : (widget.ports.length - 1) ~/ _pageSize;

  @override
  void didUpdateWidget(covariant DockerPortPages<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    _page = math.min(_page, _lastPage);
  }

  @override
  Widget build(BuildContext context) {
    final start = _page * _pageSize;
    final end = math.min(start + _pageSize, widget.ports.length);
    final l = context.l10n;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (_lastPage > 0)
          Wrap(
            spacing: GlassSpacing.s6,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              GlassIconButton(
                key: const ValueKey('docker-ports-previous'),
                icon: Icons.chevron_left_rounded,
                tooltip: l.commonBack,
                size: 28,
                onPressed: widget.enabled && _page > 0 ? () => setState(() => _page--) : null,
              ),
              Text(l.dockerPortRange(start + 1, end, widget.ports.length)),
              GlassIconButton(
                key: const ValueKey('docker-ports-next'),
                icon: Icons.chevron_right_rounded,
                tooltip: l.commonNext,
                size: 28,
                onPressed: widget.enabled && _page < _lastPage ? () => setState(() => _page++) : null,
              ),
            ],
          ),
        widget.buildPage(widget.ports.sublist(start, end)),
      ],
    );
  }
}
