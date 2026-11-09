import SwiftUI

/// The same representative photo and single corner count used by Web and Android.
struct MapClusterMarker<Thumbnail: View>: View {
    let count: Int64
    let thumbnail: Thumbnail
    init(count: Int64, @ViewBuilder thumbnail: () -> Thumbnail) {
        self.count = count
        self.thumbnail = thumbnail()
    }
    var body: some View {
        thumbnail.frame(width: 52, height: 52)
            .clipShape(.rect(cornerRadius: 10))
            .overlay { RoundedRectangle(cornerRadius: 10).strokeBorder(.white, lineWidth: 2) }
            .shadow(color: .black.opacity(0.3), radius: 6, y: 3)
            .overlay(alignment: .topTrailing) {
                if count > 1 {
                    Text("\(count)").font(.system(size: 12, weight: .bold)).monospacedDigit()
                        .foregroundStyle(.white).padding(.horizontal, 6).padding(.vertical, 3)
                        .background(.black.opacity(0.85), in: .capsule)
                        .overlay { Capsule().strokeBorder(.white, lineWidth: 2) }
                        .fixedSize().offset(x: 8, y: -8).accessibilityHidden(true)
                }
            }.padding(.top, 8).padding(.trailing, 8)
    }
}
