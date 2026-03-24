use eframe::egui;
use ico::IconDir;

pub fn load_app_icon() -> egui::IconData {
    let icon_bytes = include_bytes!("../assets/icon.ico");
    let mut cursor = std::io::Cursor::new(icon_bytes.as_slice());
    let icon_dir = IconDir::read(&mut cursor).expect("failed to read assets/icon.ico");

    let best_entry = icon_dir
        .entries()
        .iter()
        .max_by_key(|entry| entry.width() * entry.height())
        .expect("assets/icon.ico does not contain any icon entries");

    let image = best_entry
        .decode()
        .expect("failed to decode icon image from assets/icon.ico");

    egui::IconData {
        rgba: image.rgba_data().to_vec(),
        width: image.width(),
        height: image.height(),
    }
}
