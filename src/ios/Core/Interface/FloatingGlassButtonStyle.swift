import SwiftUI

enum FloatingGlassControl {
    static let height: CGFloat = 44
}

struct FloatingGlassButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .padding(.horizontal, 12)
            .frame(minWidth: FloatingGlassControl.height)
            .frame(height: FloatingGlassControl.height)
            .contentShape(.capsule)
            .glassEffect(.regular.interactive(), in: .capsule)
            .opacity(configuration.isPressed ? 0.75 : 1)
    }
}
