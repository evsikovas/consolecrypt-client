#ifndef RUNNER_FONT_FAMILIES_H_
#define RUNNER_FONT_FAMILIES_H_

#include <string>
#include <vector>

// Installed font families as Flutter sees them on Windows: Skia resolves
// `TextStyle.fontFamily` with IDWriteFontCollection::FindFamilyName on the
// DirectWrite system font collection, so the names come from that same
// collection (which also holds the fonts installed for the current user).
//
// Both functions return UTF-8 names, deduplicated case-insensitively and
// sorted; any DirectWrite failure yields an empty list (Dart then falls back).

// Fixed-pitch families (IDWriteFont1::IsMonospacedFont), one name per family:
// the en-us family name, else the first one.
std::vector<std::string> MonospaceFontFamilies();

// Every family name FindFamilyName matches (all localized names of every
// family), used to tell whether a typed family is installed at all.
std::vector<std::string> InstalledFontFamilies();

#endif  // RUNNER_FONT_FAMILIES_H_
