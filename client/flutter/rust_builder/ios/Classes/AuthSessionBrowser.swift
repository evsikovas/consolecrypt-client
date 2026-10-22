import AuthenticationServices
import Flutter
import UIKit

/// The OS owns the authentication browser (`ASWebAuthenticationSession`).
/// Rust owns PKCE, callback state, issuer verification and the resulting
/// tokens. One instance serves one fixed redirect URI:
/// * Kubernetes OIDC — `consolecrypt://kubernetes/oauth/callback`, ephemeral;
/// * enterprise sign-in to the sync server (protocol 1.8) —
///   `consolecrypt://auth/callback`, sharing the browser session so an
///   existing sign-in at the organisation's IdP is reused.
final class AuthSessionBrowser: NSObject, ASWebAuthenticationPresentationContextProviding {
  private weak var registrar: FlutterPluginRegistrar?
  private let redirect: URL
  private let ephemeral: Bool
  /// Plain `http` is accepted only for a loopback development server.
  private let allowLoopbackHttp: Bool
  private var session: ASWebAuthenticationSession?
  private var completion: FlutterResult?
  private var ticket: String?

  init(registrar: FlutterPluginRegistrar, redirectUri: String, ephemeral: Bool, allowLoopbackHttp: Bool) {
    self.registrar = registrar
    self.redirect = URL(string: redirectUri)!
    self.ephemeral = ephemeral
    self.allowLoopbackHttp = allowLoopbackHttp
    super.init()
  }

  private func acceptableScheme(_ url: URL) -> Bool {
    if url.scheme == "https" { return true }
    guard allowLoopbackHttp, url.scheme == "http", let host = url.host else { return false }
    return ["localhost", "127.0.0.1", "::1"].contains(host)
  }

  func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    let arguments = call.arguments as? [String: Any]
    switch call.method {
    case "openBrowser":
      guard session == nil else {
        result(FlutterError(code: "busy", message: "An authentication browser is already open", details: nil))
        return
      }
      guard let raw = arguments?["url"] as? String,
            let url = URL(string: raw), acceptableScheme(url), url.host != nil,
            url.user == nil, url.password == nil, url.fragment == nil,
            arguments?["redirectUri"] as? String == redirect.absoluteString,
            let requestTicket = arguments?["ticket"] as? String,
            !requestTicket.isEmpty, requestTicket.count <= 64,
            let parts = URLComponents(url: url, resolvingAgainstBaseURL: false),
            let states = parts.queryItems?.filter({ $0.name == "state" }),
            states.count == 1, let state = states.first?.value,
            (32...256).contains(state.count),
            registrar?.viewController?.view.window != nil else {
        result(FlutterError(code: "browser", message: "Authentication browser unavailable", details: nil))
        return
      }
      completion = result
      ticket = requestTicket
      let expected = redirect
      let browser = ASWebAuthenticationSession(url: url, callbackURLScheme: expected.scheme) {
        [weak self] callback, _ in
        DispatchQueue.main.async {
          guard let self = self, self.ticket == requestTicket else { return }
          guard let callback = callback,
                callback.absoluteString.count <= 32768,
                callback.scheme == expected.scheme, callback.host == expected.host,
                callback.path == expected.path, callback.user == nil,
                callback.password == nil, callback.fragment == nil,
                let callbackParts = URLComponents(url: callback, resolvingAgainstBaseURL: false),
                let callbackStates = callbackParts.queryItems?.filter({ $0.name == "state" }),
                callbackStates.count == 1, callbackStates.first?.value == state else {
            self.finish(nil)
            return
          }
          self.finish(callback.absoluteString)
        }
      }
      browser.presentationContextProvider = self
      browser.prefersEphemeralWebBrowserSession = ephemeral
      session = browser
      if !browser.start() {
        finish(nil)
      }
    case "cancelBrowser":
      if ticket == (arguments?["ticket"] as? String) {
        session?.cancel()
        finish(nil)
      }
      result(nil)
    default:
      result(FlutterMethodNotImplemented)
    }
  }

  func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
    // The view window is checked before starting the session. It remains the
    // presentation anchor for this scene, rather than another connected scene.
    registrar?.viewController?.view.window ?? ASPresentationAnchor()
  }

  private func finish(_ callback: String?) {
    let result = completion
    completion = nil
    session = nil
    ticket = nil
    result?(callback)
  }

  deinit { session?.cancel() }
}
