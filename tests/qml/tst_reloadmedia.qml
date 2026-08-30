import QtQuick
import QtTest
import "../../ryoku/shell/quickshell/reload-cover" as Reload

Item {
    id: root
    width: 640
    height: 360

    Component {
        id: mediaFactory
        Reload.ReloadMedia {
            width: 640
            height: 360
            active: true
        }
    }

    TestCase {
        name: "ReloadMedia"
        when: windowShown

        function makeMedia(descriptor) {
            const item = createTemporaryObject(mediaFactory, root, { descriptor: descriptor })
            verify(!!item, "Component exists")
            return item
        }

        function test_default_uses_bundled_wordmark() {
            const item = makeMedia({ path: "", name: "", kind: "default", bytes: 0 })
            compare(item.showingDefault, true)
            compare(item.customReady, false)
            compare(item.mediaError, false)
        }

        function test_valid_image_becomes_custom_media() {
            const source = Qt.resolvedUrl("../../ryoku/shell/quickshell/reload-cover/assets/logo.png")
            const item = makeMedia({ path: String(source), name: "logo.png", kind: "image", bytes: 1 })
            tryCompare(item, "customReady", true, 3000)
            compare(item.showingDefault, false)
        }

        function test_animated_image_becomes_custom_media() {
            const source = Qt.resolvedUrl("../../ryoku/hub/quickshell/art/tiling-dwindle.gif")
            const item = makeMedia({ path: String(source), name: "tiling-dwindle.gif", kind: "animated", bytes: 1 })
            tryCompare(item, "customReady", true, 3000)
            compare(item.showingDefault, false)
        }

        function test_missing_image_reports_error_and_keeps_default() {
            ignoreWarning(/QML AnimatedImage: Error Reading Animated Image File file:\/\/\/missing\/reload-cover\.png/)
            ignoreWarning(/QML AnimatedImage: Error Reading Animated Image File file:\/\/\/missing\/reload-cover\.png/)
            const item = makeMedia({ path: "/missing/reload-cover.png", name: "missing.png", kind: "image", bytes: 1 })
            tryCompare(item, "mediaError", true, 3000)
            compare(item.showingDefault, true)
        }

        function test_failure_phase_forces_default() {
            const source = Qt.resolvedUrl("../../ryoku/shell/quickshell/reload-cover/assets/logo.png")
            const item = makeMedia({ path: String(source), name: "logo.png", kind: "image", bytes: 1 })
            tryCompare(item, "customReady", true, 3000)
            item.forceDefault = true
            compare(item.showingDefault, true)
        }
    }
}
