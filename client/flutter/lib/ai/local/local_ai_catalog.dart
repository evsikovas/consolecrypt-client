import 'dart:ffi';

enum LocalAiPlatform { macArm64, macX64, windowsX64, unsupported }

enum LocalAiTier { light, medium, strong }

/// Only this reviewed catalog can supply executable/model download URLs.
/// llama.cpp: https://github.com/ggml-org/llama.cpp/releases/tag/b11739
/// Qwen: https://huggingface.co/Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF
class LocalAiArtifact {
  const LocalAiArtifact({required this.url, required this.sha256, required this.bytes, required this.filename});
  final String url, sha256, filename;
  final int bytes;
}

class LocalAiModel extends LocalAiArtifact {
  const LocalAiModel({
    required this.id,
    required this.label,
    required super.url,
    required super.sha256,
    required super.bytes,
    required super.filename,
    this.recommendedRamGiB = 4,
    this.disableReasoning = false,
    this.tier = LocalAiTier.light,
    this.publisher = 'Qwen',
  });
  final String id, label;
  final int recommendedRamGiB;
  final bool disableReasoning;
  final LocalAiTier tier;
  final String publisher;
  String get license => 'Apache-2.0';
  String get sourceUrl => url.split('/resolve/').first;
}

abstract final class LocalAiCatalog {
  static const runtimeVersion = 'b11739';
  static const runtimeCommit = '0ce8cd11416c8840498e6701bcbe39ad4b858c2b';
  static const model = LocalAiModel(
    id: 'qwen2.5-coder-1.5b-instruct-q4_k_m',
    label: 'Qwen2.5-Coder 1.5B · Q4_K_M', // l10n-ignore: canonical model name
    url:
        'https://huggingface.co/Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF/resolve/'
        'f86cb2c1fa58255f8052cc32aeede1b7482d4361/qwen2.5-coder-1.5b-instruct-q4_k_m.gguf',
    sha256: 'cc324af070c2ecbfd324a30884d2f951a7ff756aba85cb811a6ec436933bb046',
    bytes: 1117320768,
    filename: 'qwen2.5-coder-1.5b-instruct-q4_k_m.gguf',
  );
  static const qwen4B = LocalAiModel(
    id: 'qwen3.5-4b-q4_k_m',
    label: 'Qwen3.5 4B · Q4_K_M', // l10n-ignore: canonical model name
    url:
        'https://huggingface.co/unsloth/Qwen3.5-4B-GGUF/resolve/'
        'e87f176479d0855a907a41277aca2f8ee7a09523/Qwen3.5-4B-Q4_K_M.gguf',
    sha256: '00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4',
    bytes: 2740937888,
    filename: 'Qwen3.5-4B-Q4_K_M.gguf',
    recommendedRamGiB: 8,
    disableReasoning: true,
    tier: LocalAiTier.medium,
    publisher: 'Qwen / Unsloth', // l10n-ignore: canonical model publishers
  );
  static const qwen9B = LocalAiModel(
    id: 'qwen3.5-9b-q4_k_m',
    label: 'Qwen3.5 9B · Q4_K_M', // l10n-ignore: canonical model name
    url:
        'https://huggingface.co/unsloth/Qwen3.5-9B-GGUF/resolve/'
        '3885219b6810b007914f3a7950a8d1b469d598a5/Qwen3.5-9B-Q4_K_M.gguf',
    sha256: '03b74727a860a56338e042c4420bb3f04b2fec5734175f4cb9fa853daf52b7e8',
    bytes: 5680522464,
    filename: 'Qwen3.5-9B-Q4_K_M.gguf',
    recommendedRamGiB: 16,
    disableReasoning: true,
    tier: LocalAiTier.strong,
    publisher: 'Qwen / Unsloth', // l10n-ignore: canonical model publishers
  );
  // Keep the original model first: upgrades never choose a larger model for the user.
  static const models = [model, qwen4B, qwen9B];
  static const runtimes = {
    LocalAiPlatform.macArm64: LocalAiArtifact(
      url: 'https://github.com/ggml-org/llama.cpp/releases/download/b11739/llama-b11739-bin-macos-arm64.tar.gz',
      sha256: 'a2073ed1aa00904d408a6694429aa9b6dcf4949c2683206c403930849ce222ac',
      bytes: 12324618,
      filename: 'llama-b11739-bin-macos-arm64.tar.gz',
    ),
    LocalAiPlatform.macX64: LocalAiArtifact(
      url: 'https://github.com/ggml-org/llama.cpp/releases/download/b11739/llama-b11739-bin-macos-x64.tar.gz',
      sha256: 'f439aa26fa4d60ff60053b06ed901efe75865cd381063d1e65a0407a0d2761e6',
      bytes: 11799331,
      filename: 'llama-b11739-bin-macos-x64.tar.gz',
    ),
    LocalAiPlatform.windowsX64: LocalAiArtifact(
      url: 'https://github.com/ggml-org/llama.cpp/releases/download/b11739/llama-b11739-bin-win-cpu-x64.zip',
      sha256: 'b1e0a14abdb4665c1b821b377ba063b221cec62882c373065cf8fb672988d3ff',
      bytes: 19708508,
      filename: 'llama-b11739-bin-win-cpu-x64.zip',
    ),
  };

  static LocalAiPlatform get platform => switch (Abi.current()) {
    Abi.macosArm64 => LocalAiPlatform.macArm64,
    Abi.macosX64 => LocalAiPlatform.macX64,
    Abi.windowsX64 => LocalAiPlatform.windowsX64,
    _ => LocalAiPlatform.unsupported,
  };
}
