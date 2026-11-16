import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_url.dart';
import 'package:material_ui/material_ui.dart';

/// The only address input is the user's empty-to-typed field. There is no
/// imported JSON, source endpoint, credential resolver or persistence callback.
class MonitoringGrafanaOpen extends StatefulWidget {
  const MonitoringGrafanaOpen({
    super.key,
    required this.isCurrent,
    required this.launch,
    required this.onOpened,
    required this.onCancel,
  });
  final bool Function() isCurrent;
  final Future<bool> Function(Uri) launch;
  final VoidCallback onOpened, onCancel;
  @override
  State<MonitoringGrafanaOpen> createState() => _MonitoringGrafanaOpenState();
}

class _MonitoringGrafanaOpenState extends State<MonitoringGrafanaOpen> {
  final address = TextEditingController();
  Uri? reviewed;
  bool invalid = false, failed = false, opening = false;
  int generation = 0;
  bool get current => mounted && widget.isCurrent();
  @override
  void initState() {
    super.initState();
    address.addListener(changed);
  }

  void changed() {
    generation++;
    setState(() {
      reviewed = null;
      invalid = false;
      failed = false;
    });
  }

  @override
  void dispose() {
    generation++;
    reviewed = null;
    address.removeListener(changed);
    address.clear();
    address.dispose();
    super.dispose();
  }

  void review() {
    if (!current || opening) return;
    final uri = reviewGrafanaBrowserUri(address.text);
    setState(() {
      reviewed = uri;
      invalid = uri == null;
      failed = false;
    });
  }

  Future<void> open() async {
    final uri = reviewed;
    if (!current || opening || uri == null) return;
    final token = ++generation;
    setState(() {
      opening = true;
      failed = false;
    });
    try {
      final accepted = await widget.launch(uri);
      if (!current || token != generation) return;
      if (accepted) {
        widget.onOpened();
      } else {
        setState(() => failed = true);
      }
    } catch (_) {
      if (current && token == generation) setState(() => failed = true);
    } finally {
      if (current && token == generation) setState(() => opening = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, uri = reviewed;
    return GlassDialog(
      title: l.grafanaOpenTitle,
      icon: Icons.open_in_new_rounded,
      width: 680,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(l.grafanaOpenIntro),
            const SizedBox(height: 12),
            TextField(
              key: const ValueKey('grafana-open-address'),
              controller: address,
              enabled: !opening,
              maxLength: 4096,
              keyboardType: TextInputType.url,
              autocorrect: false,
              decoration: InputDecoration(labelText: l.grafanaOpenAddress),
            ),
            Align(
              alignment: Alignment.centerLeft,
              child: GlassButton(
                key: const ValueKey('grafana-open-review'),
                label: l.grafanaOpenReview,
                onPressed: opening || address.text.isEmpty ? null : review,
              ),
            ),
            if (invalid || failed) ...[
              const SizedBox(height: 12),
              InfoBanner(
                key: const ValueKey('grafana-open-error'),
                message: invalid ? l.grafanaOpenInvalid : l.grafanaOpenFailed,
                tone: BannerTone.danger,
              ),
            ],
            if (uri != null) ...[
              const SizedBox(height: 12),
              Text(l.grafanaOpenReviewed),
              ContentSurface(
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxHeight: 180),
                  child: SingleChildScrollView(
                    child: SelectableText(uri.toString(), key: const ValueKey('grafana-open-exact-uri')),
                  ),
                ),
              ),
              const SizedBox(height: 8),
              Text(l.grafanaOpenBrowserSession),
            ],
            if (opening) const LinearProgressIndicator(),
          ],
        ),
      ),
      primaryAction: GlassButton(
        key: const ValueKey('grafana-open-confirm'),
        label: l.commonOpen,
        onPressed: !opening && uri != null && current ? open : null,
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: widget.onCancel)],
    );
  }
}
