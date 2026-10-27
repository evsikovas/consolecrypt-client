# irondash_engine_context 0.5.5

This directory contains the published irondash_engine_context 0.5.5 package by Superlist /
irondash (MIT, see `LICENSE`; the upstream grant is preserved).

- Published source: https://pub.dev/packages/irondash_engine_context/versions/0.5.5

Copies of pub.dev packages with two Android build changes each:

- `cargokit/gradle/plugin.gradle` is replaced by the Cargokit plugin from
  `../../rust_builder/cargokit`, which runs under Gradle 9. The published plugin
  calls `Project.exec`, which Gradle 9 removed, and the Android build fails.
- `android/build.gradle` compiles against Android 36 (was 31): the AndroidX
  libraries of the app need 33 or later. `minSdkVersion` is unchanged.

| Package | Version | License | Why |
|---|---|---|---|
| `irondash_engine_context` | 0.5.5 | MIT | dependency of `super_native_extensions` |
| `super_native_extensions` | 0.9.1 | MIT | native drag and drop (`super_drag_and_drop`, `super_clipboard`) |

The app `pubspec.yaml` points at them through `dependency_overrides`. Remove a copy
once a published version builds with Gradle 9.
