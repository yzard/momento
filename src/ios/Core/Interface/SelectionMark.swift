import SwiftUI

struct SelectionMark: View {
    let selected: Bool
    var body: some View {
        ZStack {
            Circle().fill(selected ? Color.black.opacity(0.75) : .clear)
            Circle().strokeBorder(.white, lineWidth: 2)
            if selected { Image(systemName: "checkmark").font(.system(size: 13, weight: .bold)).foregroundStyle(.white) }
        }.frame(width: 24, height: 24).contentShape(Circle()).accessibilityHidden(true)
    }
}

struct SelectionActionsKey: PreferenceKey {
    static var defaultValue: [Destination: AnyView] { [:] }
    static func reduce(value: inout [Destination: AnyView], nextValue: () -> [Destination: AnyView]) {
        value.merge(nextValue(), uniquingKeysWith: { _, new in new })
    }
}


struct TrailingSelectionActions<Content: View>: View {
    let content: Content
    init(@ViewBuilder content: () -> Content) { self.content = content() }
    var body: some View {
        GeometryReader { geometry in
            ScrollView(.horizontal) {
                HStack { content.fixedSize(horizontal: true, vertical: false) }.frame(minWidth: geometry.size.width, alignment: .trailing)
            }.scrollIndicators(.hidden)
        }.frame(height: 44).accessibilityElement(children: .contain).accessibilityIdentifier("selectionActions")
    }
}
