import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:material_ui/material_ui.dart';

/// Letters outside basic Latin (A–Z) found in typed text — the usual cause
/// of a "wrong passphrase" is a different keyboard layout (e.g. Russian
/// instead of English). Detected from the characters themselves; no native
/// input-source API is needed.
enum TypedScript {
  /// Cyrillic letters (U+0400–U+052F).
  cyrillic,

  /// Other non-ASCII letters (accented Latin, Greek, Arabic, CJK, …).
  otherNonLatin,
}

/// The first non-A–Z script among the letters of [text] (Cyrillic wins), or
/// `null` when every letter is ASCII. Punctuation, symbols and emoji do not
/// count. The text itself is never stored or logged.
TypedScript? nonLatinScript(String text) {
  var other = false;
  for (final rune in text.runes) {
    if (rune < 0x80) continue;
    if (rune >= 0x0400 && rune <= 0x052F) return TypedScript.cyrillic;
    if (_isLetterLike(rune)) other = true;
  }
  return other ? TypedScript.otherNonLatin : null;
}

bool _isLetterLike(int rune) {
  if (rune <= 0xBF || rune == 0xD7 || rune == 0xF7) return false; // Latin-1 punctuation / symbols
  if (rune >= 0x2000 && rune <= 0x2BFF) return false; // punctuation, arrows, maths, symbols
  if (rune >= 0x3000 && rune <= 0x303F) return false; // CJK punctuation
  if (rune >= 0xFE00 && rune <= 0xFE0F) return false; // variation selectors
  if (rune >= 0x1F000) return false; // emoji and pictographs
  return true;
}

/// Localized hint for [script] (secret fields).
String typedScriptHint(AppLocalizations l10n, TypedScript script) => switch (script) {
  TypedScript.cyrillic => l10n.secretFieldCyrillicHint,
  TypedScript.otherNonLatin => l10n.secretFieldNonLatinHint,
};

/// A small warning line (icon + text) under a field: Caps Lock, keyboard
/// layout. Colour is never the only cue.
class FieldNote extends StatelessWidget {
  const FieldNote({required this.text, super.key, this.icon = Icons.warning_rounded});

  final String text;
  final IconData icon;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return Padding(
      padding: const EdgeInsets.only(top: GlassSpacing.s2),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.only(top: 1),
            child: Icon(icon, size: 14, color: tokens.palette.warning),
          ),
          const SizedBox(width: GlassSpacing.s4),
          Flexible(
            child: Text(text, style: tokens.typography.callout.copyWith(color: tokens.palette.warning)),
          ),
        ],
      ),
    );
  }
}

/// Notice under a NEW passphrase that contains letters outside A–Z:
/// unlocking will need the same keyboard layout (the user may continue).
class PassphraseLayoutNotice extends StatelessWidget {
  const PassphraseLayoutNotice({required this.passphrase, super.key});

  final String passphrase;

  @override
  Widget build(BuildContext context) {
    if (nonLatinScript(passphrase) == null) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.only(top: GlassSpacing.s8),
      child: InfoBanner(
        key: const ValueKey('passphrase-layout-notice'),
        tone: BannerTone.warning,
        icon: Icons.keyboard_rounded,
        message: context.l10n.passphraseLayoutNotice,
      ),
    );
  }
}
