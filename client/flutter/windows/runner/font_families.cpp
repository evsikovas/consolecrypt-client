#include "font_families.h"

#include <windows.h>

#include <dwrite_1.h>
#include <wrl/client.h>

#include <algorithm>
#include <cwctype>
#include <set>

using Microsoft::WRL::ComPtr;

namespace {

std::string ToUtf8(const std::wstring& text) {
  if (text.empty()) {
    return {};
  }
  const int size =
      WideCharToMultiByte(CP_UTF8, 0, text.data(), static_cast<int>(text.size()),
                          nullptr, 0, nullptr, nullptr);
  if (size <= 0) {
    return {};
  }
  std::string utf8(static_cast<size_t>(size), '\0');
  WideCharToMultiByte(CP_UTF8, 0, text.data(), static_cast<int>(text.size()),
                      utf8.data(), size, nullptr, nullptr);
  return utf8;
}

std::wstring NameAt(IDWriteLocalizedStrings* names, UINT32 index) {
  UINT32 length = 0;
  if (FAILED(names->GetStringLength(index, &length))) {
    return {};
  }
  std::wstring name(static_cast<size_t>(length) + 1, L'\0');
  if (FAILED(names->GetString(index, name.data(), length + 1))) {
    return {};
  }
  name.resize(length);
  return name;
}

// The family name DirectWrite matches by: en-us, else the first one.
std::wstring PrimaryName(IDWriteLocalizedStrings* names) {
  if (names->GetCount() == 0) {
    return {};
  }
  UINT32 index = 0;
  BOOL exists = FALSE;
  if (FAILED(names->FindLocaleName(L"en-us", &index, &exists)) || !exists) {
    index = 0;
  }
  return NameAt(names, index);
}

// A family is monospaced when its first non-simulated face is.
bool IsMonospacedFamily(IDWriteFontFamily* family) {
  const UINT32 count = family->GetFontCount();
  for (UINT32 i = 0; i < count; ++i) {
    ComPtr<IDWriteFont> font;
    if (FAILED(family->GetFont(i, &font))) {
      continue;
    }
    if (font->GetSimulations() != DWRITE_FONT_SIMULATIONS_NONE) {
      continue;
    }
    ComPtr<IDWriteFont1> font1;
    if (FAILED(font.As(&font1))) {
      return false;  // Windows 7 without the platform update.
    }
    return font1->IsMonospacedFont() != FALSE;
  }
  return false;
}

struct CaseInsensitiveLess {
  bool operator()(const std::wstring& a, const std::wstring& b) const {
    return std::lexicographical_compare(
        a.begin(), a.end(), b.begin(), b.end(), [](wchar_t x, wchar_t y) {
          return std::towlower(x) < std::towlower(y);
        });
  }
};

// The shared factory's system collection is the one Flutter's Skia font
// manager uses; on Windows 10 1809+ it includes per-user fonts.
template <typename Visit>
std::vector<std::string> CollectFamilies(Visit visit) {
  try {
    ComPtr<IDWriteFactory> factory;
    if (FAILED(DWriteCreateFactory(
            DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory),
            reinterpret_cast<IUnknown**>(factory.GetAddressOf())))) {
      return {};
    }
    ComPtr<IDWriteFontCollection> collection;
    if (FAILED(factory->GetSystemFontCollection(&collection, FALSE)) ||
        !collection) {
      return {};
    }
    std::set<std::wstring, CaseInsensitiveLess> names;
    const UINT32 count = collection->GetFontFamilyCount();
    for (UINT32 i = 0; i < count; ++i) {
      ComPtr<IDWriteFontFamily> family;
      if (FAILED(collection->GetFontFamily(i, &family))) {
        continue;
      }
      ComPtr<IDWriteLocalizedStrings> family_names;
      if (FAILED(family->GetFamilyNames(&family_names))) {
        continue;
      }
      visit(family.Get(), family_names.Get(), names);
    }
    std::vector<std::string> result;
    result.reserve(names.size());
    for (const auto& name : names) {
      if (!name.empty() && name[0] != L'@') {  // Skip vertical CJK aliases.
        result.push_back(ToUtf8(name));
      }
    }
    return result;
  } catch (...) {
    return {};
  }
}

}  // namespace

std::vector<std::string> MonospaceFontFamilies() {
  return CollectFamilies([](IDWriteFontFamily* family,
                            IDWriteLocalizedStrings* family_names,
                            std::set<std::wstring, CaseInsensitiveLess>& out) {
    if (IsMonospacedFamily(family)) {
      out.insert(PrimaryName(family_names));
    }
  });
}

std::vector<std::string> InstalledFontFamilies() {
  return CollectFamilies([](IDWriteFontFamily*,
                            IDWriteLocalizedStrings* family_names,
                            std::set<std::wstring, CaseInsensitiveLess>& out) {
    const UINT32 count = family_names->GetCount();
    for (UINT32 i = 0; i < count; ++i) {
      out.insert(NameAt(family_names, i));
    }
  });
}
