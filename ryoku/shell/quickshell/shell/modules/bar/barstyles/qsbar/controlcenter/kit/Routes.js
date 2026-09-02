.pragma library

// QS Bar Settings routes, in rail order. One entry per page: its Latin name, its
// kanji seal (a real word, per the desktop's language, never decoration), the
// one-line summary the head prints, and the words search matches on.
//
// The panel is about one thing, the bar, so the rail is one flat group of four:
// where the bar sits (Bar), how its widgets are arranged (Layout), what each
// widget is and does (Widgets), and the dock that rides beside it (Dock). What
// this panel used to also carry -- logo, spaces, pickers, desktop widgets, the
// mid-work switches, the session -- already has a home (folded into Widgets,
// moved to the Hub, or reached from quick settings), so it left.
var ROUTES = [
    { id: "bars",    label: "Bar",     gloss: "\u5e2f", file: "BarsRoute",
      desc: "Where the bar sits, the shape it takes, and how its surface reads.",
      keywords: "position top bottom form full fit dock notch islands surface border corners frost shadow depth tooltip gap gaps accent colour scale size motion animation auto hide" },
    { id: "layout",  label: "Layout",  gloss: "\u914d\u7f6e", file: "LayoutRoute",
      desc: "Arrange the widgets across the bar's left, centre and right lanes.",
      keywords: "arrange order move reorder left center centre right lane add widget plugin unlock drag reset layout hide show" },
    { id: "widgets", label: "Widgets", gloss: "\u90e8\u54c1", file: "WidgetsRoute",
      desc: "Every widget: show it, size it, colour it, and tune what it says.",
      keywords: "widget show hide on off density icon compact colour fill launcher mark wordmark glyph workspaces marker ai claude codex opencode volume boost clock weather sensor temperature plugin settings" },
    { id: "dock",    label: "Dock",    gloss: "\u53f0", file: "DockRoute",
      desc: "The app dock on the edge opposite the bar.",
      keywords: "dock app pinned pin edge autohide auto hide magnify frost depth shadow label media chip peek" }
];

function byId(id) {
    for (var i = 0; i < ROUTES.length; i++)
        if (ROUTES[i].id === id) return ROUTES[i];
    return null;
}

function labelFor(id) {
    var r = byId(id);
    return r ? r.label : id;
}

function fileFor(id) {
    var r = byId(id);
    return r ? r.file : "";
}

function indexOf(id) {
    for (var i = 0; i < ROUTES.length; i++)
        if (ROUTES[i].id === id) return i;
    return -1;
}

// Retired route ids, mapped to their nearest new home, so an old caller (a
// keybind, a saved link, `bar settings <route>`) never lands on nothing:
//   logo, spaces      -> the launcher's / workspaces' own settings, in Widgets
//   widgets, appearance -> the Widgets route (renamed from the old Appearance)
//   pickers           -> the Hub's Desktop page owns picker style now; nearest here is Widgets
//   desktop           -> the Hub owns desktop widgets now; nearest here is Widgets
//   system, session   -> quick settings (Super+Escape); nearest here is Bar
function resolve(id) {
    if (byId(id)) return id;
    switch (id) {
    case "logo":
    case "spaces":
    case "appearance":
    case "pickers":
    case "desktop":
        return "widgets";
    case "system":
    case "session":
        return "bars";
    default:
        return "bars";
    }
}
