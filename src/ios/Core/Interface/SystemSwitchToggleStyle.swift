import SwiftUI

/// Keep native switches green when enabled, independent of monochrome button tint.
struct SystemSwitchToggleStyle: ToggleStyle {
    func makeBody(configuration: Configuration) -> some View {
        Toggle(configuration).toggleStyle(.switch).tint(Color(uiColor: .systemGreen))
    }
}
