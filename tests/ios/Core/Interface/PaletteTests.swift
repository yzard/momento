import Testing
import UIKit
@testable import Momento

@MainActor struct PaletteTests {
    @Test func accentAdaptsToBlackAndWhiteWithOpaqueContrast() throws {
        let color = try #require(UIColor(named: "InterfaceAccent"))
        let content = try #require(UIColor(named: "InterfaceOnAccent"))
        let appearances: [(UIUserInterfaceStyle, CGFloat)] = [(.light, 0), (.dark, 1)]
        for (style, expected) in appearances {
            let resolved = color.resolvedColor(with: UITraitCollection(userInterfaceStyle: style))
            var red: CGFloat = 0, green: CGFloat = 0, blue: CGFloat = 0, alpha: CGFloat = 0
            #expect(resolved.getRed(&red, green: &green, blue: &blue, alpha: &alpha))
            #expect(red == expected && green == expected && blue == expected && alpha == 1)
            let foreground = content.resolvedColor(with: UITraitCollection(userInterfaceStyle: style))
            #expect(foreground.getRed(&red, green: &green, blue: &blue, alpha: &alpha))
            #expect(red == 1 - expected && green == 1 - expected && blue == 1 - expected && alpha == 1)
        }
    }
    @Test func sharedBrandMarkLoads() throws {
        let mark = try #require(UIImage(named: "BrandMark"))
        #expect(mark.size.width == mark.size.height)
        #expect(mark.size.width >= 192)
    }
}
