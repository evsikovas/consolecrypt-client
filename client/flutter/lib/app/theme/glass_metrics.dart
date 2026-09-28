import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter/painting.dart';

/// Corner radii (LIQUID_GLASS_SPEC §2.5). macOS uses the rounder scale;
/// Windows 11 has 8 px window corners and a flatter scale.
@immutable
final class GlassRadii {
  const GlassRadii({
    required this.xs,
    required this.sm,
    required this.md,
    required this.row,
    required this.menu,
    required this.card,
    required this.panel,
    required this.palette,
    required this.dialog,
    this.shellInset = 8,
  });

  final double xs;

  /// Small controls (22 high).
  final double sm;

  /// Medium controls, fields (28 high), code groups.
  final double md;

  /// Sidebar items and list rows (concentric: panel − 8).
  final double row;

  /// Menus, popovers, tooltips.
  final double menu;

  /// Content cards, terminal card, palette rows.
  final double card;

  /// Floating sidebar, SFTP panes (`shellPanel`).
  final double panel;

  /// Command palette.
  final double palette;

  /// Dialogs and sheets.
  final double dialog;

  /// Distance of the floating sidebar / content column from the window edge.
  /// 0 gives the macOS 27 edge-to-edge sidebar.
  final double shellInset;

  static const macOS = GlassRadii(xs: 4, sm: 6, md: 8, row: 10, menu: 12, card: 14, panel: 18, palette: 22, dialog: 26);

  static const windows = GlassRadii(xs: 4, sm: 4, md: 6, row: 6, menu: 8, card: 8, panel: 12, palette: 16, dialog: 20);

  static GlassRadii forPlatform(TargetPlatform platform) => platform == TargetPlatform.macOS ? macOS : windows;

  /// Minimum radius of a concentric shape (4 for elements shorter than 20).
  static double minRadiusFor(double? height) => height != null && height < 20 ? 4 : 6;

  /// Concentric corner radius: `max(outer − inset, rMin)` (§1.5, §2.5).
  static double concentric(double outer, double inset, {double? height}) =>
      math.max(outer - inset, minRadiusFor(height));

  /// Whether a shape of [height] with [radius] is drawn as a capsule.
  static bool isCapsule(double height, double radius) => height <= 2 * radius;

  /// Continuous-corner shape used everywhere (`RoundedSuperellipseBorder`).
  static OutlinedBorder shape(double radius) =>
      RoundedSuperellipseBorder(borderRadius: BorderRadius.all(Radius.circular(radius)));

  static GlassRadii lerp(GlassRadii a, GlassRadii b, double t) => t < 0.5 ? a : b;
}

/// 4-pt spacing scale and layout paddings (§2.6).
abstract final class GlassSpacing {
  static const double s2 = 2;
  static const double s4 = 4;
  static const double s6 = 6;
  static const double s8 = 8;
  static const double s12 = 12;
  static const double s16 = 16;
  static const double s20 = 20;
  static const double s24 = 24;
  static const double s32 = 32;
  static const double s40 = 40;
  static const double s48 = 48;

  static const scale = [s2, s4, s6, s8, s12, s16, s20, s24, s32, s40, s48];

  static const double page = 24;
  static const double pageCompact = 16;
  static const double section = 24;
  static const double card = 16;
  static const double dialog = 24;
  static const double secureDialog = 28;

  /// Gap between items inside a toolbar group / between groups.
  static const double toolbarItemGap = 6;
  static const double toolbarGroupGap = 12;
}

/// Control sizes of the kit.
enum GlassControlSize {
  sm(22),
  md(28),
  lg(36),
  xl(44);

  const GlassControlSize(this.height);

  final double height;

  /// lg and xl are capsules; sm/md are rounded rectangles (§1.5).
  bool get isCapsule => this == lg || this == xl;
}

/// Fixed component heights and icon sizes (§2.6, §4).
abstract final class GlassSizes {
  static const double toolbarBand = 52;
  static const double toolbarButton = 32;
  static const double toolbarGroup = 32;
  static const double sidebarRow = 32;
  static const double sidebarWidth = 240;
  static const double sidebarCompactWidth = 64;
  static const double listRow = 36;
  static const double listRowDense = 30;
  static const double tab = 28;
  static const double tabTrack = 34;
  static const double tabMinWidth = 120;
  static const double tabMaxWidth = 220;
  static const double toast = 44;
  static const double toastMinWidth = 320;
  static const double toastMaxWidth = 520;
  static const double badge = 20;
  static const double badgeDense = 16;
  static const double statusPill = 24;
  static const double menuItem = 26;
  static const double menuMinWidth = 200;
  static const double segmented = 28;

  /// WCAG 2.2 2.5.8 minimum target size.
  static const double minHitTarget = 24;

  /// macOS traffic-light reservation at the top of the floating sidebar and
  /// the toolbar's leading inset in compact mode.
  static const double trafficLightsTop = 52;
  static const double trafficLightsLeading = 78;

  static const double iconRow = 18;
  static const double iconToolbar = 20;
  static const double iconMenu = 16;
  static const double iconBadge = 14;
  static const double iconBadgeDense = 12;

  /// Window width below which the shell goes compact.
  static const double compactBreakpoint = 1000;
}
