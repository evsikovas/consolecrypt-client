import Cocoa
import CryptoKit
import Darwin
import FlutterMacOS
import UniformTypeIdentifiers

enum UpdateExportError: String, Error {
  case arguments, storage, destinationExists = "destination_exists", checksum, installer
}

/// Write fresh bytes, rather than copying the cache's sandbox quarantine
/// attributes. Only an NSSavePanel-approved URL may be passed as destination.
/// App Sandbox and Gatekeeper remain enabled; executable consent comes from
/// the save panel and the user-selected.executable entitlement.
enum VerifiedUpdateExport {
  static func write(source: URL, destination: URL, bytes: Int64, sha256: String) throws {
    let inputDescriptor = Darwin.open(source.path, O_RDONLY | O_NOFOLLOW)
    guard inputDescriptor >= 0 else { throw UpdateExportError.storage }
    let input = FileHandle(fileDescriptor: inputDescriptor, closeOnDealloc: true)
    defer { try? input.close() }
    var info = stat()
    guard fstat(inputDescriptor, &info) == 0,
      info.st_mode & S_IFMT == S_IFREG, info.st_size == bytes
    else { throw UpdateExportError.checksum }

    // Never truncate an existing DMG (including one with the old quarantine
    // mark), follow a link, or overwrite the running app. Choose a new name.
    let outputDescriptor = Darwin.open(destination.path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0o600)
    guard outputDescriptor >= 0 else {
      throw errno == EEXIST ? UpdateExportError.destinationExists : UpdateExportError.storage
    }
    let output = FileHandle(fileDescriptor: outputDescriptor, closeOnDealloc: true)
    var completed = false
    defer {
      try? output.close()
      if !completed { try? FileManager.default.removeItem(at: destination) }
    }
    var count: Int64 = 0
    var hash = SHA256()
    while let data = try input.read(upToCount: 65536), !data.isEmpty {
      count += Int64(data.count)
      guard count <= bytes else { throw UpdateExportError.checksum }
      hash.update(data: data)
      try output.write(contentsOf: data)
    }
    let actual = hash.finalize().map { String(format: "%02x", $0) }.joined()
    guard count == bytes, actual == sha256 else { throw UpdateExportError.checksum }
    try output.synchronize()
    try output.close()
    completed = true
  }
}

final class UpdateBridge {
  private let channel: FlutterMethodChannel
  private weak var window: NSWindow?
  private var busy = false

  init(messenger: FlutterBinaryMessenger, window: NSWindow) {
    self.window = window
    channel = FlutterMethodChannel(name: "consolecrypt/macos_updates", binaryMessenger: messenger)
    channel.setMethodCallHandler { [weak self] call, result in
      guard let self = self else { return result(FlutterMethodNotImplemented) }
      guard call.method == "saveAndOpen" else { return result(FlutterMethodNotImplemented) }
      self.saveAndOpen(call.arguments, result: result)
    }
  }

  private func saveAndOpen(_ arguments: Any?, result: @escaping FlutterResult) {
    guard !busy, let args = arguments as? [String: Any],
      let path = args["path"] as? String,
      let name = args["fileName"] as? String,
      name.range(of: #"^ConsoleCrypt-[0-9]+\.[0-9]+\.[0-9]+\+[0-9]+-macos-universal\.dmg$"#,
        options: .regularExpression) != nil,
      let bytes = args["bytes"] as? Int64, bytes > 0, bytes <= 524288000,
      let hash = args["sha256"] as? String,
      hash.range(of: #"^[a-f0-9]{64}$"#, options: .regularExpression) != nil,
      let title = args["title"] as? String, title.count <= 200,
      ((args["prompt"] as? String)?.count ?? 0) <= 100,
      let window = window
    else { return result("arguments") }
    let source = URL(fileURLWithPath: path)
    let cache = FileManager.default.temporaryDirectory.resolvingSymlinksInPath()
    let canonical = source.resolvingSymlinksInPath()
    guard canonical.lastPathComponent == name,
      canonical.deletingLastPathComponent().lastPathComponent.hasPrefix("consolecrypt-update-"),
      canonical.deletingLastPathComponent().deletingLastPathComponent() == cache
    else { return result("arguments") }

    busy = true
    let panel = NSSavePanel()
    panel.title = title
    if let prompt = args["prompt"] as? String { panel.prompt = prompt }
    panel.nameFieldStringValue = name
    panel.allowedContentTypes = [UTType(filenameExtension: "dmg")!]
    panel.canCreateDirectories = true
    panel.beginSheetModal(for: window) { [weak self] response in
      guard let self = self else { return result("storage") }
      guard response == .OK, let destination = panel.url else {
        self.busy = false
        return result("cancelled")
      }
      let scoped = destination.startAccessingSecurityScopedResource()
      DispatchQueue.global(qos: .userInitiated).async {
        let outcome: String
        do {
          try VerifiedUpdateExport.write(source: canonical, destination: destination, bytes: bytes, sha256: hash)
          outcome = "saved"
        } catch let error as UpdateExportError {
          outcome = error.rawValue
        } catch {
          outcome = "storage"
        }
        DispatchQueue.main.async {
          defer {
            self.busy = false
            if scoped { destination.stopAccessingSecurityScopedResource() }
          }
          guard outcome == "saved" else { return result(outcome) }
          result(NSWorkspace.shared.open(destination) ? "opened" : "installer")
        }
      }
    }
  }
}
