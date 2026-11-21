import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_exposure.dart';
import 'package:consolecrypt/core/widgets/secret_selectable_text.dart';
import 'package:consolecrypt/wslc/src/rust/api/desk.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

/// Dialogs retain their original API lease even if the app switches profiles.
class WslcApiScope extends InheritedWidget {
  const WslcApiScope({required this.api, required this.copySecret, required super.child, super.key});
  final WslcApi api;
  final Future<void> Function(String) copySecret;
  static WslcApiScope _scope(BuildContext context) {
    if (!context.mounted) throw StateError('WSLc workspace was closed');
    final scope = context.getInheritedWidgetOfExactType<WslcApiScope>();
    if (scope == null) throw StateError('WSLc workspace is unavailable');
    return scope;
  }

  static WslcApi of(BuildContext context) => _scope(context).api;
  static Future<void> copy(BuildContext context, ClipboardData data) => _scope(context).copySecret(data.text ?? '');
  @override
  bool updateShouldNotify(WslcApiScope oldWidget) => oldWidget.api != api;
}

/// Includes keyboard/context-menu copy, not just the explicit copy buttons.
class WslcSelectableText extends StatelessWidget {
  const WslcSelectableText(this.text, {this.style, this.minLines, this.maxLines, super.key});
  final String text;
  final TextStyle? style;
  final int? minLines, maxLines;
  @override
  Widget build(BuildContext context) => SecretSelectableText(
    text,
    style: style,
    minLines: minLines,
    maxLines: maxLines,
    onCopy: (selected) => WslcApiScope.copy(context, ClipboardData(text: selected)),
  );
}

Future<T?> showWslcDialog<T>({
  required BuildContext context,
  required WidgetBuilder builder,
  bool barrierDismissible = true,
  bool containsSecrets = true,
}) {
  final scope = WslcApiScope._scope(context);
  return showAppDialog<T>(
    context,
    secure: true,
    barrierDismissible: barrierDismissible,
    builder: (context) => WslcApiScope(
      api: scope.api,
      copySecret: scope.copySecret,
      child: containsSecrets ? RevealedSecret(child: Builder(builder: builder)) : Builder(builder: builder),
    ),
  );
}
