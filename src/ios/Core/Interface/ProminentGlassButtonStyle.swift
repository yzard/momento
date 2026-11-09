import SwiftUI

/// Native prominent glass with foreground contrast matched to the neutral accent.
struct ProminentGlassButtonStyle: PrimitiveButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        Button(role: configuration.role, action: configuration.trigger) {
            configuration.label.foregroundStyle(Color("InterfaceOnAccent"))
        }.buttonStyle(.glassProminent)
    }
}
