import XCTest

final class PhysicalBrowsingTests: XCTestCase {
    override func setUp() { super.setUp(); continueAfterFailure = false }
    @MainActor func testSignedInLibraryReadOnly() throws {
        guard ProcessInfo.processInfo.environment["MOMENTO_PHYSICAL_READONLY"] == "1" else {
            throw XCTSkip("Only run explicitly on the user's signed-in device")
        }
        let app = XCUIApplication()
        app.launch()
        XCTAssertTrue(app.buttons["Library"].waitForExistence(timeout: 20), "Sign in on the device before running read-only verification")
        capture(app, "Timeline")
        app.buttons["Library"].tap()
        let admin = app.buttons["nav.Admin"].exists
        let destinations = ["Photos", "Videos", "Screenshots", "Documents", "Albums", "Map", "Places", "Faces", "Deduplicate", "Trash", "Settings"]
            + (admin ? ["Import", "Metadata", "AI", "User Management"] : []) + ["Timeline"]
        for destination in destinations {
            let navigation = app.descendants(matching: .any).matching(identifier: "libraryNavigation").firstMatch
            if !navigation.exists { app.buttons["Library"].tap() }
            XCTAssertTrue(navigation.waitForExistence(timeout: 5))
            if ["Photos", "Videos"].contains(destination), app.buttons["Expand Timeline"].exists {
                app.buttons["Expand Timeline"].tap()
            }
            if destination == "Deduplicate", app.buttons["Expand Utility"].exists { app.buttons["Expand Utility"].tap() }
            if ["Import", "Metadata", "AI", "User Management"].contains(destination), app.buttons["Expand Admin"].exists { app.buttons["Expand Admin"].tap() }
            let button = app.buttons.matching(identifier: "nav.\(destination)").firstMatch
            for _ in 0..<5 where !button.exists || !button.isHittable {
                if ["Timeline", "Photos", "Videos"].contains(destination) { navigation.swipeDown() }
                else { navigation.swipeUp() }
            }
            XCTAssertTrue(button.exists, "Missing \(destination)")
            button.tap()
            XCTAssertTrue(app.buttons["Library"].waitForExistence(timeout: 10), "Navigation did not close after selecting \(destination)")
            let idle = XCTNSPredicateExpectation(predicate: NSPredicate(format: "count == 0"), object: app.activityIndicators)
            XCTAssertEqual(XCTWaiter.wait(for: [idle], timeout: 20), .completed)
            XCTAssertFalse(app.alerts["Momento"].exists, "Server error while browsing \(destination)")
            XCTAssertFalse(app.staticTexts["Could not load"].exists)
            capture(app, destination)
            if destination == "Photos" || destination == "Videos" { verifyViewer(app, video: destination == "Videos") }
            if ["Settings", "Import", "Metadata", "AI", "User Management"].contains(destination) {
                verifySettingsAndAdminLayout(app, destination: destination)
            }
        }
        XCUIDevice.shared.orientation = .landscapeLeft
        let rotated = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            app.frame.width > app.frame.height
        }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [rotated], timeout: 5), .completed)
        app.swipeUp()
        XCTAssertTrue(app.buttons["Library"].exists)
        capture(app, "Timeline landscape")
        XCUIDevice.shared.orientation = .portrait
    }

    @MainActor func testMediaPlaybackReadOnly() throws {
        guard ProcessInfo.processInfo.environment["MOMENTO_PHYSICAL_READONLY"] == "1" else { throw XCTSkip("Explicit signed-in device verification only") }
        let app = XCUIApplication(); app.launch()
        XCTAssertTrue(app.buttons["Library"].waitForExistence(timeout: 20))
        for destination in ["Photos", "Videos"] {
            app.buttons["Library"].tap()
            let navigation = app.descendants(matching: .any).matching(identifier: "libraryNavigation").firstMatch
            XCTAssertTrue(navigation.waitForExistence(timeout: 5))
            for _ in 0..<5 where !app.buttons["nav.\(destination)"].isHittable { navigation.swipeDown() }
            if app.buttons["Expand Timeline"].exists { app.buttons["Expand Timeline"].tap() }
            app.buttons["nav.\(destination)"].tap()
            verifyViewer(app, video: destination == "Videos")
        }
    }

    @MainActor private func verifySettingsAndAdminLayout(_ app: XCUIApplication, destination: String) {
        switch destination {
        case "Settings":
            XCTAssertLessThan(app.buttons["Change password"].frame.maxY, app.buttons["signOut"].frame.minY)
            XCTAssertEqual(app.buttons["Change password"].frame.maxX, app.buttons["signOut"].frame.maxX, accuracy: 2)
            XCTAssertTrue(app.switches["Camera photos and videos only"].exists)
        case "Import":
            XCTAssertTrue(app.staticTexts["WebDAV"].exists)
            XCTAssertTrue(app.staticTexts["Local Import"].exists)
        case "Metadata": XCTAssertTrue(app.buttons["Generate Metadata"].exists || app.buttons["Cancel Metadata"].exists)
        case "AI": XCTAssertTrue(app.scrollViews["adminAIStatusTable"].exists)
        default: XCTAssertTrue(app.scrollViews["adminUsersTable"].exists)
        }
        app.swipeUp()
        capture(app, "\(destination) lower controls")
        XCUIDevice.shared.orientation = .landscapeLeft
        let rotated = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in app.frame.width > app.frame.height }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [rotated], timeout: 5), .completed)
        XCTAssertFalse(app.alerts["Momento"].exists)
        capture(app, "\(destination) landscape layout")
        XCUIDevice.shared.orientation = .portrait
    }

    @MainActor private func verifyViewer(_ app: XCUIApplication, video: Bool) {
        let pattern = video ? ".*\\.(mp4|mov|m4v|mkv|avi|webm)$" : ".*\\.(jpg|jpeg|png|heic|heif|webp|gif|dng)$"
        let item = app.buttons.matching(NSPredicate(format: "label MATCHES[c] %@", pattern)).firstMatch
        XCTAssertTrue(item.waitForExistence(timeout: 20), "No media available to verify the viewer")
        item.tap()
        XCTAssertTrue(app.buttons["Close"].waitForExistence(timeout: 10))
        if video {
            let play = app.buttons.matching(NSPredicate(format: "label BEGINSWITH[c] 'Play'")).firstMatch
            let videoSurface = app.descendants(matching: .any).matching(identifier: "videoPlayer").firstMatch
            XCTAssertTrue(videoSurface.waitForExistence(timeout: 20))
            let visible = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value IN %@", ["Ready", "Poster"]), object: videoSurface)
            XCTAssertEqual(XCTWaiter.wait(for: [visible], timeout: 20), .completed, "Opening a video must display it without a second tap")
            capture(app, "Native video controls")
            let hierarchy = XCTAttachment(string: app.debugDescription)
            hierarchy.name = "Native video accessibility"; hierarchy.lifetime = .keepAlways; add(hierarchy)
            XCTAssertTrue(play.waitForExistence(timeout: 15))
            XCTAssertTrue(app.buttons["Mute video"].exists)
            app.buttons["Mute video"].tap(); XCTAssertTrue(app.buttons["Unmute video"].exists)
            XCTAssertFalse(app.buttons["Subtitles"].exists); XCTAssertFalse(app.buttons["Show Text"].exists)
            play.tap()
            let elapsed = app.staticTexts["videoElapsedTime"]
            let advances = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label != '0:00'"), object: elapsed)
            XCTAssertEqual(XCTWaiter.wait(for: [advances], timeout: 10), .completed)
            let pause = app.buttons.matching(NSPredicate(format: "label BEGINSWITH[c] 'Pause'")).firstMatch
            XCTAssertTrue(pause.waitForExistence(timeout: 15))
            capture(app, "Video playing")
            pause.tap()
            XCTAssertTrue(play.waitForExistence(timeout: 5))
            app.sliders["videoSeekBar"].adjust(toNormalizedSliderPosition: 0.5)
            XCTAssertNotEqual(elapsed.label, "0:00")
            app.buttons["Unmute video"].tap(); XCTAssertTrue(app.buttons["Mute video"].exists)
            if !app.buttons["Close"].exists { app.tap() }
        } else {
            XCTAssertTrue(app.images["viewerImage"].waitForExistence(timeout: 15), "Photo must appear on opening")
            app.buttons["Information"].firstMatch.tap()
            XCTAssertTrue(app.navigationBars["Information"].waitForExistence(timeout: 5))
            capture(app, "Photo information")
            app.buttons["Done"].firstMatch.tap()
        }
        app.buttons["Close"].tap()
        XCTAssertTrue(app.buttons["Library"].waitForExistence(timeout: 5))
    }

    @MainActor private func capture(_ app: XCUIApplication, _ name: String) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = "Read-only \(name)"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
