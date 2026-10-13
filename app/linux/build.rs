use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("com.trommi.linux").qml_files([
        "qml/main.qml",
        "qml/SignIn.qml",
        "qml/Desk.qml",
        "qml/CardView.qml",
        "qml/Sidebar.qml",
        "qml/Chat.qml",
    ]))
    .qt_module("Quick")
    .qt_module("QuickControls2")
    .files(["src/bridge.rs"])
    .build();
    // cxx-qt-build asks for gold, which drops the Rust symbols its initializers call (undefined
    // `cxx_qt_init_*`); lld links them. The later -fuse-ld wins.
    println!("cargo::rustc-link-arg=-fuse-ld=lld");
}
