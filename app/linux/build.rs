use cxx_qt_build::{CxxQtBuilder, QmlModule};
use std::path::PathBuf;

fn main() {
    // The web app's faces (app/web/public/fonts, OFL), read from there: one copy of the fonts in the repository. A
    // resource file in OUT_DIR names each under fonts/ in the QML module. The latin files (fonts.css): Bricolage
    // Grotesque, IBM Plex Mono 400 and 500, IBM Plex Sans.
    let here = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("the crate's folder"));
    let fonts = here.join("../web/public/fonts").canonicalize().expect("app/web/public/fonts");
    let mut qrc = String::from("<RCC><qresource prefix=\"/qt/qml/com/trommi/linux\">\n");
    for n in [1, 3, 5, 7] {
        let file = fonts.join(format!("f{n}.woff2"));
        println!("cargo::rerun-if-changed={}", file.display());
        qrc.push_str(&format!("<file alias=\"fonts/f{n}.woff2\">{}</file>\n", file.display()));
    }
    qrc.push_str("</qresource></RCC>\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("fonts.qrc");
    std::fs::write(&out, qrc).expect("fonts.qrc");

    CxxQtBuilder::new_qml_module(QmlModule::new("com.trommi.linux").qml_files([
        "qml/main.qml",
        "qml/Theme.qml",
        "qml/PenMark.qml",
        "qml/SignIn.qml",
        "qml/Sidebar.qml",
        "qml/Desk.qml",
        "qml/DeskRow.qml",
        "qml/CardView.qml",
        "qml/Chat.qml",
        "qml/Attachment.qml",
        "qml/pen.mjs",
    ]))
    .qt_module("Quick")
    .qt_module("QuickControls2")
    .qrc(&out)
    .files(["src/bridge.rs"])
    .build();
    // cxx-qt-build asks for gold, which drops the Rust symbols its initializers call (undefined
    // `cxx_qt_init_*`); lld links them. The later -fuse-ld wins.
    println!("cargo::rustc-link-arg=-fuse-ld=lld");
}
