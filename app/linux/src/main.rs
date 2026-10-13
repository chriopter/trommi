// (the app's library holds the bridge and the QML module: linked by naming it)
extern crate trommi_linux;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};

fn main() {
    // the crate's QML module and its resources, which a binary links only when asked
    cxx_qt::init_crate!(trommi_linux);
    let mut app = QGuiApplication::new();
    if let Some(app) = app.as_mut() {
        app.set_application_name(&QString::from("Trommi"));
    }
    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/com/trommi/linux/qml/main.qml"));
    }
    if let Some(app) = app.as_mut() {
        app.exec();
    }
}
