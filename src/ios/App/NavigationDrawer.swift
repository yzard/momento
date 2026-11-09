import SwiftUI

struct NavigationDrawer: View {
    @Environment(AppModel.self) private var model
    @SceneStorage("drawerTimelineExpanded") private var timelineExpanded = true
    @SceneStorage("drawerUtilityExpanded") private var utilityExpanded = false
    @SceneStorage("drawerAdminExpanded") private var adminExpanded = false
    let selected: Destination
    let select: (Destination) -> Void
    let logout: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                Image("BrandMark").resizable().scaledToFit().frame(width: 36, height: 36).clipShape(.rect(cornerRadius: 10)).accessibilityHidden(true)
                Text("Momento").font(.title.bold())
            }.frame(maxWidth: .infinity, alignment: .leading).padding(24)
            ScrollView {
                VStack(spacing: 4) {
                    group("Timeline", icon: Destination.timeline.icon, expanded: $timelineExpanded,
                          active: selected == .timeline || (!timelineExpanded && selected.timeline)) {
                        if selected == .timeline { timelineExpanded.toggle() } else { select(.timeline) }
                    }
                    if timelineExpanded {
                        ForEach([Destination.photos, .videos, .screenshots, .documents].filter { $0.available(model) }) { row($0, indented: true) }
                    }
                    ForEach([Destination.albums, .map, .places, .faces].filter { $0.available(model) }) { row($0) }
                    if Destination.duplicates.available(model) {
                        group("Utility", icon: "wrench.and.screwdriver", expanded: $utilityExpanded,
                              active: !utilityExpanded && selected == .duplicates) { utilityExpanded.toggle() }
                        if utilityExpanded { row(.duplicates, indented: true) }
                    }
                    row(.trash)
                    if model.session?.user.role == "admin" {
                        group("Admin", icon: "person.badge.shield.checkmark", expanded: $adminExpanded,
                              active: !adminExpanded && selected.admin) {
                            if selected.admin { adminExpanded.toggle() }
                            else { adminExpanded = true; select(.imports) }
                        }
                        if adminExpanded { ForEach(Destination.allCases.filter(\.admin)) { row($0, indented: true) } }
                    }
                }.padding(.horizontal, 12)
            }
            Divider()
            VStack(spacing: 4) {
                row(.settings)
                Button(action: logout) { label("Logout", icon: "rectangle.portrait.and.arrow.right") }
                    .buttonStyle(.plain).accessibilityIdentifier("nav.Logout")
            }.padding(12)
        }.accessibilityElement(children: .contain).accessibilityIdentifier("libraryNavigation")
    }

    private func row(_ destination: Destination, indented: Bool = false) -> some View {
        Button { select(destination) } label: { label(destination.rawValue, icon: destination.icon) }
            .buttonStyle(.plain)
            .background(selected == destination ? Color.primary.opacity(0.12) : .clear, in: .capsule)
            .accessibilityAddTraits(selected == destination ? .isSelected : [])
            .padding(.leading, indented ? 20 : 0)
            .accessibilityIdentifier("nav.\(destination.rawValue)")
    }

    private func group(_ title: String, icon: String, expanded: Binding<Bool>, active: Bool, action: @escaping () -> Void) -> some View {
        HStack(spacing: 0) {
            Button(action: action) { label(title, icon: icon) }.buttonStyle(.plain).accessibilityIdentifier("nav.\(title)")
            Button { expanded.wrappedValue.toggle() } label: {
                Image(systemName: expanded.wrappedValue ? "chevron.up" : "chevron.down").frame(width: 44, height: 48)
            }.buttonStyle(.plain).accessibilityLabel("\(expanded.wrappedValue ? "Collapse" : "Expand") \(title)")
        }
        .background(active ? Color.primary.opacity(0.12) : .clear, in: .capsule)
        .accessibilityAddTraits(active ? .isSelected : [])
    }

    private func label(_ title: String, icon: String) -> some View {
        HStack(spacing: 12) {
            Image(systemName: icon).frame(width: 24)
            Text(title).font(.subheadline.weight(.medium))
            Spacer(minLength: 0)
        }.padding(.horizontal, 16).frame(minHeight: 48).contentShape(Rectangle())
    }
}
