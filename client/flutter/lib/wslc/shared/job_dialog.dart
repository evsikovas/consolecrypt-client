import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:material_ui/material_ui.dart';

/// Runs a long CLI job (pull, build, push, save…) and shows its live output.
/// Returns true when the job finished successfully.
Future<bool> showJobDialog(
  BuildContext context, {
  required String title,
  required IconData icon,
  required Future<BigInt> Function() start,
  String? successText,
}) async {
  final ok = await showWslcDialog<bool>(
    context: context,
    barrierDismissible: false,
    builder: (_) =>
        _JobDialog(title: title, icon: icon, start: start, successText: successText ?? context.l10n.commonDone),
  );
  return ok == true;
}

class _Line {
  _Line(this.text, this.progress);
  String text;
  bool progress;
}

class _JobDialog extends StatefulWidget {
  const _JobDialog({required this.title, required this.icon, required this.start, required this.successText});
  final String title;
  final IconData icon;
  final Future<BigInt> Function() start;
  final String successText;
  @override
  State<_JobDialog> createState() => _JobDialogState();
}

class _JobDialogState extends State<_JobDialog> {
  late final api = WslcApiScope.of(context);

  final lines = <_Line>[];
  final scroll = ScrollController();
  BigInt? job;
  Timer? poll;
  bool polling = false;
  bool finished = false;
  bool success = false;
  int? exitCode;
  String? error;

  @override
  void initState() {
    super.initState();
    unawaited(begin());
  }

  Future<void> begin() async {
    try {
      final id = await widget.start();
      if (!mounted) {
        await api.cancelJob(job: id);
        return;
      }
      job = id;
      poll = Timer.periodic(const Duration(milliseconds: 150), (_) => unawaited(pump()));
    } catch (e) {
      if (mounted) {
        setState(() {
          finished = true;
          error = errorText(e);
        });
      }
    }
  }

  Future<void> pump() async {
    final id = job;
    if (polling || id == null || finished) return;
    polling = true;
    try {
      final batch = await api.takeJob(job: id);
      if (!mounted) return;
      if (batch.lines.isEmpty && !batch.finished) return;
      setState(() {
        for (final l in batch.lines) {
          // Progress updates overwrite the previous progress line in place.
          if (lines.isNotEmpty && lines.last.progress) {
            lines.last
              ..text = l.text
              ..progress = l.progress;
          } else {
            lines.add(_Line(l.text, l.progress));
          }
        }
        if (lines.length > 3000) lines.removeRange(0, lines.length - 3000);
        if (batch.finished) {
          finished = true;
          success = batch.success;
          exitCode = batch.exitCode;
          poll?.cancel();
        }
      });
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (scroll.hasClients) scroll.jumpTo(scroll.position.maxScrollExtent);
      });
    } catch (e) {
      if (mounted) {
        setState(() {
          finished = true;
          error = errorText(e);
        });
      }
    } finally {
      polling = false;
    }
  }

  Future<void> close() async {
    poll?.cancel();
    final id = job;
    if (id != null) {
      if (!finished) await api.cancelJob(job: id);
      await api.forgetJob(job: id);
    }
    if (mounted) Navigator.pop(context, success);
  }

  @override
  void dispose() {
    poll?.cancel();
    scroll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final (color, status) = !finished
        ? (p.accentEnd, context.l10n.wslcRunning)
        : success
        ? (p.success, widget.successText)
        : (
            p.danger,
            error ??
                (exitCode == null ? context.l10n.sftpTransferCancelled : context.l10n.wslcErrorExitCode(exitCode!)),
          );
    return Dialog(
      insetPadding: const EdgeInsets.all(40),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 860, maxHeight: 620),
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(
                children: [
                  GradientIcon(widget.icon, size: 34),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Text(
                      widget.title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(fontSize: 17, fontWeight: FontWeight.w700),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 14),
              Expanded(
                child: DecoratedBox(
                  decoration: BoxDecoration(
                    color: p.terminal,
                    borderRadius: BorderRadius.circular(12),
                    border: Border.all(color: p.border),
                  ),
                  child: lines.isEmpty
                      ? Center(
                          child: Text(
                            finished ? '' : context.l10n.wslcWaitingForOutput,
                            style: TextStyle(color: p.faint),
                          ),
                        )
                      : SelectionArea(
                          child: ListView.builder(
                            controller: scroll,
                            padding: const EdgeInsets.all(12),
                            itemCount: lines.length,
                            itemBuilder: (context, i) => Text(
                              lines[i].text,
                              style: TextStyle(
                                fontFamily: monoFont,
                                fontFamilyFallback: monoFallback,
                                fontSize: 12.5,
                                height: 1.4,
                                color: p.terminalText,
                              ),
                            ),
                          ),
                        ),
                ),
              ),
              const SizedBox(height: 14),
              Row(
                children: [
                  if (!finished)
                    const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2))
                  else
                    StatusDot(color: color, glow: true),
                  const SizedBox(width: 10),
                  Expanded(
                    child: Text(
                      status,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(color: color, fontWeight: FontWeight.w600),
                    ),
                  ),
                  if (!finished)
                    TextButton(onPressed: close, child: Text(context.l10n.commonCancel))
                  else
                    FilledButton(onPressed: close, child: Text(context.l10n.commonClose)),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}
