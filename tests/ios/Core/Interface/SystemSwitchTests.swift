import SwiftUI
import Testing
import UIKit
@testable import Momento

@MainActor struct SystemSwitchTests {
    @Test func enabledSwitchStaysGreenUnderWhiteButtonTint() throws {
        let host = UIHostingController(rootView:
            Toggle("Camera photos and videos only", isOn: .constant(true))
                .toggleStyle(SystemSwitchToggleStyle()).tint(.white).padding()
        )
        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 400, height: 100))
        window.overrideUserInterfaceStyle = .dark
        window.rootViewController = host
        window.makeKeyAndVisible()
        defer { window.isHidden = true }
        host.view.layoutIfNeeded()
        let control = try #require(findSwitch(host.view))
        #expect(control.isOn)
        let color = try #require(control.onTintColor).resolvedColor(with: control.traitCollection)
        var red: CGFloat = 0, green: CGFloat = 0, blue: CGFloat = 0, alpha: CGFloat = 0
        #expect(color.getRed(&red, green: &green, blue: &blue, alpha: &alpha))
        #expect(green > red + 0.2 && green > blue + 0.2 && alpha == 1)
    }
    private func findSwitch(_ view: UIView) -> UISwitch? {
        if let control = view as? UISwitch { return control }
        return view.subviews.lazy.compactMap { findSwitch($0) }.first
    }
}
