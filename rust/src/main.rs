//! 8BitDo Ultimate Software V2 para Linux (Ultimate 2) — janela egui.
//!   u2tool                      — abre a interface (grava no controle)
//!   u2tool --somente-leitura    — não grava nada
//!   u2tool --offline ARQ.bin    — edita um backup em vez do controle
//!   u2tool --shot ABA saida.png — abre na aba, captura a janela e sai (depuração)

use u2tool::{app, assets, proto};

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k);
    let offline = arg("--offline").and_then(|i| args.get(i + 1).cloned());
    let write = arg("--somente-leitura").is_none();
    let shot = arg("--shot").and_then(|i| Some((args.get(i + 1)?.clone(), args.get(i + 2)?.clone())));

    let Some(dir) = assets::find_dir() else {
        eprintln!("imagens do app não encontradas (ui/assets/resource) — rode extract_assets.py");
        std::process::exit(1);
    };
    // mesma trava do server.py: só um programa fala com o controle
    let _lock = if offline.is_none() {
        match proto::single_instance() {
            Some(f) => Some(f),
            None => {
                eprintln!("o configurador já está aberto (trava em $XDG_RUNTIME_DIR/u2tool.lock)");
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    let mut vp = eframe::egui::ViewportBuilder::default()
        .with_title("8BitDo Ultimate Software V2")
        .with_app_id("u2tool")
        .with_inner_size([1080.0, 780.0])
        .with_min_inner_size([540.0, 390.0]);
    if let Ok(img) = image::open(dir.join("logo_white.png")) {
        let img = img.to_rgba8();
        vp = vp.with_icon(eframe::egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() });
    }
    let opts = eframe::NativeOptions { viewport: vp, vsync: true, ..Default::default() };
    eframe::run_native("u2tool", opts, Box::new(move |cc| {
        Ok(Box::new(app::App::new(cc, assets::Assets::new(dir), app::Opts { offline, write, shot })))
    }))
}
