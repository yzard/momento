import XCTest

final class LaunchTests: XCTestCase {
    override func setUp() { super.setUp(); continueAfterFailure = false }
    @MainActor private func signInButton(_ app: XCUIApplication) -> XCUIElement {
        let button = app.buttons["Sign in"]
        // The native Form lazily creates rows outside the keyboard's visible viewport.
        for _ in 0..<3 where !button.exists || !button.isHittable { app.swipeUp() }
        XCTAssertTrue(button.exists)
        return button
    }
    @MainActor func testALoginSurvivesRotationAndProtectsPassword() throws {
        let app = XCUIApplication()
        app.launch()
        XCTAssertTrue(app.staticTexts["Your moments. Your home."].waitForExistence(timeout: 10))
        XCTAssertTrue(app.textFields["server"].exists)
        XCTAssertTrue(app.secureTextFields["password"].exists)
        let username = app.textFields["username"]
        username.tap(); username.typeText("rotation-test")
        XCUIDevice.shared.orientation = .landscapeLeft
        XCTAssertEqual(app.textFields["username"].value as? String, "rotation-test")
        XCUIDevice.shared.orientation = .portrait
        XCTAssertEqual(app.textFields["username"].value as? String, "rotation-test")
        XCTAssertFalse(signInButton(app).isEnabled)
        let attachment = XCTAttachment(screenshot: app.screenshot()); attachment.name = "Momento native sign-in"; attachment.lifetime = .keepAlways; add(attachment)
    }

    @MainActor func testBLibraryKeepsDataAcrossNavigationAndAlbumEditing() async throws {
        let origin = try XCTUnwrap(ProcessInfo.processInfo.environment["MOMENTO_UI_TEST_ORIGIN"])
        XCTAssertTrue(origin.hasPrefix("http://127.0.0.1:"))
        let app = XCUIApplication()
        app.launch()
        XCTAssertTrue(app.textFields["server"].waitForExistence(timeout: 10))
        app.textFields["server"].tap()
        if let previous = app.textFields["server"].value as? String, previous.hasPrefix("http") {
            app.textFields["server"].typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: previous.count))
        }
        app.textFields["server"].typeText(origin)
        app.textFields["username"].tap(); app.textFields["username"].typeText("ios-test")
        app.secureTextFields["password"].tap(); app.secureTextFields["password"].typeText("fixture-only")
        signInButton(app).tap()
        XCTAssertTrue(app.buttons["photo-1.jpg"].waitForExistence(timeout: 15), app.debugDescription)
        if app.buttons["Not Now"].waitForExistence(timeout: 3) { app.buttons["Not Now"].tap() }
        // Also verify Keychain restoration independently of the system's password-save window.
        app.terminate(); app.launch()
        XCTAssertTrue(app.buttons["photo-1.jpg"].waitForExistence(timeout: 10))
        for button in [app.buttons["Library"], app.buttons["Search"], app.buttons["Day"],
                       app.buttons["timelineDate"], app.buttons["timelineSelection"]] {
            XCTAssertEqual(button.frame.height, 44, accuracy: 1, "Floating controls must share the same height")
        }
        app.buttons["Select"].tap()
        let firstPhoto = app.buttons["photo-1.jpg"]
        if firstPhoto.isHittable { firstPhoto.tap() }
        else {
            // Validate the actual touch path when older XCTest cannot resolve a SwiftUI hit point.
            firstPhoto.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap()
        }
        let selected = app.staticTexts["1 selected"]
        XCTAssertTrue(selected.waitForExistence(timeout: 5))
        XCTAssertEqual(selected.frame.midY, app.buttons["Library"].frame.midY, accuracy: 2,
                       "Selection actions must share the Library control row")
        XCTAssertGreaterThan(selected.frame.minX, app.buttons["Library"].frame.maxX)
        XCTAssertFalse(app.buttons["Day"].exists, "Selection replaces the controls to the right of Library")
        app.buttons["Done"].tap()
        app.buttons["Select"].tap()
        XCTAssertTrue(app.staticTexts["0 selected"].waitForExistence(timeout: 5),
                      "Finishing selection must clear the previous selection, matching Android")
        app.buttons["Done"].tap()
        app.buttons["Library"].tap()
        let drawer = app.descendants(matching: .any).matching(identifier: "libraryNavigation").firstMatch
        XCTAssertTrue(drawer.waitForExistence(timeout: 5))
        // SceneStorage deliberately retains the user's expanded groups across launches.
        for group in ["Utility", "Admin"] {
            let collapse = app.buttons["Collapse \(group)"]
            if collapse.exists {
                for _ in 0..<6 where !collapse.isHittable { drawer.swipeUp() }
                collapse.tap()
            }
        }
        if app.buttons["Expand Timeline"].exists {
            for _ in 0..<6 where !app.buttons["Expand Timeline"].isHittable { drawer.swipeDown() }
            app.buttons["Expand Timeline"].tap()
        }
        let drawerShot = XCTAttachment(screenshot: app.screenshot()); drawerShot.name = "Android parity drawer"; drawerShot.lifetime = .keepAlways; add(drawerShot)
        XCTAssertTrue(app.buttons["nav.Photos"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["nav.Settings"].exists)
        XCTAssertTrue(app.buttons["nav.Logout"].exists)
        XCTAssertFalse(app.buttons["nav.Deduplicate"].exists)
        for _ in 0..<4 where !app.buttons["Expand Utility"].isHittable { drawer.swipeUp() }
        app.buttons["Expand Utility"].tap()
        XCTAssertTrue(app.buttons["nav.Deduplicate"].exists)
        app.buttons["Collapse Utility"].tap()
        navigate(app, to: "Photos")
        navigate(app, to: "Timeline")
        let before = try await metrics(origin)
        navigate(app, to: "Albums")
        app.buttons["New album"].tap()
        app.textFields["Name"].tap(); app.textFields["Name"].typeText("Native album")
        app.buttons["Save"].tap()
        XCTAssertTrue(app.buttons.containing(.staticText, identifier: "Native album").firstMatch.waitForExistence(timeout: 10))
        navigate(app, to: "Timeline")
        XCTAssertTrue(app.buttons["photo-1.jpg"].waitForExistence(timeout: 5))
        let after = try await metrics(origin)
        XCTAssertEqual(before["/api/v1/timeline/list"], after["/api/v1/timeline/list"], "Tab return must not reload the timeline")
        app.buttons["Library"].tap()
        let navigation = app.descendants(matching: .any).matching(identifier: "libraryNavigation").firstMatch
        for _ in 0..<4 where !app.buttons["Expand Utility"].isHittable { navigation.swipeUp() }
        app.buttons["Expand Utility"].tap()
        app.buttons["nav.Deduplicate"].tap()
        let comparison = app.buttons["View comparison"]
        XCTAssertTrue(comparison.waitForExistence(timeout: 10))
        for _ in 0..<4 where !comparison.isHittable { app.swipeUp() }
        comparison.tap()
        XCTAssertTrue(app.buttons["viewerThumbnail.1"].waitForExistence(timeout: 5),
                      "Comparison must present its media on the first opening")
        XCTAssertTrue(app.buttons["viewerThumbnail.3"].exists)
        let closeFrame = app.buttons["viewerClose"].frame
        let headingFrame = app.buttons["viewerDateLocation"].frame
        let moreFrame = app.buttons["viewerMore"].frame
        XCTAssertLessThanOrEqual(closeFrame.maxX, headingFrame.minX)
        XCTAssertLessThanOrEqual(headingFrame.maxX, moreFrame.minX)
        XCTAssertEqual(closeFrame.height, 44, accuracy: 1)
        XCTAssertEqual(headingFrame.height, closeFrame.height, accuracy: 1)
        XCTAssertEqual(moreFrame.height, closeFrame.height, accuracy: 1)
        let shareFrame = app.buttons["viewerShare"].frame
        let trashFrame = app.buttons["viewerTrash"].frame
        XCTAssertLessThan(app.buttons["viewerThumbnail.1"].frame.maxY, shareFrame.minY,
                          "The filmstrip must sit above the floating action row")
        XCTAssertEqual(shareFrame.height, 44, accuracy: 1)
        XCTAssertEqual(trashFrame.midY, shareFrame.midY, accuracy: 1)
        let viewerShot = XCTAttachment(screenshot: app.screenshot())
        viewerShot.name = "Photos-style floating viewer"; viewerShot.lifetime = .keepAlways; add(viewerShot)
        if !app.buttons["Information"].isHittable { app.tap() }
        app.buttons["Information"].tap()
        XCTAssertTrue(app.staticTexts["Media ID"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["Done"].isHittable)
        XCUIDevice.shared.orientation = .landscapeLeft
        XCTAssertTrue(app.staticTexts["Media ID"].waitForExistence(timeout: 5))
        XCUIDevice.shared.orientation = .portrait
        XCTAssertTrue(app.staticTexts["Media ID"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["Done"].isHittable)
        app.buttons["Done"].tap()
        let viewerFits = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            app.buttons["viewerClose"].frame.minX >= app.frame.minX && app.buttons["viewerMore"].frame.maxX <= app.frame.maxX
        }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [viewerFits], timeout: 5), .completed, app.debugDescription)
        if !app.buttons["Close"].isHittable { app.tap() }
        app.buttons["Close"].tap()
        navigate(app, to: "Map")
        let cluster = app.buttons["mapCluster.fixture"]
        XCTAssertTrue(cluster.waitForExistence(timeout: 15))
        XCTAssertEqual(cluster.label, "12 media")
        XCTAssertLessThanOrEqual(app.staticTexts.matching(identifier: "12").count, 1, "The map must not repeat the marker count as an annotation title")
        captureAdmin(app, "Map representative thumbnail and upper-right count")
        cluster.tap()
        XCTAssertTrue(app.navigationBars["12 moments"].waitForExistence(timeout: 5),
                      "The first cluster presentation must contain its loaded media")
        XCTAssertTrue(app.buttons["photo-1.jpg"].exists)
        app.buttons["Done"].tap()
        navigate(app, to: "Faces")
        let firstPerson = app.buttons["Select person 1"]
        XCTAssertTrue(firstPerson.waitForExistence(timeout: 5))
        firstPerson.tap()
        let faceCount = app.staticTexts["1 selected"]
        XCTAssertTrue(faceCount.waitForExistence(timeout: 5))
        XCTAssertEqual(faceCount.frame.midY, app.buttons["Library"].frame.midY, accuracy: 2)
        XCTAssertGreaterThan(faceCount.frame.minX, app.buttons["Library"].frame.maxX)
        XCTAssertFalse(app.buttons["Merge"].isEnabled)
        app.buttons["Select person 2"].tap()
        XCTAssertTrue(app.staticTexts["2 selected"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["Merge"].isEnabled)
        let dock = app.descendants(matching: .any).matching(identifier: "selectionActions").firstMatch
        XCTAssertEqual(app.buttons["Done"].frame.maxX + 12, dock.frame.maxX, accuracy: 2,
                       "The selection dock must align to the trailing edge")
        let selectionShot = XCTAttachment(screenshot: app.screenshot())
        selectionShot.name = "Faces single-circle selection and inline dock"; selectionShot.lifetime = .keepAlways; add(selectionShot)
        app.buttons["Done"].tap()
        XCTAssertFalse(app.staticTexts["2 selected"].exists)
        app.buttons["Library"].tap()
        let adminNavigation = app.descendants(matching: .any).matching(identifier: "libraryNavigation").firstMatch
        for _ in 0..<6 where !app.buttons["Expand Admin"].isHittable { adminNavigation.swipeUp() }
        app.buttons["Expand Admin"].tap()
        for _ in 0..<6 where !app.buttons["nav.User Management"].isHittable { adminNavigation.swipeUp() }
        app.buttons["nav.User Management"].tap()
        let usersTable = app.scrollViews["adminUsersTable"]
        let deactivate = app.buttons["Deactivate fixture-user"]
        XCTAssertTrue(deactivate.waitForExistence(timeout: 5))
        for _ in 0..<6 where !deactivate.isHittable { usersTable.swipeLeft() }
        deactivate.tap()
        XCTAssertTrue(app.buttons["Activate fixture-user"].waitForExistence(timeout: 5))
        let savedMetrics = try await metrics(origin)
        XCTAssertEqual(savedMetrics["/api/v1/user/update"], 1)
        navigate(app, to: "Import")
        XCTAssertTrue(app.staticTexts["WebDAV"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.staticTexts["Local Import"].exists)
        captureAdmin(app, "Import Android parity")
        navigate(app, to: "Metadata")
        XCTAssertTrue(app.buttons["Generate Metadata"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.staticTexts["WAITING FOR JOURNAL ROLLBACK (QUEUED)"].exists)
        captureAdmin(app, "Metadata Android parity")
        navigate(app, to: "AI")
        XCTAssertTrue(app.staticTexts["AI processing"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.scrollViews["adminAIStatusTable"].exists)
        let controls = app.scrollViews["adminAIControlsTable"]
        for _ in 0..<5 where !controls.exists || !controls.isHittable { app.swipeUp() }
        let minute = app.textFields["OCR Minute"]
        XCTAssertTrue(minute.waitForExistence(timeout: 5))
        for _ in 0..<4 where !minute.isHittable { app.swipeUp() }
        XCTAssertEqual(minute.value as? String, "0")
        minute.tap(); minute.typeKey("a", modifierFlags: .command); minute.typeText("5\n")
        XCTAssertEqual(minute.value as? String, "5", app.debugDescription)
        XCTAssertFalse(app.keyboards.firstMatch.exists)
        let saveSchedule = app.buttons["Save OCR cron schedule"]
        for _ in 0..<6 where !saveSchedule.isHittable { controls.swipeLeft() }
        XCTAssertTrue(saveSchedule.isEnabled, app.debugDescription)
        saveSchedule.tap()
        let scheduleMetrics = try await metrics(origin)
        XCTAssertEqual(scheduleMetrics["/api/v1/ai/schedule/update"], 1)
        captureAdmin(app, "AI controls Android parity")
        navigate(app, to: "Settings")
        XCTAssertTrue(app.buttons["Change password"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["Check for Updates"].exists)
        XCTAssertLessThan(app.buttons["Change password"].frame.maxY, app.buttons["signOut"].frame.minY)
        XCTAssertEqual(app.buttons["Change password"].frame.maxX, app.buttons["signOut"].frame.maxX, accuracy: 2)
        XCTAssertFalse(app.buttons["Open Photos permissions"].exists)
        XCTAssertTrue(app.staticTexts["No backup running"].exists)
        XCTAssertFalse(app.staticTexts["iOS decides when background work can run. Open Momento for long backups. Limited Photos access backs up only permitted items."].exists)
        XCTAssertFalse(app.staticTexts["Metadata and AI processing run separately on the server schedule."].exists)
        XCTAssertFalse(app.staticTexts.matching(NSPredicate(format: "label BEGINSWITH 'Earliest background run:'")).firstMatch.exists)
        captureAdmin(app, "Settings account actions")
        app.buttons["Change password"].tap()
        let submitPassword = app.buttons["submitPasswordChange"]
        XCTAssertTrue(app.secureTextFields["Current password"].waitForExistence(timeout: 5))
        XCTAssertFalse(submitPassword.isEnabled)
        app.secureTextFields["Current password"].tap()
        app.secureTextFields["Current password"].typeText("fixture-only")
        app.secureTextFields["New password"].tap()
        app.secureTextFields["New password"].typeText("fixture-replacement")
        XCTAssertFalse(submitPassword.isEnabled, "Mismatched confirmation must prevent submission")
        app.secureTextFields["Confirm password"].tap()
        app.secureTextFields["Confirm password"].typeText("fixture-replacement")
        XCTAssertTrue(submitPassword.isEnabled)
        submitPassword.tap()
        XCTAssertTrue(app.buttons["signOut"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.alerts["Momento"].exists)
        let passwordMetrics = try await metrics(origin)
        XCTAssertEqual(passwordMetrics["/api/v1/user/change-password"], 1)
        app.buttons["signOut"].tap()
        let confirmSignOut = app.buttons.matching(identifier: "confirmSignOut").firstMatch
        XCTAssertTrue(confirmSignOut.waitForExistence(timeout: 5))
        confirmSignOut.tap()
        XCTAssertTrue(app.textFields["server"].waitForExistence(timeout: 10))
        XCTAssertEqual(app.textFields["server"].value as? String, origin)
        app.textFields["server"].tap()
        app.textFields["server"].typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: origin.count))
        app.textFields["server"].typeText("https://another.example.invalid")
        XCTAssertEqual(app.textFields["server"].value as? String, "https://another.example.invalid")
    }

    @MainActor func testCSettingsRowsAndChoicesPreserveAccount() throws {
        let origin = try XCTUnwrap(ProcessInfo.processInfo.environment["MOMENTO_UI_TEST_ORIGIN"])
        XCTAssertTrue(origin.hasPrefix("http://127.0.0.1:"))
        let app = XCUIApplication()
        app.resetAuthorizationStatus(for: .photos)
        defer { app.terminate(); app.resetAuthorizationStatus(for: .photos) }
        app.launch()
        if app.textFields["server"].waitForExistence(timeout: 3) {
            let server = app.textFields["server"]
            server.tap()
            if let previous = server.value as? String, previous.hasPrefix("http") {
                server.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: previous.count))
            }
            server.typeText(origin)
            app.textFields["username"].tap(); app.textFields["username"].typeText("ios-test")
            app.secureTextFields["password"].tap(); app.secureTextFields["password"].typeText("fixture-only")
            signInButton(app).tap()
            XCTAssertTrue(app.buttons["Library"].waitForExistence(timeout: 15))
            if app.buttons["Not Now"].waitForExistence(timeout: 3) { app.buttons["Not Now"].tap() }
        }
        app.terminate(); app.launch()
        XCTAssertTrue(app.buttons["Library"].waitForExistence(timeout: 10))
        navigate(app, to: "Settings")
        let settingsScroll = app.scrollViews["settingsScrollView"]
        XCTAssertTrue(settingsScroll.waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["Change password"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.staticTexts[origin].exists, "Settings writes require the current disposable fixture session")
        XCTAssertEqual(app.buttons["backupRunAction"].label, "Back up now")
        XCTAssertFalse(app.buttons["Cancel"].exists)
        XCTAssertTrue(app.staticTexts["Local Cache Limit"].exists)
        let serverVersion = app.staticTexts["Server version fixture"]
        XCTAssertTrue(serverVersion.exists)
        XCTAssertGreaterThan(serverVersion.frame.minY, app.staticTexts[origin].frame.maxY)
        XCTAssertEqual(app.staticTexts.matching(NSPredicate(format: "label BEGINSWITH 'Momento iOS Client '")).count, 1)
        XCTAssertTrue(app.switches["Camera photos and videos only"].exists)
        XCTAssertFalse(app.buttons["Open Photos permissions"].exists)
        for title in ["Back up this device", "Backup diagnostics", "Backup history"] {
            let row = app.staticTexts[title]
            for _ in 0..<4 where !row.exists || !row.isHittable { settingsScroll.swipeUp() }
            XCTAssertTrue(row.exists)
        }
        let cache = app.buttons["mediaCacheSettings"]
        for _ in 0..<5 where !cache.isHittable { settingsScroll.swipeUp() }
        let backupDisplay = app.descendants(matching: .any).matching(identifier: "backupProgressContent").firstMatch
        XCTAssertTrue(backupDisplay.exists)
        XCTAssertEqual(backupDisplay.frame.minX, app.staticTexts["Backup diagnostics"].frame.minX, accuracy: 1)
        captureAdmin(app, "Settings backup and choices")
        let copy = app.buttons["copyBackupLogs"]
        let share = app.buttons["shareBackupLogs"]
        let reverify = app.buttons["reverifyBackupHistory"]
        let clear = app.buttons["clearBackupHistory"]
        XCTAssertEqual(copy.frame.midY, share.frame.midY, accuracy: 2)
        XCTAssertGreaterThan(share.frame.minX, copy.frame.minX)
        XCTAssertEqual(reverify.frame.midY, clear.frame.midY, accuracy: 2)
        XCTAssertGreaterThan(clear.frame.minX, reverify.frame.minX)
        XCTAssertEqual(app.staticTexts["Backup diagnostics"].frame.midY, copy.frame.midY, accuracy: 2)
        XCTAssertEqual(app.staticTexts["Backup history"].frame.midY, clear.frame.midY, accuracy: 2)
        XCTAssertFalse(app.staticTexts["Cached thumbnails and previews are available offline. Least recently used items are removed first."].exists)
        let clearCache = app.buttons["clearMediaCache"]
        XCTAssertTrue(clearCache.exists)
        XCTAssertEqual(clearCache.frame.midY, cache.frame.midY, accuracy: 2)
        XCTAssertLessThan(clearCache.frame.maxX, cache.frame.minX)
        let originalCache = cache.label
        clearCache.tap()
        XCTAssertEqual(cache.label, originalCache, "Clearing downloaded media must preserve the cache limit")
        cache.tap()
        XCTAssertFalse(app.buttons["Clear cache"].exists, "Clear belongs beside the chooser, outside its menu")
        XCTAssertFalse(originalCache.isEmpty)
        let choose256 = app.buttons.matching(NSPredicate(format: "label BEGINSWITH '256 MiB'")).firstMatch
        choose256.tap()
        XCTAssertTrue(cache.waitForExistence(timeout: 5))
        XCTAssertTrue(cache.label.contains("256 MiB"))
        cache.tap()
        app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", originalCache)).firstMatch.tap()
        let appearance = app.buttons["appearanceSettings"]
        XCTAssertTrue(appearance.exists)
        for _ in 0..<4 where !appearance.isHittable || appearance.frame.maxY > app.frame.maxY - 90 { settingsScroll.swipeUp() }
        captureAdmin(app, "Settings cache and appearance")
        let originalTheme = appearance.label.contains("Dark") ? "Dark" : appearance.label.contains("Light") ? "Light" : "Follow system"
        appearance.tap(); app.buttons["Dark"].tap()
        XCTAssertTrue(appearance.label.contains("Dark"))
        XCTAssertEqual(app.staticTexts["Appearance"].frame.midY, appearance.frame.midY, accuracy: 2)
        XCTAssertGreaterThan(appearance.frame.minX, app.staticTexts["Appearance"].frame.maxX)
        appearance.tap(); app.buttons[originalTheme].tap()
        XCUIDevice.shared.orientation = .landscapeLeft
        XCTAssertTrue(app.buttons["Library"].exists)
        let rotatedSettings = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in app.frame.width > app.frame.height }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [rotatedSettings], timeout: 10), .completed)
        XCTAssertEqual(backupDisplay.frame.minX, app.staticTexts["Backup diagnostics"].frame.minX, accuracy: 1)
        func visibleFrame(_ element: XCUIElement) -> Bool {
            let bounds = app.frame.insetBy(dx: 0, dy: 60)
            return element.exists && !element.frame.isEmpty && bounds.contains(element.frame)
        }
        // Empty history disables these controls; disabled buttons are never hittable.
        let historyTitle = app.staticTexts["Backup history"]
        let diagnosticsTitle = app.staticTexts["Backup diagnostics"]
        for _ in 0..<8 where !visibleFrame(historyTitle) || !visibleFrame(diagnosticsTitle) {
            if historyTitle.frame.maxY > app.frame.maxY - 60 { settingsScroll.swipeUp() }
            else { settingsScroll.swipeDown() }
        }
        let alignedRows = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            visibleFrame(reverify) && visibleFrame(clear) && visibleFrame(copy) && visibleFrame(share) &&
                abs(reverify.frame.midY - clear.frame.midY) <= 2 && abs(copy.frame.midY - share.frame.midY) <= 2
        }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [alignedRows], timeout: 5), .completed,
                       "Expanded rows must align when visible after rotation: \(app.debugDescription)")
        captureAdmin(app, "Settings expanded row layout")
        XCUIDevice.shared.orientation = .portrait
        let portraitSettings = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in app.frame.height > app.frame.width }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [portraitSettings], timeout: 10), .completed)
        // Device Hub can retain landscape hit points after reporting portrait bounds.
        // A restored session provides a fresh automation scene for the permission flow.
        app.terminate(); app.launch()
        XCTAssertTrue(app.buttons["Library"].waitForExistence(timeout: 10))
        navigate(app, to: "Settings")
        for _ in 0..<6 where !visibleFrame(app.buttons["signOut"]) || !visibleFrame(app.buttons["Change password"]) { settingsScroll.swipeDown() }
        XCTAssertTrue(app.buttons["Change password"].isHittable)
        XCTAssertLessThan(app.buttons["Change password"].frame.maxY, app.buttons["signOut"].frame.minY)
        XCTAssertEqual(app.buttons["Change password"].frame.maxX, app.buttons["signOut"].frame.maxX, accuracy: 2)
        XCTAssertTrue(app.staticTexts[origin].exists)
        let backupNow = app.buttons["Back up now"]
        for _ in 0..<5 where !backupNow.isHittable { settingsScroll.swipeUp() }
        backupNow.tap()
        XCTAssertEqual(app.buttons["backupRunAction"].label, "Cancel")
        XCTAssertFalse(app.buttons["Back up now"].exists)
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let deny = springboard.buttons.matching(NSPredicate(format: "label MATCHES[c] %@", "Don.t Allow")).firstMatch
        XCTAssertTrue(deny.waitForExistence(timeout: 30), springboard.debugDescription)
        deny.tap()
        XCTAssertTrue(app.buttons["Back up now"].waitForExistence(timeout: 5))
        let permissions = app.buttons["Open Photos permissions"]
        for _ in 0..<5 where !permissions.exists || !permissions.isHittable { settingsScroll.swipeDown() }
        XCTAssertTrue(permissions.waitForExistence(timeout: 5), app.debugDescription)
        captureAdmin(app, "Settings denied Photos access")
        for _ in 0..<6 where !app.buttons["signOut"].isHittable { settingsScroll.swipeDown() }
        app.buttons["signOut"].tap(); app.buttons.matching(identifier: "confirmSignOut").firstMatch.tap()
        XCTAssertTrue(app.textFields["server"].waitForExistence(timeout: 10))
    }

    @MainActor func testDMediaShowsImmediatelyAndVideoControlsPlaySeekAndMute() throws {
        let origin = try XCTUnwrap(ProcessInfo.processInfo.environment["MOMENTO_UI_TEST_ORIGIN"])
        let app = XCUIApplication(); app.launch()
        if app.textFields["server"].waitForExistence(timeout: 3) {
            let server = app.textFields["server"]; server.tap()
            if let previous = server.value as? String, previous.hasPrefix("http") { server.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: previous.count)) }
            server.typeText(origin)
            app.textFields["username"].tap(); app.textFields["username"].typeText("ios-test")
            app.secureTextFields["password"].tap(); app.secureTextFields["password"].typeText("fixture-only")
            signInButton(app).tap()
        }
        XCTAssertTrue(app.buttons["photo-1.jpg"].waitForExistence(timeout: 15))
        if app.buttons["Not Now"].waitForExistence(timeout: 2) { app.buttons["Not Now"].tap() }
        // Dismiss the system's separate password-save window before testing media touches.
        app.terminate(); app.launch()
        XCTAssertTrue(app.buttons["photo-5.jpg"].waitForExistence(timeout: 10))
        app.buttons["photo-5.jpg"].tap()
        XCTAssertTrue(app.buttons["viewerClose"].waitForExistence(timeout: 5), app.debugDescription)
        XCTAssertTrue(app.images["viewerImage"].waitForExistence(timeout: 10), "Photo must appear without a second tap: \(app.debugDescription)")
        XCTAssertGreaterThan(app.images["viewerImage"].frame.height, 100)
        app.buttons["viewerClose"].tap()
        navigate(app, to: "Videos")
        XCTAssertTrue(app.buttons["fragmented-preview.mp4"].waitForExistence(timeout: 10))
        app.buttons["fragmented-preview.mp4"].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap()
        let video = app.descendants(matching: .any).matching(identifier: "videoPlayer").firstMatch
        XCTAssertTrue(video.waitForExistence(timeout: 10))
        let ready = NSPredicate(format: "value == 'Ready'")
        expectation(for: ready, evaluatedWith: video); waitForExpectations(timeout: 15)
        XCTAssertTrue(app.buttons["Play video"].exists)
        XCTAssertFalse(app.buttons["Subtitles"].exists); XCTAssertFalse(app.buttons["Show Text"].exists)
        XCTAssertEqual(app.staticTexts["videoDuration"].label, "0:10")
        captureAdmin(app, "Video controls before first interaction")
        app.buttons["Mute video"].tap(); XCTAssertTrue(app.buttons["Unmute video"].waitForExistence(timeout: 5), app.debugDescription)
        app.buttons["Play video"].tap()
        let elapsed = app.staticTexts["videoElapsedTime"]
        expectation(for: NSPredicate(format: "label != '0:00'"), evaluatedWith: elapsed); waitForExpectations(timeout: 8)
        app.buttons["Pause video"].tap()
        app.sliders["videoSeekBar"].adjust(toNormalizedSliderPosition: 0.65)
        expectation(for: NSPredicate(format: "label IN %@", ["0:06", "0:07"]), evaluatedWith: elapsed); waitForExpectations(timeout: 5)
        app.buttons["Unmute video"].tap(); XCTAssertTrue(app.buttons["Mute video"].waitForExistence(timeout: 5), app.debugDescription)
        captureAdmin(app, "Video first frame and working playback controls")
        app.buttons["viewerClose"].tap()
    }

    @MainActor private func captureAdmin(_ app: XCUIApplication, _ name: String) {
        let shot = XCTAttachment(screenshot: app.screenshot()); shot.name = name; shot.lifetime = .keepAlways; add(shot)
    }

    @MainActor private func navigate(_ app: XCUIApplication, to destination: String) {
        if app.buttons["Library"].exists { app.buttons["Library"].tap() }
        let button = app.buttons["nav.\(destination)"]
        let navigation = app.descendants(matching: .any).matching(identifier: "libraryNavigation").firstMatch
        XCTAssertTrue(navigation.waitForExistence(timeout: 5))
        for _ in 0..<6 where !button.exists || !button.isHittable {
            if ["Timeline", "Photos", "Videos", "Screenshots", "Documents"].contains(destination) { navigation.swipeDown() } else { navigation.swipeUp() }
        }
        XCTAssertTrue(button.exists)
        button.tap()
    }

    private func metrics(_ origin: String) async throws -> [String: Int] {
        let (data, _) = try await URLSession.shared.data(from: URL(string: origin + "/metrics")!)
        return try JSONDecoder().decode([String: Int].self, from: data)
    }
}
