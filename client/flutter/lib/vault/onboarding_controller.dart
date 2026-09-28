import 'dart:math';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

final class OnboardingState {
  const OnboardingState({this.kit, this.positions = const []});

  static const empty = OnboardingState();

  /// Held in memory only between "create vault" and verification.
  final RecoveryKit? kit;

  /// 0-based indexes of the words the user must re-enter (sorted).
  final List<int> positions;
}

/// Vault creation + mandatory Recovery Kit verification (CLIENT_SPEC §8.3,
/// ADR-0002: re-enter 3 random words before continuing).
class OnboardingController extends Notifier<OnboardingState> {
  OnboardingController({Random? random}) : _random = random ?? Random.secure();

  final Random _random;

  static const wordsToVerify = 3;

  @override
  OnboardingState build() => OnboardingState.empty;

  /// Distinct random positions in `[0, total)`, sorted ascending.
  static List<int> pickPositions(Random random, {int count = wordsToVerify, int total = RecoveryKit.wordCount}) {
    final picked = <int>{};
    while (picked.length < count) {
      picked.add(random.nextInt(total));
    }
    return picked.toList()..sort();
  }

  Future<void> createVault({required String name, required SecretText passphrase}) async {
    try {
      final kit = await ref.read(vaultServiceProvider).createVault(name: name, passphrase: passphrase);
      _setKit(kit);
    } finally {
      passphrase.wipe();
    }
  }

  /// After an app restart mid-onboarding the kit is gone from memory.
  Future<void> regenerateKit() async {
    final kit = await ref.read(vaultServiceProvider).regenerateRecoveryKit();
    _setKit(kit);
  }

  void _setKit(RecoveryKit kit) {
    state.kit?.forget();
    state = OnboardingState(kit: kit, positions: pickPositions(_random));
  }

  /// Case-insensitive comparison of the requested words.
  bool verify(Map<int, String> answers) {
    final kit = state.kit;
    if (kit == null) return false;
    final words = kit.exposeWords();
    return state.positions.every((p) => (answers[p] ?? '').trim().toLowerCase() == words[p]);
  }

  /// Marks the kit as saved and drops it from memory.
  Future<void> complete() async {
    await ref.read(vaultServiceProvider).confirmRecoveryKitSaved();
    state.kit?.forget();
    state = OnboardingState.empty;
  }
}

final onboardingControllerProvider = NotifierProvider<OnboardingController, OnboardingState>(OnboardingController.new);
