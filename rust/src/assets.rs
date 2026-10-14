//! Imagens e textos do app oficial (ui/assets, extraídos por extract_assets.py).
//! As texturas carregam sob demanda e ficam em cache.

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// $APPDIR/usr/share/u2tool/assets (AppImage) → assets/ ao lado do binário → ui/assets (gerado por extract_assets.py)
pub fn find_dir() -> Option<PathBuf> {
    let mut c = Vec::new();
    if let Ok(a) = std::env::var("APPDIR") {
        c.push(PathBuf::from(a).join("usr/share/u2tool/assets"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            c.push(d.join("assets"));
            c.push(d.join("../share/u2tool/assets"));
            c.push(d.join("../../../ui/assets")); // rust/target/release → ui/assets
        }
    }
    c.push(PathBuf::from("ui/assets"));
    c.push(PathBuf::from("../ui/assets"));
    c.into_iter().find(|p| p.join("resource").is_dir())
}

pub struct Assets {
    dir: PathBuf,
    tex: HashMap<String, Option<TextureHandle>>,
    strings: HashMap<String, String>,
}

impl Assets {
    pub fn new(dir: PathBuf) -> Self {
        let strings = std::fs::read_to_string(dir.join("strings.json")).ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.as_object().cloned())
            .map(|o| o.into_iter().filter_map(|(k, v)| Some((k, v.get("en")?.as_str()?.replace("\\r\\n", "\n").replace("\r\n", "\n")
                .replace("\\\"", "\"")))).collect())
            .unwrap_or_default();
        Assets { dir, tex: HashMap::new(), strings }
    }

    /// Texto em inglês do LanguageTools (o app só tem inglês e chinês)
    pub fn s(&self, k: &str) -> String {
        self.strings.get(k).cloned().unwrap_or_else(|| k.to_string())
    }

    /// Caminho relativo a resource/, em minúsculas
    pub fn tex(&mut self, ctx: &egui::Context, rel: &str) -> Option<TextureHandle> {
        let key = rel.to_lowercase();
        if let Some(t) = self.tex.get(&key) {
            return t.clone();
        }
        let t = load(&self.dir.join("resource").join(&key)).map(|img| ctx.load_texture(&key, img, TextureOptions::LINEAR));
        self.tex.insert(key, t.clone());
        t
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }
}

fn load(p: &Path) -> Option<ColorImage> {
    let img = image::open(p).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}
