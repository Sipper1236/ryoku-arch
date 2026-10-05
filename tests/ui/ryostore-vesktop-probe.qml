import QtQuick
import Quickshell
import "ryostore" as Ryo
import "ryostore/Singletons" as State

ShellRoot {
    id: root

    property int phase: 0
    property int attempts: 0

    function check(ok, msg) {
        if (!ok)
            throw new Error("VESKTOP-UI-FAIL " + msg);

    }

    function find(o, n) {
        if (o.objectName === n)
            return o;

        for (const c of o.children || []) {
            const x = find(c, n);
            if (x)
                return x;

        }
        return null;
    }

    FloatingWindow {
        implicitWidth: 1180
        implicitHeight: 760

        Ryo.App {
            id: app

            anchors.fill: parent
        }

    }

    Timer {
        interval: 50
        running: true
        repeat: true
        onTriggered: {
            root.attempts++;
            if (root.attempts > 300)
                throw new Error("VESKTOP-UI-FAIL timeout phase " + root.phase);

            if (State.Store.items.length !== 6)
                return ;

            if (root.phase === 1) {
                const installed = State.Store.items.filter((i) => {
                    return i.category === "vesktop-themes" && ["discord", "second"].indexOf(i.id) >= 0;
                });
                if (installed.some((i) => {
                    return !i.installed;
                }) || State.Store.busyKey !== "")
                    return ;

                root.check(installed.every((i) => {
                    return i.active === false && i.enabled === false;
                }), "install does not activate");
                app.selectKey("vesktop-themes:discord");
                app.openSelectedDetail();
                root.phase = 2;
                return ;
            }
            if (root.phase === 2) {
                const guide = root.find(app, "ryostore-detail-vesktop-guidance");
                if (!guide || !guide.visible)
                    return ;

                root.check(guide.text.indexOf("Installed.") === 0 && guide.text.indexOf("Vesktop Settings > Vencord > Themes") >= 0, "installed guidance refreshed");
                app.closeDetail();
                app.openRoute("colorschemes");
                app.providerFilter = "Shared";
                root.check(app.providerItems("Shared").length === 1 && app.providerItems("Shared")[0].id === "scheme", "desktop category independent");
                console.log("VESKTOP-UI-PASS");
                State.Store.shutdown();
                Qt.quit();
                return ;
            }
            root.check(app.navigationCategories.some((c) => {
                return c.id === "vesktop-themes";
            }), "navigation");
            app.openRoute("vesktop-themes");
            app.providerFilter = "Shared";
            root.check(app.themesBrowse, "provider strip visible");
            root.check(app.themeInstallable === 2, "bulk count category scoped");
            root.check(app.providerItems("Shared").length === 5, "bulk queue category scoped");
            root.check(app.collection.length === 5 && app.collection.every((i) => {
                return i.category === "vesktop-themes";
            }), "provider collection");
            app.providerFilter = "__mine__";
            root.check(app.collection.length === 1 && app.collection[0].id === "old", "installed themes scoped");
            app.selectKey("vesktop-themes:old");
            app.openSelectedDetail();
            const guide = root.find(app, "ryostore-detail-vesktop-guidance");
            root.check(guide && guide.visible && guide.text.indexOf("Installed.") === 0 && guide.text.indexOf("Vesktop Settings > Vencord > Themes") >= 0, "success guidance");
            app.closeDetail();
            app.providerFilter = "Shared";
            const tabs = root.find(app, "ryostore-provider-tabs");
            root.check(tabs && tabs.visible, "bulk provider controls visible");
            tabs.installAll();
            root.phase = 1;
        }
    }

}
