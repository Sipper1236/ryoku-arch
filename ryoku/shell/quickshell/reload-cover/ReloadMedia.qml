import QtQuick

Item {
    id: root

    property var descriptor: ({ path: "", name: "", kind: "default", bytes: 0 })
    property bool active: true
    property bool forceDefault: false

    readonly property string mediaPath: descriptor && typeof descriptor.path === "string" ? descriptor.path : ""
    readonly property string mediaKind: descriptor && typeof descriptor.kind === "string" ? descriptor.kind : "default"
    readonly property bool wantsVideo: mediaPath !== "" && mediaKind === "video"
    readonly property bool wantsImage: mediaPath !== "" && (mediaKind === "image" || mediaKind === "animated")
    readonly property bool imageReady: wantsImage && customImage.status === Image.Ready
    readonly property bool videoReady: videoLoader.status === Loader.Ready && videoLoader.item && videoLoader.item.ready
    readonly property bool customReady: !forceDefault && (imageReady || videoReady)
    readonly property bool mediaError: wantsImage
        ? customImage.status === Image.Error
        : (wantsVideo && (videoLoader.status === Loader.Error
            || (videoLoader.status === Loader.Ready && videoLoader.item && videoLoader.item.failed)))
    readonly property string mediaErrorText: wantsVideo && videoLoader.status === Loader.Ready && videoLoader.item
        ? videoLoader.item.errorText
        : ""
    readonly property bool showingDefault: forceDefault || !customReady
    readonly property real defaultLogoWidth: Math.min(width * 0.58, 928)
    readonly property real defaultLogoHeight: defaultLogoWidth * 160 / 928

    Image {
        anchors.centerIn: parent
        visible: root.showingDefault
        source: "assets/logo.png"
        sourceSize.width: Math.round(root.defaultLogoWidth)
        fillMode: Image.PreserveAspectFit
        width: root.defaultLogoWidth
        height: root.defaultLogoHeight
    }

    AnimatedImage {
        id: customImage
        objectName: "customImage"
        anchors.fill: parent
        visible: root.imageReady && !root.forceDefault
        source: root.wantsImage ? (root.mediaPath.indexOf("://") >= 0 ? root.mediaPath : "file://" + root.mediaPath) : ""
        sourceSize.width: Math.max(1, Math.ceil(width))
        fillMode: Image.PreserveAspectFit
        asynchronous: true
        cache: false
        playing: root.active && visible && status === Image.Ready
    }

    Loader {
        id: videoLoader
        anchors.fill: parent
        active: root.active && root.wantsVideo && !root.forceDefault
        source: active ? Qt.resolvedUrl("ReloadVideo.qml") : ""
        onLoaded: {
            if (status === Loader.Ready && item) {
                item.path = Qt.binding(() => root.mediaPath)
                item.active = Qt.binding(() => root.active && !root.forceDefault)
            }
        }
    }
}
