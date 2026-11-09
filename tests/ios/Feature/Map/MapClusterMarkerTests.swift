import SwiftUI
import Testing
import UIKit
@testable import Momento

@MainActor struct MapClusterMarkerTests {
    @Test func representativePhotoHasOneUpperRightBadgeWithoutACaptionRow() throws {
        let group = try render(count: 12)
        let single = try render(count: 1)
        #expect(group.width == 60 && group.height == 60)
        #expect(single.width == group.width && single.height == group.height)
        let photo = pixel(group, x: 26, y: 34)
        #expect(photo[0] > 240 && photo[1] < 10 && photo[2] < 10)
        let groupCorner = pixel(group, x: 56, y: 10)
        let singleCorner = pixel(single, x: 56, y: 10)
        #expect(groupCorner[3] > singleCorner[3] + 100)
    }
    private func render(count: Int64) throws -> CGImage {
        let renderer = ImageRenderer(content: MapClusterMarker(count: count) { Color(red: 1, green: 0, blue: 0) })
        renderer.scale = 1
        return try #require(renderer.uiImage?.cgImage)
    }
    private func pixel(_ image: CGImage, x: Int, y: Int) -> [Int] {
        var bytes = [UInt8](repeating: 0, count: image.width * image.height * 4)
        bytes.withUnsafeMutableBytes { buffer in
            let context = CGContext(data: buffer.baseAddress, width: image.width, height: image.height,
                bitsPerComponent: 8, bytesPerRow: image.width * 4, space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
            context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        }
        let offset = (y * image.width + x) * 4
        return bytes[offset..<(offset + 4)].map(Int.init)
    }
}
