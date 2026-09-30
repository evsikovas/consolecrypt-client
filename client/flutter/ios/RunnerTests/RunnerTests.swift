import Flutter
import UIKit
import XCTest

class RunnerTests: XCTestCase {

  func testSensitiveDocumentsAreNotSharedByTheRunner() {
    let info = Bundle.main.infoDictionary!
    XCTAssertEqual(info["UIFileSharingEnabled"] as? Bool, false)
    XCTAssertEqual(info["LSSupportsOpeningDocumentsInPlace"] as? Bool, false)
  }

  func testFaceIDHasRequiredPurposeAndVersionUsesNativeMetadata() {
    let info = Bundle.main.infoDictionary!
    XCTAssertFalse((info["NSFaceIDUsageDescription"] as? String ?? "").isEmpty)
    XCTAssertFalse((info["CFBundleShortVersionString"] as? String ?? "").contains("$("))
    XCTAssertNotNil(Int(info["CFBundleVersion"] as? String ?? ""))
  }

}
