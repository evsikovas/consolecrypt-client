import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:material_ui/material_ui.dart';

/// Resource cards use all available mobile space; on desktop they are regular
/// glass dialogs in the app palette (secrets and mutations keep their own
/// secure dialogs). The scope boundary and explicit actions are unchanged.
Future<T?> showKubeResourceDetails<T>(BuildContext context, {required WidgetBuilder builder}) {
  if (!AppPlatform.isMobile || MediaQuery.sizeOf(context).width >= 720) {
    return showAppDialog<T>(context, builder: builder);
  }
  final themes = InheritedTheme.capture(from: context, to: Navigator.of(context, rootNavigator: true).context);
  final glass = GlassScope.of(context);
  return Navigator.of(context, rootNavigator: true).push<T>(
    MaterialPageRoute<T>(
      fullscreenDialog: true,
      builder: (context) => themes.wrap(
        GlassScope(
          data: glass,
          child: Scaffold(
            body: SafeArea(
              child: GlassDialogScope(
                variant: GlassVariant.secure,
                animation: const AlwaysStoppedAnimation(1),
                opaque: true,
                sheet: false,
                child: SizedBox.expand(child: Builder(builder: builder)),
              ),
            ),
          ),
        ),
      ),
    ),
  );
}
