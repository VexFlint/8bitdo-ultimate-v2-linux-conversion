//! Interface em egui com a mesma disposição do app oficial para Windows.
//! Tudo é desenhado num quadro lógico de 1080×780 (a janela WPF original), escalado para a janela.

use crate::assets::Assets;
use crate::config::{self, Config, Sixaxis, feat, off};
use crate::device::{self, Cmd, Event, Handle, Info};
use crate::profiles::{self, FileProfile, MacroSteps};
use crate::proto::Keys;
use eframe::egui::{self, Align, Align2, Color32, CursorIcon, FontId, Id, Pos2, Rect, Rounding, Sense, Stroke, Ui, Vec2, pos2, vec2};
use std::collections::HashMap;
use std::f32::consts::PI;
use std::time::{Duration, Instant};

mod macro_ui;

// ------------------------------------------------------------------ cores

const fn c(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}
fn ca(hex: u32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, (a * 255.0).round() as u8)
}
fn white(a: f32) -> Color32 {
    ca(0xFFFFFF, a)
}
const BG: Color32 = c(0x171717);
const TEXT: Color32 = c(0xBBBBBB);
const MUTED: Color32 = c(0x9A9A9A);
const ACCENT: Color32 = c(0x80D5FF);
const BLUE: Color32 = c(0x296785);

// ------------------------------------------------------------------ quadro com escala

/// Origem (em pixels da tela) de um bloco posicionado, escala e recorte.
#[derive(Clone, Copy)]
struct Fr {
    o: Pos2,
    k: f32,
    clip: Rect,
}
impl Fr {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.o.x + x * self.k, self.o.y + y * self.k)
    }
    fn r(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(self.p(x, y), vec2(w * self.k, h * self.k))
    }
    fn at(&self, x: f32, y: f32) -> Fr {
        Fr { o: self.p(x, y), ..*self }
    }
    fn boxed(&self, x: f32, y: f32, w: f32, h: f32) -> Fr {
        Fr { o: self.p(x, y), k: self.k, clip: self.clip.intersect(self.r(x, y, w, h)) }
    }
    fn s(&self, v: f32) -> f32 {
        v * self.k
    }
    fn local(&self, p: Pos2) -> (f32, f32) {
        ((p.x - self.o.x) / self.k, (p.y - self.o.y) / self.k)
    }
}

/// Resultado de uma área clicável (zerado quando há um diálogo por cima)
#[derive(Default, Clone, Copy)]
struct H {
    hov: bool,
    click: bool,
    drag: bool,
    stop: bool,
    pos: Option<(f32, f32)>,
}

// ------------------------------------------------------------------ estado

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Loading,
    Search,
    Home,
    Profiles,
    Settings,
    /// gravação de macro: gravação/edição de uma macro (ocupa a página inteira)
    MacroEdit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Mapping,
    Macro,
    Stick,
    Trigger,
    Vibration,
    Led,
    Axis,
}
impl Tab {
    fn key(self) -> &'static str {
        match self {
            Tab::Mapping => "mapping",
            Tab::Macro => "macro",
            Tab::Stick => "stick",
            Tab::Trigger => "trigger",
            Tab::Vibration => "vibration",
            Tab::Led => "led",
            Tab::Axis => "axis",
        }
    }
    pub fn from_key(s: &str) -> Option<Tab> {
        [Tab::Mapping, Tab::Macro, Tab::Stick, Tab::Trigger, Tab::Vibration, Tab::Led, Tab::Axis].into_iter().find(|t| t.key() == s)
    }
}

struct FileEdit {
    id: String,
    name: String,
    cfg: Config,
    macros: MacroSteps,
}

/// slot 0-2 = perfil do controle; File = perfil da lista local (S_Setting == -1)
enum Slot {
    Dev(usize),
    File(FileEdit),
}

#[derive(Clone)]
enum Act {
    DeleteSlot(usize),
    MoveSlot(usize, usize),
    LoadFile(usize, usize),
    DeleteFile(String),
    /// MacroView.DeleteFile: apaga a macro da biblioteca e desliga onde estiver ativa
    DeleteMacro(String),
}

#[derive(Clone)]
enum RenameAct {
    Slot(usize),
    File(usize),
    Create,
    /// MacroRenameDialog: nova macro, renomear ou copiar uma da biblioteca
    MacroCreate,
    MacroRename(String),
    MacroCopy(String),
}

#[derive(Clone, Copy)]
enum ChoiceAct {
    LedMode,
    LedColorType,
    AxisMapping,
    AxisTrigger,
}

enum Modal {
    SlotMenu(usize),
    FileMenu(usize, f32),
    Choice { x: f32, y: f32, items: Vec<(u8, String)>, cur: u8, act: ChoiceAct },
    Tisp { msg: String, ok: Option<Act> },
    OpenTisp(usize),
    Rename { text: String, act: RenameAct, focus: bool },
    MapPick(&'static str),
    AxisPick,
    /// menu da engrenagem de uma macro da biblioteca (Macro_File); `y` na raiz da aba
    MacroMenu { file: usize, name: String, y: f32, active: bool },
}
impl Modal {
    fn is_popup(&self) -> bool {
        matches!(self, Modal::SlotMenu(_) | Modal::FileMenu(..) | Modal::Choice { .. } | Modal::MacroMenu { .. })
    }
}

/// LedView: estado da aba RGB (igual ao objeto `led` do app.js)
#[derive(Default)]
struct Led {
    custom: Option<bool>,
    mode: u8,
    color_type: u8,
    keys: Vec<usize>,
    sync: bool,
    bright: Option<f32>,
}

/// Estado da roda de cor (ColorInformation)
#[derive(Default)]
struct Picker {
    hsv: (f32, f32, f32),
    color: Option<u32>,
    hex: String,
    rgb: [String; 3],
}

pub struct Opts {
    pub offline: Option<String>,
    pub write: bool,
    pub shot: Option<(String, String)>,
}

pub struct App {
    dev: Handle,
    assets: Assets,
    write: bool,
    offline: bool,
    info: Option<Info>,
    cfg: Option<Config>,
    /// o que o controle tem gravado (base do diff, como o self.base do server.py)
    base: Vec<u8>,
    files: Vec<FileProfile>,
    screen: Screen,
    slot: Slot,
    tab: Tab,
    back: bool,
    modal: Option<Modal>,
    modal_frame: u64,
    frame: u64,
    blocked: bool,
    status: String,
    warned: bool,
    keys: Keys,
    live: Option<(bool, Option<(u8, u8)>)>,
    last_load: Instant,
    scroll: HashMap<&'static str, f32>,
    drag_val: HashMap<&'static str, (u8, u8)>,
    led: Led,
    picker: Picker,
    vib_since: [Option<Instant>; 2],
    /// passos das macros dos 3 perfis do controle (o struct só guarda nome, tecla e contagem)
    steps: device::Steps,
    /// os passos foram lidos? (se não, mover/carregar perfil zera as macros do destino)
    macros_ok: bool,
    mac: macro_ui::MacroTab,
    editor: Option<macro_ui::Editor>,
    shot: Option<(String, String)>,
    shot_frames: u32,
}

// ------------------------------------------------------------------ tabelas do app.js

/// CustomizePosition.getUltimate2(0, -155): pontos da linha, ponto do botão, tipo, lado
type MapRow = (&'static [(f32, f32)], (f32, f32), &'static str, char);
const U2_MAP: &[MapRow] = &[
    (&[(480., 270.), (470., 260.), (250., 260.)], (250., 260.), "SELECT", 'L'),
    (&[(476., 360.), (385., 360.), (250., 360.)], (250., 360.), "Up", 'L'),
    (&[(445., 393.), (428., 410.), (250., 410.)], (250., 410.), "Left", 'L'),
    (&[(500., 393.), (476., 393.), (409., 460.), (250., 460.)], (250., 460.), "Right", 'L'),
    (&[(476., 420.), (386., 510.), (250., 510.)], (250., 510.), "Down", 'L'),
    (&[(405., 310.), (355., 310.), (250., 310.)], (250., 310.), "L3", 'L'),
    (&[(605., 270.), (615., 260.), (830., 260.)], (830., 260.), "START", 'R'),
    (&[(688., 282.), (715., 282.), (743., 310.), (830., 310.)], (830., 310.), "Y", 'R'),
    (&[(725., 320.), (765., 360.), (830., 360.)], (830., 360.), "B", 'R'),
    (&[(655., 320.), (675., 320.), (765., 410.), (830., 410.)], (830., 410.), "X", 'R'),
    (&[(688., 350.), (798., 460.), (830., 460.)], (830., 460.), "A", 'R'),
    (&[(608., 400.), (718., 510.), (830., 510.)], (830., 510.), "R3", 'R'),
    (&[(440., 240.), (350., 330.), (250., 330.)], (250., 330.), "AS_P4", 'L'),
    (&[(380., 230.), (250., 230.)], (250., 230.), "R", 'L'),
    (&[(380., 260.), (360., 280.), (250., 280.)], (250., 280.), "R2", 'L'),
    (&[(420., 380.), (250., 380.)], (250., 380.), "AS_P1", 'L'),
    (&[(635., 240.), (725., 330.), (830., 330.)], (830., 330.), "AS_P3", 'R'),
    (&[(695., 230.), (830., 230.)], (830., 230.), "L", 'R'),
    (&[(695., 260.), (715., 280.), (830., 280.)], (830., 280.), "L2", 'R'),
    (&[(660., 380.), (830., 380.)], (830., 380.), "AS_P2", 'R'),
];
const BACK_TYPES: [&str; 8] = ["AS_P1", "AS_P2", "AS_P3", "AS_P4", "L", "R", "L2", "R2"];

/// MappingImageEngineer.getMapKeyString (ramo Ultimate 2)
fn key_name(k: &str) -> &str {
    match k {
        "L" => "L1",
        "R" => "R1",
        "START" => "Menu",
        "SELECT" => "View",
        "switchHome" => "Home",
        "turbo" => "Share",
        "screenshot" => "Screenshot",
        "LS_Up" => "LSUP",
        "LS_Down" => "LSDOWN",
        "LS_Left" => "LSLEFT",
        "LS_Right" => "LSRIGHT",
        "RS_Up" => "RSUP",
        "RS_Down" => "RSDOWN",
        "RS_Left" => "RSLEFT",
        "RS_Right" => "RSRIGHT",
        "N" | "AS_P1" | "AS_P2" | "AS_P3" | "AS_P4" => "Null",
        _ => k,
    }
}

const PICK_U2: [&str; 18] = ["A", "B", "X", "Y", "L", "L2", "L3", "R", "R2", "R3", "SELECT", "START", "switchHome",
    "Up", "Down", "Left", "Right", "N"];

const PRESETS: [u32; 7] = [0xFF0000, 0x8B00FF, 0x0000FF, 0x007FFF, 0x00FF00, 0xFFFF00, 0xFFA500];

/// KeyMap → nome da imagem (AxisView.getKeyMapString)
const AXIS_KEYS: [(u32, &str); 18] = [(0x2000, "A"), (0x1000, "B"), (0x10, "X"), (0x20, "Y"), (0x200, "Up"),
    (0x100, "Down"), (0x80, "Left"), (0x40, "Right"), (0x400, "L1"), (0x800, "R1"), (0x4000, "L2"), (0x8000, "R2"),
    (0x2, "L3"), (0x4, "R3"), (0x2000000, "PR"), (0x4000000, "PL"), (0x200000, "L4"), (0x40000000, "R4")];
fn axis_name(k: u32) -> &'static str {
    AXIS_KEYS.iter().find(|a| a.0 == k).map_or("A", |a| a.1)
}

fn rgb_to_hsv(c: u32) -> (f32, f32, f32) {
    let r = ((c >> 16) & 255) as f32 / 255.0;
    let g = ((c >> 8) & 255) as f32 / 255.0;
    let b = (c & 255) as f32 / 255.0;
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let d = mx - mn;
    let mut h = 0.0;
    if d > 0.0 {
        h = if mx == r { ((g - b) / d) % 6.0 } else if mx == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
    }
    ((h * 60.0 + 360.0) % 360.0, if mx > 0.0 { d / mx } else { 0.0 }, mx)
}
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> u32 {
    let f = |n: f32| {
        let k = (n + h / 60.0) % 6.0;
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    ((f(5.0) * 255.0).round() as u32) << 16 | ((f(3.0) * 255.0).round() as u32) << 8 | (f(1.0) * 255.0).round() as u32
}

/// pontos de um arco curto entre dois pontos do círculo (centro, raio)
fn arc_pts(cx: f32, cy: f32, r: f32, a: (f32, f32), b: (f32, f32)) -> Vec<(f32, f32)> {
    let t1 = (a.1 - cy).atan2(a.0 - cx);
    let t2 = (b.1 - cy).atan2(b.0 - cx);
    let mut d = t2 - t1;
    while d > PI {
        d -= 2.0 * PI;
    }
    while d < -PI {
        d += 2.0 * PI;
    }
    let n = ((d.abs() * r / 3.0).ceil() as usize).max(2);
    (0..=n).map(|i| {
        let t = t1 + d * i as f32 / n as f32;
        (cx + r * t.cos(), cy + r * t.sin())
    }).collect()
}

// ------------------------------------------------------------------ App

impl App {
    pub fn new(cc: &eframe::CreationContext, assets: Assets, o: Opts) -> Self {
        setup_fonts(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        let offline = o.offline.is_some();
        let dev = device::spawn(o.offline, o.write, move || ctx.request_repaint());
        let _ = dev.tx.send(Cmd::Load);
        App {
            dev,
            assets,
            write: o.write,
            offline,
            info: None,
            cfg: None,
            base: Vec::new(),
            files: profiles::list(),
            screen: Screen::Loading,
            slot: Slot::Dev(0),
            tab: Tab::Mapping,
            back: false,
            modal: None,
            modal_frame: 0,
            frame: 0,
            blocked: false,
            status: "lendo o controle…".into(),
            warned: false,
            keys: Keys { lx: 128, ly: 128, rx: 128, ry: 128, ..Default::default() },
            live: None,
            last_load: Instant::now(),
            scroll: HashMap::new(),
            drag_val: HashMap::new(),
            led: Led::default(),
            picker: Picker::default(),
            vib_since: [None, None],
            steps: Default::default(),
            macros_ok: true,
            mac: Default::default(),
            editor: None,
            shot: o.shot,
            shot_frames: 0,
        }
    }

    fn s(&self, k: &str) -> String {
        self.assets.s(k)
    }

    fn color_suffix(&self) -> &'static str {
        match self.info.as_ref().map_or(0, |i| i.color) {
            2 => "_MM",
            3 => "_XT",
            4 => "_LT",
            _ => "",
        }
    }
    fn variant(&self) -> u8 {
        self.info.as_ref().map_or(1, |i| i.variant)
    }
    fn is_file(&self) -> bool {
        matches!(self.slot, Slot::File(_))
    }
    fn dev_slot(&self) -> Option<usize> {
        match self.slot {
            Slot::Dev(i) => Some(i),
            _ => None,
        }
    }
    /// (cópia da configuração em edição, índice do perfil dentro dela)
    fn cur(&self) -> (Config, usize) {
        match &self.slot {
            Slot::File(f) => (f.cfg.clone(), 0),
            Slot::Dev(i) => (self.cfg.clone().unwrap_or_else(|| Config::new(vec![0; crate::proto::CFG_SIZE])), *i),
        }
    }
    fn cur_mut(&mut self) -> (&mut Config, usize) {
        match &mut self.slot {
            Slot::File(f) => (&mut f.cfg, 0),
            Slot::Dev(i) => (self.cfg.as_mut().expect("config"), *i),
        }
    }
    fn open_modal(&mut self, m: Modal) {
        self.modal = Some(m);
        self.modal_frame = self.frame;
    }

    // ---------------------------------------------------------------- backend

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(e) = self.dev.rx.try_recv() {
            match e {
                Event::Loaded(info) => {
                    self.base = info.config.clone();
                    self.cfg = Some(Config::new(info.config.clone()));
                    self.steps = info.macros.clone();
                    self.macros_ok = info.macros_err.is_none();
                    self.status = if self.offline { "modo offline (arquivo)".into() } else if self.write { String::new() } else { "somente leitura".into() };
                    if let Some(e) = &info.macros_err {
                        self.status = format!("erro lendo as macros: {e}");
                    }
                    self.info = Some(info);
                    if matches!(self.screen, Screen::Loading | Screen::Search) {
                        self.screen = Screen::Home;
                        self.apply_shot_target();
                    }
                }
                Event::NotConnected => {
                    if self.screen == Screen::Loading || self.cfg.is_none() {
                        self.screen = Screen::Search;
                        self.status.clear();
                    }
                }
                Event::Applied { fields, slot } => {
                    self.status = if fields > 0 { "salvo".into() } else { String::new() };
                    if let Some(c) = self.cfg.as_mut() {
                        c.set_cur_slot(slot);
                    }
                    if self.base.len() > off::SLOT + 1 {
                        self.base[off::SLOT..off::SLOT + 2].copy_from_slice(&slot.to_le_bytes());
                    }
                    // OpenTispDialog: editou um perfil que não é o ativo no controle
                    if let Some(i) = self.dev_slot() {
                        if fields > 0 && slot as usize != i && !self.warned && self.modal.is_none() {
                            self.warned = true;
                            self.open_modal(Modal::OpenTisp(i));
                        }
                    }
                }
                Event::Slot(s) => {
                    if let Some(c) = self.cfg.as_mut() {
                        c.set_cur_slot(s);
                    }
                    if self.base.len() > off::SLOT + 1 {
                        self.base[off::SLOT..off::SLOT + 2].copy_from_slice(&s.to_le_bytes());
                    }
                }
                Event::Keys(k, at) => {
                    self.keys = k;
                    self.record_keys(&k, at);
                }
                Event::Error(e) => self.status = format!("erro: {e}"),
            }
        }
        // SearchController: continua procurando o controle
        if self.screen == Screen::Search && self.last_load.elapsed() > Duration::from_secs(2) {
            self.last_load = Instant::now();
            let _ = self.dev.tx.send(Cmd::Load);
        }
        if self.screen == Screen::Search {
            ctx.request_repaint_after(Duration::from_secs(2));
        }
        // leitura ao vivo só nas abas que mostram a entrada (S_ReadKey.readKey)
        let want = if self.screen == Screen::Settings && !self.is_file() && self.modal.as_ref().is_none_or(|m| m.is_popup() || matches!(m, Modal::Tisp { .. } | Modal::OpenTisp(_))) {
            match self.tab {
                Tab::Stick | Tab::Trigger => Some((true, None)),
                Tab::Vibration => {
                    let (cfg, i) = self.cur();
                    Some((true, Some((cfg.rumble_level(i, 0), cfg.rumble_level(i, 1)))))
                }
                _ => None,
            }
        } else {
            None
        };
        let want = want.unwrap_or((false, None));
        if self.live != Some(want) {
            self.live = Some(want);
            let _ = self.dev.tx.send(Cmd::Live { on: want.0, vib: want.1 });
            if !want.0 {
                self.keys = Keys { lx: 128, ly: 128, rx: 128, ry: 128, ..Default::default() };
            }
        }
    }

    /// Igual ao app: cada alteração é gravada na hora (escrita).
    fn commit(&mut self) {
        match &self.slot {
            Slot::File(f) => {
                self.status = match profiles::save(Some(&f.id), &f.name, &f.cfg.b, &f.macros) {
                    Ok(_) => "salvo na lista de perfis".into(),
                    Err(e) => format!("erro: {e}"),
                };
                self.files = profiles::list();
            }
            Slot::Dev(_) => self.commit_dev(),
        }
    }
    fn commit_dev(&mut self) {
        if !self.write && !self.offline {
            self.status = "somente leitura — alteração não enviada".into();
            return;
        }
        let Some(cfg) = self.cfg.as_ref() else { return };
        let new = cfg.b.clone();
        let base = std::mem::replace(&mut self.base, new.clone());
        if base != new {
            self.status = "gravando…".into();
            let _ = self.dev.tx.send(Cmd::Apply { base, new });
        }
    }
    fn set_theme(&mut self, t: u8) {
        if let Some(i) = self.info.as_mut() {
            i.theme = t;
        }
        if !self.write && !self.offline {
            self.status = "somente leitura — efeito não enviado".into();
            return;
        }
        let _ = self.dev.tx.send(Cmd::Theme(t));
        self.status = "salvo".into();
    }

    fn go_profiles(&mut self) {
        self.screen = Screen::Profiles;
        self.files = profiles::list();
        if !self.offline {
            let _ = self.dev.tx.send(Cmd::Slot);
        }
    }
    fn go_settings(&mut self, slot: Slot, tab: Tab) {
        self.slot = slot;
        self.tab = tab;
        self.screen = Screen::Settings;
        self.scroll.clear();
        self.mac = Default::default();
    }

    fn run_act(&mut self, a: Act) {
        match a {
            Act::DeleteSlot(i) => {
                if let Some(c) = self.cfg.as_mut() {
                    c.remove_profile(i);
                }
                self.steps[i] = Default::default();
                self.commit_dev();
            }
            Act::MoveSlot(i, k) => {
                if let Some(c) = self.cfg.as_mut() {
                    let from = c.clone();
                    c.copy_profile(&from, i, k);
                }
                let steps = self.steps[i].clone();
                self.load_slot_macros(k, steps);
            }
            Act::LoadFile(f, k) => {
                let Some(fp) = self.files.get(f).cloned() else { return };
                if let Some(cfg) = self.cfg.as_mut() {
                    let src = Config::new(fp.config);
                    cfg.set_u32(off::FLAG + 4 * k, config::ENABLE);
                    for (o, n) in config::PER_SLOT {
                        cfg.b[o + n * k..o + n * (k + 1)].copy_from_slice(&src.b[o..o + n]);
                    }
                }
                self.load_slot_macros(k, fp.macros);
            }
            Act::DeleteFile(id) => {
                profiles::delete(&id);
                self.files = profiles::list();
            }
            Act::DeleteMacro(name) => self.delete_macro(&name),
        }
    }

    /// PlatFormView (mover/carregar): grava o struct e depois os passos de cada macro com tecla
    /// no perfil de destino (gravação dos passos). Sem os passos lidos do controle, zera as macros.
    fn load_slot_macros(&mut self, k: usize, steps: MacroSteps) {
        let Some(cfg) = self.cfg.as_mut() else { return };
        if !self.macros_ok {
            cfg.clear_macros(k);
            self.steps[k] = Default::default();
            self.commit_dev();
            return;
        }
        // macro com tecla mas sem os passos (perfil salvo antes das macros): desliga, senão o
        // controle rodaria o que sobrou na área de destino
        let mut defs = cfg.macro_defs(k);
        for (n, d) in defs.iter_mut().enumerate() {
            if steps[n].is_empty() {
                *d = None;
            } else if let Some(m) = d {
                // a contagem do registro tem que ser a dos passos gravados
                m.steps = steps[n].len().min(config::MACRO_MAX_STEPS) as u16;
            }
        }
        if defs != cfg.macro_defs(k) {
            cfg.set_macros(k, &defs);
        }
        let writes: Vec<(usize, Vec<config::Step>)> = (0..4)
            .filter(|&n| defs[n].is_some())
            .map(|n| (n, steps[n].clone())).collect();
        self.steps[k] = steps;
        self.commit_macros_dev(k, writes);
    }

    fn run_rename(&mut self, act: RenameAct, v: String) {
        let v: String = v.chars().take(16).collect();
        match act {
            RenameAct::Slot(i) => {
                if let Some(c) = self.cfg.as_mut() {
                    c.set_name(i, &v);
                }
                self.commit_dev();
            }
            RenameAct::File(f) => {
                if let Some(fp) = self.files.get(f).cloned() {
                    let mut c = Config::new(fp.config);
                    c.set_name(0, &v);
                    let _ = profiles::save(Some(&fp.id), &v, &c.b, &fp.macros);
                    self.files = profiles::list();
                }
            }
            RenameAct::Create => {
                if let Some(cfg) = self.cfg.as_ref() {
                    let mut c = cfg.clone();
                    c.create_profile(0, &v);
                    let _ = profiles::save(None, &v, &c.b, &Default::default());
                    self.files = profiles::list();
                }
            }
            RenameAct::MacroCreate | RenameAct::MacroRename(_) | RenameAct::MacroCopy(_) => self.macro_named(act, v),
        }
    }

    // ---------------------------------------------------------------- primitivas

    fn hit(&self, ui: &Ui, f: Fr, id: impl std::hash::Hash, x: f32, y: f32, w: f32, h: f32, drag: bool) -> H {
        if self.blocked {
            return H::default();
        }
        let rect = f.r(x, y, w, h).intersect(f.clip);
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return H::default();
        }
        let r = ui.interact(rect, Id::new(id), if drag { Sense::click_and_drag() } else { Sense::click() });
        if r.hovered() || r.dragged() {
            ui.ctx().set_cursor_icon(if drag { CursorIcon::ResizeHorizontal } else { CursorIcon::PointingHand });
        }
        H {
            hov: r.hovered(),
            click: r.clicked(),
            drag: r.dragged() || r.drag_started(),
            stop: r.drag_stopped(),
            pos: r.interact_pointer_pos().or(r.hover_pos()).map(|p| f.local(p)),
        }
    }

    fn img_a(&mut self, ui: &Ui, f: Fr, path: &str, x: f32, y: f32, w: f32, h: f32, a: f32) {
        if let Some(t) = self.assets.tex(ui.ctx(), path) {
            ui.painter().with_clip_rect(f.clip).image(t.id(), f.r(x, y, w, h),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), white(a));
        }
    }
    fn img(&mut self, ui: &Ui, f: Fr, path: &str, x: f32, y: f32, w: f32, h: f32) {
        self.img_a(ui, f, path, x, y, w, h, 1.0)
    }
    /// object-fit: contain
    fn img_contain(&mut self, ui: &Ui, f: Fr, path: &str, x: f32, y: f32, w: f32, h: f32) {
        if let Some(t) = self.assets.tex(ui.ctx(), path) {
            let [tw, th] = t.size();
            let s = (w / tw as f32).min(h / th as f32);
            let (iw, ih) = (tw as f32 * s, th as f32 * s);
            self.img(ui, f, path, x + (w - iw) / 2.0, y + (h - ih) / 2.0, iw, ih);
        }
    }

    fn rect(&self, ui: &Ui, f: Fr, x: f32, y: f32, w: f32, h: f32, r: f32, col: Color32) {
        ui.painter().with_clip_rect(f.clip).rect_filled(f.r(x, y, w, h), Rounding::same(f.s(r)), col);
    }
    fn rect_line(&self, ui: &Ui, f: Fr, x: f32, y: f32, w: f32, h: f32, r: f32, wd: f32, col: Color32) {
        ui.painter().with_clip_rect(f.clip).rect_stroke(f.r(x, y, w, h), Rounding::same(f.s(r)), Stroke::new(f.s(wd), col));
    }

    /// texto com canto superior esquerdo em (x, y)
    fn text(&self, ui: &Ui, f: Fr, s: &str, x: f32, y: f32, size: f32, col: Color32) -> Rect {
        ui.painter().with_clip_rect(f.clip).text(f.p(x, y), Align2::LEFT_TOP, s, FontId::proportional(f.s(size)), col)
    }
    fn text_al(&self, ui: &Ui, f: Fr, s: &str, x: f32, y: f32, size: f32, col: Color32, al: Align2) -> Rect {
        ui.painter().with_clip_rect(f.clip).text(f.p(x, y), al, s, FontId::proportional(f.s(size)), col)
    }
    /// centralizado numa caixa de largura w (linha a linha, como text-align:center + pre-line)
    fn text_c(&self, ui: &Ui, f: Fr, s: &str, x: f32, y: f32, w: f32, size: f32, col: Color32) {
        for (n, line) in s.lines().enumerate() {
            self.text_al(ui, f, line, x + w / 2.0, y + n as f32 * size * 1.15, size, col, Align2::CENTER_TOP);
        }
    }
    /// quebra de linha em largura w
    fn text_wrap(&self, ui: &Ui, f: Fr, s: &str, x: f32, y: f32, w: f32, size: f32, col: Color32) {
        let g = ui.painter().layout(s.to_string(), FontId::proportional(f.s(size)), col, f.s(w));
        ui.painter().with_clip_rect(f.clip).galley(f.p(x, y), g, col);
    }
    fn line(&self, ui: &Ui, f: Fr, pts: &[(f32, f32)], wd: f32, col: Color32) {
        let pts = pts.iter().map(|&(x, y)| f.p(x, y)).collect();
        ui.painter().with_clip_rect(f.clip).add(egui::Shape::line(pts, Stroke::new(f.s(wd), col)));
    }
    fn circle(&self, ui: &Ui, f: Fr, x: f32, y: f32, r: f32, fill: Color32, stroke: Color32, wd: f32) {
        ui.painter().with_clip_rect(f.clip).circle(f.p(x, y), f.s(r), fill, Stroke::new(f.s(wd), stroke));
    }

    /// Barra de rolagem fina (svStyle) e deslocamento atual de uma área rolável
    fn scroll_area(&mut self, ui: &Ui, f: Fr, key: &'static str, x: f32, y: f32, w: f32, h: f32, content: f32) -> Fr {
        let max = (content - h).max(0.0);
        let mut off = self.scroll.get(key).copied().unwrap_or(0.0).clamp(0.0, max);
        let rect = f.r(x, y, w, h);
        if !self.blocked && ui.rect_contains_pointer(rect) {
            let d = ui.input(|i| i.smooth_scroll_delta.y);
            off = (off - d / f.k).clamp(0.0, max);
        }
        self.scroll.insert(key, off);
        if max > 0.0 {
            let th = h * h / content;
            let ty = y + (h - th) * off / max;
            self.rect(ui, f, x + w - 6.0, ty, 6.0, th, 3.0, c(0x3A3A3A));
        }
        let mut inner = f.boxed(x, y, w, h);
        inner.o.y -= f.s(off);
        inner
    }

    // ---------------------------------------------------------------- controles

    /// Botão "voltar" 80×80 com texto (mainpage/back_*)
    fn back_button(&mut self, ui: &Ui, f: Fr, label: &str) -> bool {
        let h = self.hit(ui, f, "back", 0.0, 0.0, 80.0, 80.0, false);
        self.img(ui, f, if h.hov { "mainpage/back_enter.png" } else { "mainpage/back_normal.png" }, 0.0, 0.0, 80.0, 80.0);
        self.text_al(ui, f, label, 40.0, 48.0, 16.0, TEXT, Align2::CENTER_TOP);
        h.click
    }

    /// controle deslizante duplo: dois cursores; trilho de 300px de x=20 a 320. Devolve (a, z, soltou).
    fn progress(&mut self, ui: &Ui, f: Fr, key: &'static str, title: &str, x: f32, y: f32, ab: (u8, u8), max: f32) -> Option<(u8, u8, bool)> {
        let f = f.at(x, y);
        let (mut a, mut z) = self.drag_val.get(key).copied().unwrap_or(ab);
        let px = |v: u8| 20.0 + v as f32 * 300.0 / max;
        let mut out = None;
        for which in 0..2 {
            let v = if which == 0 { a } else { z };
            let h = self.hit(ui, f, (key, which), px(v) - 15.0, 20.0, 30.0, 100.0, true);
            if h.drag || h.stop {
                if let Some((mx, _)) = h.pos {
                    let nv = ((mx - 20.0).clamp(0.0, 300.0) * max / 300.0).round() as i32;
                    if which == 0 { a = nv.min(z as i32 - 1).max(0) as u8 } else { z = nv.max(a as i32 + 1).min(max as i32) as u8 }
                }
                self.drag_val.insert(key, (a, z));
                out = Some((a, z, h.stop));
                if h.stop {
                    self.drag_val.remove(key);
                }
            }
        }
        self.text(ui, f, title, 20.0, 0.0, 19.0, MUTED);
        self.rect(ui, f, 20.0, 63.0, 300.0, 4.0, 0.0, white(0.1));
        self.rect(ui, f, px(a), 63.0, px(z) - px(a), 4.0, 0.0, ACCENT);
        for v in [a, z] {
            self.img(ui, f, "sticks/sticks_tisp.png", px(v) - 12.0, 50.0, 24.0, 24.0);
            self.text_al(ui, f, &((v as u32 * 100) / max as u32).to_string(), px(v), 102.0, 16.0, MUTED, Align2::CENTER_TOP);
        }
        out
    }

    /// controle deslizante de vibração: um cursor, 6 posições (0..5) a cada 60px. Devolve o nível ao soltar.
    fn level_progress(&mut self, ui: &Ui, f: Fr, key: &'static str, title: &str, x: f32, y: f32, lv0: u8) -> Option<u8> {
        let f = f.at(x, y);
        let mut lv = self.drag_val.get(key).map_or(lv0, |v| v.0);
        let h = self.hit(ui, f, key, 20.0 + 60.0 * lv as f32 - 15.0, 20.0, 30.0, 100.0, true);
        let mut out = None;
        if h.drag || h.stop {
            if let Some((mx, _)) = h.pos {
                lv = (((mx - 20.0) / 60.0).round()).clamp(0.0, 5.0) as u8;
            }
            self.drag_val.insert(key, (lv, 0));
            if h.stop {
                self.drag_val.remove(key);
                out = Some(lv);
            }
        }
        self.text(ui, f, title, 20.0, 0.0, 20.0, MUTED);
        self.rect(ui, f, 20.0, 63.0, 300.0, 4.0, 0.0, white(0.1));
        self.rect(ui, f, 20.0, 63.0, 60.0 * lv as f32, 4.0, 0.0, ACCENT);
        for k in 0..6 {
            self.rect(ui, f, 20.0 + (60.0 * k as f32).min(299.0), 58.0, 1.0, 15.0, 0.0, Color32::WHITE);
            self.text(ui, f, &k.to_string(), 17.0 + 60.0 * k as f32, 100.0, 12.0, TEXT);
        }
        self.img(ui, f, "sticks/sticks_tisp.png", 20.0 + 60.0 * lv as f32 - 12.0, 50.0, 24.0, 24.0);
        out
    }

    /// SpeedProgress / SensitivityProgress / DeadCompensateProgress: 276 de largura, trilho de 252
    fn value_slider(&mut self, ui: &Ui, f: Fr, key: &'static str, title: &str, x: f32, y: f32, v0: u8, min: u8, max: u8,
                    small: Option<(&str, &str)>) -> Option<u8> {
        let f = f.at(x, y);
        let mut v = self.drag_val.get(key).map_or(v0, |v| v.0);
        let px = |v: u8| (v as f32 - min as f32) * 252.0 / (max - min) as f32;
        let h = self.hit(ui, f, key, 12.0 + px(v) - 12.0, 32.0, 24.0, 24.0, true);
        let mut out = None;
        if h.drag || h.stop {
            if let Some((mx, _)) = h.pos {
                v = (min as f32 + (mx - 12.0).clamp(0.0, 252.0) * (max - min) as f32 / 252.0).round() as u8;
            }
            self.drag_val.insert(key, (v, 0));
            if h.stop {
                self.drag_val.remove(key);
                out = Some(v);
            }
        }
        self.text(ui, f, title, 12.0, 0.0, 18.0, white(0.6));
        self.text_al(ui, f, &v.to_string(), 264.0, 4.0, 14.0, white(0.6), Align2::RIGHT_TOP);
        self.rect(ui, f, 12.0, 41.0, 252.0, 6.0, 0.0, white(0.1));
        self.rect(ui, f, 12.0, 41.0, px(v), 6.0, 0.0, ACCENT);
        self.circle(ui, f, 12.0 + px(v), 44.0, 11.0, c(0x50B3E3), Color32::WHITE, 2.0);
        if let Some((s, l)) = small {
            self.text(ui, f, s, 12.0, 57.0, 14.0, white(0.6));
            self.text_al(ui, f, l, 264.0, 57.0, 14.0, white(0.6), Align2::RIGHT_TOP);
        }
        out
    }

    /// interruptor 62×28. Devolve true quando clicado.
    fn toggle(&mut self, ui: &Ui, f: Fr, id: impl std::hash::Hash, x: f32, y: f32, on: bool) -> bool {
        let h = self.hit(ui, f, id, x, y, 62.0, 28.0, false);
        self.rect(ui, f, x, y, 62.0, 28.0, 14.0, if on { BLUE } else { c(0x2B2B2B) });
        let kx = if on { 36.0 } else { 6.0 };
        self.rect(ui, f, x + kx, y + 4.0, 20.0, 20.0, 10.0, if on { ACCENT } else { white(0.6) });
        h.click
    }

    /// sysbtn15 + PullDown.png: "dropdown" de 252×36
    fn dropdown(&mut self, ui: &Ui, f: Fr, id: &'static str, x: f32, y: f32, label: &str) -> bool {
        let h = self.hit(ui, f, id, x, y, 252.0, 36.0, false);
        let a = if h.hov { 1.0 } else { 0.6 };
        self.rect(ui, f, x, y, 252.0, 36.0, 4.0, BG);
        self.rect_line(ui, f, x + 0.5, y + 0.5, 251.0, 35.0, 4.0, 1.0, white(a));
        let fc = f.boxed(x + 20.0, y, 198.0, 36.0);
        self.text_al(ui, fc, label, 0.0, 18.0, 16.0, white(a), Align2::LEFT_CENTER);
        self.img_a(ui, f, "selectplatform/pulldown.png", x + 218.0, y + 1.0, 32.0, 32.0, a);
        h.click
    }

    // ---------------------------------------------------------------- telas

    fn frame_ui(&mut self, ui: &mut Ui) {
        let full = ui.max_rect();
        ui.painter().rect_filled(full, Rounding::ZERO, BG);
        let k = (full.width() / 1080.0).min(full.height() / 780.0);
        let o = pos2(full.center().x - 540.0 * k, full.center().y - 390.0 * k);
        let win = Fr { o, k, clip: Rect::from_min_size(o, vec2(1080.0 * k, 780.0 * k)) };

        // titlebar
        self.img(ui, win, "logomain.png", 0.0, 0.0, 52.0, 42.0);
        self.text_al(ui, win, "8BitDo Ultimate Software V2", 52.0, 21.0, 16.0, TEXT, Align2::LEFT_CENTER);
        let st = self.status.clone();
        self.text_al(ui, win, &st, 1064.0, 21.0, 13.0, c(0x707070), Align2::RIGHT_CENTER);

        let content = win.boxed(0.0, 42.0, 1080.0, 738.0);
        self.blocked = self.modal.is_some();
        match self.screen {
            Screen::Loading => {}
            Screen::Search => self.search_view(ui, content),
            Screen::Home => self.home_view(ui, content),
            Screen::Profiles => self.profiles_view(ui, content),
            Screen::Settings => self.settings_view(ui, content),
            Screen::MacroEdit => self.editor_view(ui, content),
        }
        self.blocked = false;
        if self.modal.is_some() {
            self.modal_ui(ui, win, content);
        }
    }

    /// busca de controle
    fn search_view(&mut self, ui: &Ui, f: Fr) {
        let t = self.s("getConnect");
        self.text_c(ui, f, &t, 0.0, 306.0, 1080.0, 26.0, c(0xACACAC));
        let h = self.hit(ui, f, "help", 358.0, 368.0, 364.0, 64.0, false);
        self.rect(ui, f, 358.0, 368.0, 364.0, 64.0, 32.0, c(0x333333));
        let t = self.s("getNeedHelp");
        self.text_al(ui, f, &t, 540.0, 400.0, 20.0, Color32::WHITE, Align2::CENTER_CENTER);
        if h.click {
            self.last_load = Instant::now();
            let _ = self.dev.tx.send(Cmd::Load);
        }
    }

    /// tela inicial: para o Ultimate 2, só "Profiles" e "Firmware"
    fn home_view(&mut self, ui: &Ui, f: Fr) {
        let top = (738.0 - 665.0) / 2.0;
        let btn_y = top + 110.0 + 353.0;
        let pid = self.info.as_ref().map_or(0, |i| i.pid);
        let name = self.s(if pid == 0x6012 { "getUltimate2_4_2" } else { "getUltimate2_4_2RR" });
        self.text_c(ui, f, &name, 0.0, top + 64.0, 1080.0, 22.0, TEXT);
        let suf = self.color_suffix();
        self.img_contain(ui, f, &format!("selectplatform/ultimate_2_4_2{suf}.png"), 282.0, top + 136.0, 516.0, 300.0);
        self.img_a(ui, f, "selectplatform/usb.png", 532.0, btn_y + 8.0, 28.0, 28.0, 0.6);
        for (x, icon, label) in [(254.0, "jp_setting.png", "Profiles"), (408.0, "update.png", "Firmware")] {
            let (tx, ty) = (142.0 + x, btn_y + 62.0);
            let h = self.hit(ui, f, label, tx, ty, 134.0, 110.0, false);
            self.rect(ui, f, tx, ty, 134.0, 110.0, 3.0, c(if h.hov { 0x4C4C4C } else { 0x333333 }));
            self.img(ui, f, &format!("selectplatform/{icon}"), tx + 52.0, ty + 28.0, 31.0, 30.0);
            self.text_al(ui, f, label, tx + 67.0, ty + 78.0, 14.0, c(0x989898), Align2::CENTER_TOP);
            if h.click {
                if label == "Profiles" {
                    self.go_profiles();
                } else {
                    self.open_modal(Modal::Tisp {
                        msg: "Atualização de firmware não faz parte deste port\n(pode inutilizar o controle).\nUse o app oficial no Windows.".into(),
                        ok: None,
                    });
                }
            }
        }
        let h = self.hit(ui, f, "answer", 12.0, 684.0, 40.0, 40.0, false);
        self.img(ui, f, if h.hov { "mainpage/answer_enter.png" } else { "mainpage/answer_normal.png" }, 12.0, 684.0, 40.0, 40.0);
    }

    /// tela de plataforma
    fn profiles_view(&mut self, ui: &Ui, f: Fr) {
        let Some(cfg) = self.cfg.clone() else { return };
        const R1: f32 = 80.0;
        let l = self.s("getHome");
        if self.back_button(ui, f, &l) {
            self.screen = Screen::Home;
        }
        let t = self.s("getProfile");
        self.text_c(ui, f, &t, 0.0, 24.0, 1080.0, 32.0, white(0.2));
        // Create_grid: botão "Create new profile"
        let h = self.hit(ui, f, "create", 341.0, R1, 398.0, 52.0, false);
        self.rect(ui, f, 380.0, R1, 320.0, 52.0, 26.0, if h.hov { BLUE } else { white(0.1) });
        let t = self.s("getCreateProfile");
        self.text_al(ui, f, &t, 540.0, R1 + 26.0, 24.0, Color32::WHITE, Align2::CENTER_CENTER);
        if h.click {
            self.open_modal(Modal::Rename { text: String::new(), act: RenameAct::Create, focus: true });
        }
        let t = self.s("getCurrentProfile");
        self.text_c(ui, f, &t, 0.0, R1 + 76.0, 1080.0, 20.0, white(0.6));
        let null = self.s("getNull");
        for i in 0..3 {
            let used = cfg.used(i);
            let active = cfg.cur_slot() as usize == i;
            let y = R1 + 112.0 + 52.0 * i as f32;
            let name = if used { let n = cfg.name(i); if n.is_empty() { "—".into() } else { n } } else { null.clone() };
            if self.plat_row(ui, f, ("slot", i), y, &format!("platform/conplat_{}{}.png", i + 1, if active { "selection" } else { "normal" }), &name, used) {
                self.go_settings(Slot::Dev(i), self.tab);
            }
            if self.plat_set(ui, f, ("slotset", i), 315.0 + 398.0, y) {
                self.open_modal(Modal::SlotMenu(i));
            }
        }
        let t = self.s("getProfileList");
        self.text_c(ui, f, &t, 0.0, R1 + 292.0, 1080.0, 20.0, white(0.6));
        // lista de perfis em arquivo (ScrollViewer 500px)
        let files = self.files.clone();
        let lh = 738.0 - R1 - 330.0 - 42.0;
        let inner = self.scroll_area(ui, f, "files", 290.0, R1 + 330.0, 500.0, lh, 52.0 * files.len() as f32);
        for (k, fp) in files.iter().enumerate() {
            let y = 52.0 * k as f32;
            if self.plat_row(ui, inner.at(-315.0 + 25.0, 0.0), ("file", k), y, "platform/conplat_file.png", &fp.name, true) {
                self.go_settings(Slot::File(FileEdit { id: fp.id.clone(), name: fp.name.clone(), cfg: Config::new(fp.config.clone()),
                                                       macros: fp.macros.clone() }), self.tab);
            }
            if self.plat_set(ui, inner, ("fileset", k), 25.0 + 398.0, y) {
                let (_, sy) = f.local(inner.p(0.0, y));
                self.open_modal(Modal::FileMenu(k, sy - 40.0));
            }
        }
    }

    /// linha de perfil (.plat): 386×52 com ícone 36 e nome
    fn plat_row(&mut self, ui: &Ui, f: Fr, id: impl std::hash::Hash, y: f32, icon: &str, name: &str, used: bool) -> bool {
        let h = self.hit(ui, f, id, 315.0, y, 386.0, 52.0, false);
        let lit = h.hov && used;
        if lit {
            self.rect(ui, f, 315.0, y, 386.0, 52.0, 4.0, white(0.1));
        }
        let a = if lit { 1.0 } else { 0.6 };
        self.img_a(ui, f, icon, 321.0, y + 8.0, 36.0, 36.0, a);
        let fc = f.boxed(363.0, y, 280.0, 52.0);
        self.text_al(ui, fc, name, 0.0, 26.0, 18.0, white(a), Align2::LEFT_CENTER);
        self.rect(ui, f, 315.0, y + 51.0, 386.0, 1.0, 0.0, c(0x333333));
        h.click && used
    }
    fn plat_set(&mut self, ui: &Ui, f: Fr, id: impl std::hash::Hash, x: f32, y: f32) -> bool {
        let h = self.hit(ui, f, id, x, y, 52.0, 52.0, false);
        self.img_a(ui, f, if h.hov { "platform/setting_selection.png" } else { "platform/setting_normal.png" }, x, y, 52.0, 52.0,
                   if h.hov { 1.0 } else { 0.5 });
        self.rect(ui, f, x, y + 51.0, 52.0, 1.0, 0.0, c(0x333333));
        h.click
    }

    // ---------------------------------------------------------------- menu de ajustes

    /// Margens do ramo PID_Ultimate2: com m_mode == 2 a aba Vibration some e as outras se deslocam
    fn tabs_layout(&self) -> Vec<(Tab, &'static str, f32)> {
        if self.variant() == 2 {
            vec![(Tab::Mapping, "getMapping", 53.0), (Tab::Macro, "getMacro", 160.0), (Tab::Stick, "getStick", 267.0),
                 (Tab::Trigger, "getTrigger", 374.0), (Tab::Led, "getRGB", 481.0), (Tab::Axis, "getAxis", 588.0)]
        } else {
            vec![(Tab::Mapping, "getMapping", 0.0), (Tab::Macro, "getMacro", 107.0), (Tab::Stick, "getStick", 214.0),
                 (Tab::Trigger, "getTrigger", 324.0), (Tab::Vibration, "getVibration", 431.0), (Tab::Led, "getRGB", 538.0),
                 (Tab::Axis, "getAxis", 645.0)]
        }
    }

    fn settings_view(&mut self, ui: &mut Ui, f: Fr) {
        if self.variant() == 2 && self.tab == Tab::Vibration {
            self.tab = Tab::Mapping;
        }
        let (cfg, i) = self.cur();
        let l = self.s("getProfile");
        if self.back_button(ui, f, &l) {
            self.go_profiles();
            return;
        }
        let plat = ["plat_switch.png", "plat_android.png", "plat_window.png", "plat_window.png"][cfg.mode().min(3) as usize];
        let (icon, label, name) = match &self.slot {
            Slot::File(fe) => ("platform/conplat_file.png".to_string(), self.s("getFileProfile"), fe.name.clone()),
            Slot::Dev(i) => (format!("platform/conplat_{}.png", i + 1), self.s("getCurrentProfile"), cfg.name(*i)),
        };
        self.img(ui, f, &icon, 24.0, 102.0, 20.0, 20.0);
        self.text(ui, f, &label, 48.0, 98.0, 20.0, TEXT);
        self.img(ui, f, &format!("platform/{plat}"), 24.0, 128.0, 20.0, 20.0);
        self.text(ui, f.boxed(48.0, 124.0, 800.0, 30.0), &name, 0.0, 0.0, 20.0, TEXT);
        for (tab, label, x) in self.tabs_layout() {
            let on = tab == self.tab;
            let tx = 173.5 + x;
            let h = self.hit(ui, f, ("tab", tab.key()), tx, 0.0, 88.0, 80.0, false);
            self.img(ui, f, &format!("menu/{}_{}.png", tab.key(), if on { "selection" } else { "normal" }), tx, 0.0, 88.0,
                     if tab == Tab::Axis { 80.0 } else { 58.0 });
            let t = self.s(label);
            self.text_al(ui, f, &t, tx + 44.0, 52.0, 14.0, if on { ACCENT } else { TEXT }, Align2::CENTER_TOP);
            if on || h.hov {
                self.rect(ui, f, tx, 78.0, 88.0, 2.0, 0.0, if on { ACCENT } else { TEXT });
            }
            if h.click && !on {
                self.tab = tab;
                self.scroll.clear();
            }
        }
        let body = f.boxed(0.0, 148.0, 1080.0, 590.0);
        match self.tab {
            Tab::Mapping => self.mapping_view(ui, body, &cfg, i),
            Tab::Macro => self.macro_view(ui, body, &cfg, i),
            Tab::Stick => self.sticks_view(ui, body, &cfg, i),
            Tab::Trigger => self.trigger_view(ui, body, &cfg, i),
            Tab::Vibration => self.vibration_view(ui, body, &cfg, i),
            Tab::Led => self.led_view(ui, body, &cfg, i),
            Tab::Axis => self.axis_view(ui, body, &cfg, i),
        }
    }

    fn using_tisp(&self, cfg: &Config, i: usize) -> bool {
        !self.is_file() && cfg.cur_slot() as usize != i
    }

    // ---------------------------------------------------------------- aba Mapping

    fn mapping_view(&mut self, ui: &Ui, body: Fr, cfg: &Config, i: usize) {
        let root = body.at(0.0, (590.0 - 577.0) / 2.0);
        let back = self.back;
        let suf = self.color_suffix();
        let bg = if back { format!("mapping/back_ultimate2_{}{suf}.png", if self.variant() == 2 { 1 } else { 2 }) } else { format!("mapping/ultimate2_mapping{suf}.png") };
        self.img(ui, root, &bg, 289.0, if back { 55.0 } else { 45.0 }, 502.0, 387.0);
        if self.using_tisp(cfg, i) {
            let t = self.s("getUsingTisp");
            self.text(ui, root, &t, 227.0, 14.0, 14.0, c(0xDB9B34));
        }
        let defs = macro_ui::macro_defs_named(cfg, i);
        for (n, &(pts, kp, ty, dir)) in U2_MAP.iter().enumerate() {
            if BACK_TYPES.contains(&ty) != back {
                continue;
            }
            let target = cfg.target(i, ty);
            let badge = macro_ui::macro_badge(&defs, ty);
            // handleUltimate2: destaque = remapeado; P1-P4 vazios e Home/Share padrão não destacam;
            // tecla com macro mostra M1-M4 (setMapping)
            let highlight = badge.is_some() || if ty.starts_with("AS_") { target != "N" } else { target != ty };
            let p: Vec<_> = pts.iter().map(|&(x, y)| (x, y - 155.0)).collect();
            self.line(ui, root, &p, 2.0, if highlight { ACCENT } else { c(0x4A4A4A) });
            let (bx, by) = (kp.0, kp.1 - 155.0);
            let lx = if dir == 'L' { bx - 45.0 } else { bx + 5.0 };
            let name = key_name(target).to_lowercase();
            let h = self.hit(ui, root, ("map", n), lx, by - 20.0, 40.0, 40.0, false);
            let state = if h.hov { "enter" } else if highlight { "selection" } else { "normal" };
            let img = badge.unwrap_or_else(|| format!("mapping/mapping_{name}_{state}.png"));
            self.img(ui, root, &img, lx, by - 20.0, 40.0, 40.0);
            if h.hov {
                let tip = format!("{} → {}", key_name(ty), key_name(target));
                egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), Id::new("maptip"), |ui| { ui.label(tip); });
            }
            if h.click {
                self.open_modal(Modal::MapPick(ty));
            }
        }
        // Front / Back (96×32, 100px acima do fundo)
        let y = 577.0 - 132.0;
        for (x, key, on, val) in [(0.0, "getFront", !back, false), (139.0, "getBackMapping", back, true)] {
            let bx = 422.5 + x;
            let h = self.hit(ui, root, key, bx, y, 96.0, 32.0, false);
            self.rect(ui, root, bx, y, 96.0, 32.0, 5.0, if on { BLUE } else { c(0x282828) });
            if h.hov {
                self.rect_line(ui, root, bx + 1.0, y + 1.0, 94.0, 30.0, 5.0, 2.0, BLUE);
            }
            let t = self.s(key);
            self.text_al(ui, root, &t, bx + 48.0, y + 16.0, 18.0, if on { Color32::WHITE } else { c(0x939393) }, Align2::CENTER_CENTER);
            if h.click {
                self.back = val;
            }
        }
    }

    /// tela de mapeamento: página inteira com a grade de destinos
    fn map_picker(&mut self, ui: &Ui, content: Fr, ty: &'static str) {
        self.rect(ui, content, 0.0, 0.0, 1080.0, 738.0, 0.0, BG);
        let l = self.s("getBack");
        if self.back_button(ui, content, &l) {
            self.modal = None;
            return;
        }
        let (cfg, i) = self.cur();
        let current = cfg.target(i, ty);
        let grid = content.at(337.0, 227.0);
        let (mut lx, mut ly) = (0.0f32, 0.0f32);
        for (n, &m) in PICK_U2.iter().enumerate() {
            let (mut x, mut y) = (lx + 76.0, ly);
            if n == 0 {
                (x, y) = (0.0, 0.0);
            }
            if x >= 406.0 {
                (x, y) = (0.0, ly + 76.0);
            }
            (lx, ly) = (x, y);
            let real = if m == "switchHome" { "Home" } else { m };
            let nm = key_name(real).to_lowercase();
            let cur = real == current;
            let h = self.hit(ui, grid, ("pick", n), x, y, 56.0, 56.0, false);
            self.img(ui, grid, &format!("mapping/setmapping/settingmapping_{nm}_{}.png", if cur || h.hov { "enter" } else { "normal" }), x, y, 56.0, 56.0);
            // ponto branco no destino padrão do botão
            let is_default = (ty.starts_with("AS_") && m == "N") || m == ty || (ty == "Home" && m == "switchHome");
            if is_default {
                self.circle(ui, grid, x + 56.0 * 0.46 + 2.0, y + 56.0 * 0.8 + 2.0, 2.0, Color32::WHITE, Color32::TRANSPARENT, 0.0);
            }
            if h.click {
                let (c, i) = self.cur_mut();
                c.set_target(i, ty, real);
                self.modal = None;
                self.commit();
            }
        }
    }

    // ---------------------------------------------------------------- aba Stick (Stick)

    fn sticks_view(&mut self, ui: &Ui, body: Fr, cfg: &Config, i: usize) {
        let top = (590.0 - 520.0) / 2.0;
        // usa a altura toda da área (590) para caber sem barra de rolagem
        let inner = self.scroll_area(ui, body, "sticks", 0.0, 0.0, 416.0, 590.0, 575.0);
        let mut st = [cfg.pair(off::JOY, i, 0), cfg.pair(off::JOY, i, 1)];
        for (side, key, title, y) in [(0, "lstick", "getLStick", 20.0), (1, "rstick", "getRStick", 170.0)] {
            let t = self.s(title);
            if let Some((a, z, done)) = self.progress(ui, inner, key, &t, 25.0, y, st[side], 128.0) {
                st[side] = (a, z);
                let (c, i) = self.cur_mut();
                c.set_pair(off::JOY, i, side, a, z);
                if done {
                    self.commit();
                }
            }
        }
        let fe = cfg.feature(i);
        let mut toggles = vec![];
        let t = self.s("getInvertx");
        self.text(ui, inner, &t, 45.0, 320.0, 19.0, MUTED);
        self.text(ui, inner, "L", 70.0, 365.0, 19.0, MUTED);
        toggles.push((feat::LX, 109.0, 365.0));
        self.text(ui, inner, "R", 299.0, 365.0, 19.0, MUTED);
        toggles.push((feat::RX, 335.0, 365.0));
        let t = self.s("getInverty");
        self.text(ui, inner, &t, 45.0, 410.0, 19.0, MUTED);
        self.text(ui, inner, "L", 70.0, 455.0, 19.0, MUTED);
        toggles.push((feat::LY, 109.0, 455.0));
        self.text(ui, inner, "R", 299.0, 455.0, 19.0, MUTED);
        toggles.push((feat::RY, 335.0, 455.0));
        let t = self.s("getSwaplrstick");
        self.text_wrap(ui, inner, &t, 45.0, 500.0, 280.0, 19.0, MUTED);
        toggles.push((feat::SWAP_STICKS, 335.0, 500.0));
        let t = self.s("getSwapdpad");
        self.text_wrap(ui, inner, &t, 45.0, 545.0, 280.0, 19.0, MUTED);
        toggles.push((feat::DPAD_LSTICK, 335.0, 545.0));
        for (mask, x, y) in toggles {
            if self.toggle(ui, inner, ("feat", mask), x, y, fe & mask != 0) {
                let (c, i) = self.cur_mut();
                c.set_feature(i, fe ^ mask);
                self.commit();
            }
        }

        let art = body.at(448.0, top + 10.0);
        let suf = self.color_suffix();
        self.img_contain(ui, art, &format!("selectplatform/ultimate_2_4_2{suf}.png"), 14.0, 19.0, 572.0, 462.0);
        let k = self.keys;
        // setas do direcional (só aparecem com o dongle, m_IDLE == 1)
        if self.info.as_ref().is_some_and(|i| i.pid != 0x6012) {
            for (bit, name, x, y) in [(0x200, "up", 213.0, 212.0), (0x100, "down", 213.0, 275.0), (0x80, "left", 183.0, 244.0), (0x40, "right", 244.0, 244.0)] {
                if k.buttons & bit != 0 {
                    self.img(ui, art, &format!("sticks/sticks_{name}.png"), x, y, 21.0, 21.0);
                }
            }
        }
        const MAX: f32 = 146.0;
        let center = |v: u8| if v > 123 && v < 133 { 128.0 } else { v as f32 };
        let vals = [(center(k.lx), center(k.ly)), (center(k.rx), center(k.ry))];
        for (n, (ox, oy)) in [(135.0, 163.0), (380.0, 253.0)].into_iter().enumerate() {
            let (a, z) = st[n];
            let eff_d = z as f32 * MAX / 128.0;
            let dead_d = a as f32 * MAX / 128.0;
            let mut x = vals[n].0 / 255.0 * MAX - MAX / 2.0;
            let mut y = vals[n].1 / 255.0 * MAX - MAX / 2.0;
            if x.abs() < 1.0 { x = 0.0; }
            if y.abs() < 1.0 { y = 0.0; }
            let d = x.hypot(y);
            let (mut tx, mut ty) = (x, y);
            if d > MAX / 2.0 {
                tx = x * (MAX / 2.0) / d;
                ty = y * (MAX / 2.0) / d;
            }
            let start = tx.hypot(ty) - dead_d / 2.0;
            let pct = if start > 0.0 { (start / (eff_d / 2.0 - dead_d / 2.0) * 100.0 + 0.5).clamp(0.0, 100.0) } else { 0.0 };
            self.circle(ui, art, ox, oy, MAX / 2.0, Color32::from_rgba_unmultiplied(23, 23, 23, 199), TEXT, 1.0);
            self.circle(ui, art, ox, oy, eff_d / 2.0, ca(0x80D5FF, 0.31), if pct >= 99.0 { Color32::WHITE } else { Color32::TRANSPARENT }, 1.0);
            self.circle(ui, art, ox, oy, dead_d / 2.0, Color32::from_rgba_unmultiplied(23, 23, 23, 199), Color32::TRANSPARENT, 0.0);
            self.line(ui, art, &[(ox - 22.0, oy), (ox + 22.0, oy)], 1.0, ca(0xBBBBBB, 0.3));
            self.line(ui, art, &[(ox, oy - 22.0), (ox, oy + 22.0)], 1.0, ca(0xBBBBBB, 0.3));
            self.circle(ui, art, ox, oy, 6.0, ACCENT, Color32::TRANSPARENT, 0.0);
            self.line(ui, art, &[(ox, oy), (ox + tx, oy + ty)], 2.0, ACCENT);
            self.img(ui, art, "sticks/sticks_cursor.png", ox + tx - 24.0, oy + ty - 24.0, 48.0, 48.0);
            // draw_String: texto a 10px do cursor, do lado oposto ao centro
            if pct.floor() > 0.0 {
                let lx = ox + x + if x < 0.0 { -50.0 } else { 10.0 };
                let ly = oy + y + if y > 0.0 { 10.0 } else { -10.0 - 16.0 };
                let s = (pct.floor() as u32).to_string();
                self.text(ui, art, &s, lx + 1.0, ly + 1.0, 16.0, Color32::from_rgba_unmultiplied(34, 34, 34, 199));
                self.text(ui, art, &s, lx, ly, 16.0, Color32::WHITE);
            }
        }
    }

    // ---------------------------------------------------------------- aba Trigger (Trigger)

    fn trigger_view(&mut self, ui: &Ui, body: Fr, cfg: &Config, i: usize) {
        let top = (590.0 - 520.0) / 2.0;
        let panel = body.at(0.0, top);
        let pct = |v: u8| (v as u32 * 100 / 255) as u8;
        let mut tr = [cfg.pair(off::TRIG, i, 0), cfg.pair(off::TRIG, i, 1)];
        for (side, key, title, y) in [(0, "ltrig", "getLTrigger", 20.0), (1, "rtrig", "getRTrigger", 170.0)] {
            let t = self.s(title);
            let (a, z) = tr[side];
            if let Some((a2, z2, done)) = self.progress(ui, panel, key, &t, 25.0, y, (pct(a), pct(z)), 100.0) {
                let (a, z) = ((a2 as u32 * 255 / 100) as u8, (z2 as u32 * 255 / 100) as u8);
                tr[side] = (a, z);
                let (c, i) = self.cur_mut();
                c.set_pair(off::TRIG, i, side, a, z);
                if done {
                    self.commit();
                }
            }
        }
        let t = self.s("getSwapltrt");
        self.text(ui, panel, &t, 45.0, 320.0, 20.0, MUTED);
        let fe = cfg.feature(i);
        if self.toggle(ui, panel, "swaptrig", 335.0, 320.0, fe & feat::SWAP_TRIGGERS != 0) {
            let (c, i) = self.cur_mut();
            c.set_feature(i, fe ^ feat::SWAP_TRIGGERS);
            self.commit();
        }
        // grade 500×500 à direita; imagem 500×292 centralizada
        let art = body.at(455.0 + (625.0 - 500.0) / 2.0, top + 10.0);
        let suf = self.color_suffix();
        self.img(ui, art, &format!("trigger/ultimate2_trigger{suf}.png"), 0.0, (500.0 - 292.0) / 2.0, 500.0, 292.0);
        // Canvas do original: arcos em (-10,60) e (320,60)
        let la = art.at(-10.0, 60.0);
        let ra = art.at(320.0, 60.0);
        self.line(ui, la, &arc_pts(100.0, 100.0, 100.0, (200.0, 100.0), (100.0, 0.0)), 5.0, c(0x3A3A3A));
        self.line(ui, ra, &arc_pts(100.0, 100.0, 100.0, (100.0, 0.0), (0.0, 100.0)), 5.0, c(0x3A3A3A));
        let get_left = |p: i32| 270 - p * 90 / 100;
        let get_right = |p: i32| 270 + p * 90 / 100;
        let rad = |a: i32| a as f32 * PI / 180.0;
        let get_point = |a: i32| ((100.0 + 100.0 * rad(a).sin()).trunc(), (100.0 - 100.0 * rad(a).cos()).trunc());
        let get_point1 = |a: i32| ((100.0 - 100.0 * rad(a).sin()).trunc(), (100.0 + 100.0 * rad(a).cos()).trunc());
        let k = self.keys;
        let left = if k.buttons & 0x4000 != 0 { 255 } else { k.lt } as i32;
        let right = if k.buttons & 0x8000 != 0 { 255 } else { k.rt } as i32;
        let (ls, le) = (pct(tr[0].0) as i32, pct(tr[0].1) as i32);
        let (rs, re) = (pct(tr[1].0) as i32, pct(tr[1].1) as i32);
        let lp = get_point(get_left(left * 100 / 255));
        let rp = get_point(get_right(right * 100 / 255));
        self.line(ui, la, &arc_pts(100.0, 100.0, 100.0, get_point1(get_left(ls)), get_point1(get_left(le))), 5.0, ACCENT);
        self.line(ui, ra, &arc_pts(100.0, 100.0, 100.0, get_point(get_right(re)), get_point(get_right(rs))), 5.0, ACCENT);
        let (lx, ly) = (175.0 - lp.0, 245.0 - lp.1);
        let (rx, ry) = (rp.0 + 305.0, rp.1 + 45.0);
        self.img(ui, art, "sticks/sticks_cursor.png", lx, ly, 30.0, 30.0);
        self.img(ui, art, "sticks/sticks_cursor.png", rx, ry, 30.0, 30.0);
        let lv = left * 100 / 255;
        let rv = right * 100 / 255;
        if lv > ls {
            let s = (((lv - ls) * 100 / (le - ls).max(1)).min(100)).to_string();
            self.text(ui, art, &s, lx + 60.0, ly, 19.0, ACCENT);
        }
        if rv > rs {
            let s = (((rv - rs) * 100 / (re - rs).max(1)).min(100)).to_string();
            self.text(ui, art, &s, rx - 50.0, ry, 19.0, ACCENT);
        }
    }

    // ---------------------------------------------------------------- aba Vibration (Vibration)

    fn vibration_view(&mut self, ui: &Ui, body: Fr, cfg: &Config, i: usize) {
        let top = (590.0 - 520.0) / 2.0;
        let panel = body.at(0.0, top);
        for (side, key, title, y) in [(0, "lvib", "getLVibration", 20.0), (1, "rvib", "getRVibration", 170.0)] {
            let t = self.s(title);
            if let Some(lv) = self.level_progress(ui, panel, key, &t, 25.0, y, cfg.rumble_level(i, side)) {
                let (c, i) = self.cur_mut();
                c.set_rumble_level(i, side, lv);
                self.commit();
            }
        }
        let t = self.s("getLTRTVibration");
        self.text(ui, panel, &t, 45.0, 320.0, 14.0, MUTED);
        let art = body.at(455.0 + (625.0 - 580.0) / 2.0, top + 10.0);
        let suf = self.color_suffix();
        self.img(ui, art, &format!("selectplatform/ultimate_2_4_2{suf}.png"), 40.0, (500.0 - 393.0) / 2.0, 500.0, 393.0);
        // destaque animado a cada 250 ms enquanto LT/RT estiver apertado
        let k = self.keys;
        let pressed = [k.lt > 0 || k.buttons & 0x4000 != 0, k.rt > 0 || k.buttons & 0x8000 != 0];
        let names = ["", "_1", "_2", "_3", "_4", "_5"];
        for n in 0..2 {
            if pressed[n] {
                let t0 = *self.vib_since[n].get_or_insert_with(Instant::now);
                let fr = (t0.elapsed().as_millis() / 250 % 6) as usize;
                self.img(ui, art, &format!("vibration/vibration_highlight{}.png", names[fr]), if n == 0 { 15.0 } else { 415.0 }, 325.0, 150.0, 150.0);
                ui.ctx().request_repaint_after(Duration::from_millis(250));
            } else {
                self.vib_since[n] = None;
            }
        }
    }

    // ---------------------------------------------------------------- aba RGB (RGB)

    fn led_color(&self, cfg: &Config, i: usize) -> u32 {
        if self.led.custom == Some(true) {
            return self.led.keys.first().map_or(0, |&k| cfg.single(i)[k]);
        }
        match self.led.mode {
            1 => { let d = cfg.dynamic(i); if self.led.color_type == 1 { d.1 } else { d.0 } }
            2 => { let h = cfg.hotwheel(i); if self.led.color_type == 1 { h.1 } else { h.0 } }
            _ => 0,
        }
    }
    fn set_led_color(&mut self, col: u32, done: bool) {
        let (custom, mode, ct, keys, sync) = (self.led.custom == Some(true), self.led.mode, self.led.color_type, self.led.keys.clone(), self.led.sync);
        let (cfg, i) = self.cur_mut();
        if custom {
            let mut s = cfg.single(i);
            for k in keys {
                s[k] = col;
                if sync {
                    s[(k + 12) % 24] = col;
                }
            }
            cfg.set_single(i, &s);
        } else if mode == 1 {
            let mut d = cfg.dynamic(i);
            if ct == 1 { d.1 = col } else { d.0 = col }
            cfg.set_dynamic(i, d);
        } else if mode == 2 {
            let mut h = cfg.hotwheel(i);
            if ct == 1 { h.1 = col } else { h.0 = col }
            cfg.set_hotwheel(i, h);
        }
        if done {
            self.commit();
        }
    }

    fn led_view(&mut self, ui: &mut Ui, body: Fr, cfg: &Config, i: usize) {
        let theme = self.info.as_ref().map_or(5, |i| i.theme);
        if self.led.custom.is_none() {
            self.led.custom = Some(theme == 4);
            self.led.mode = match theme { 2 => 1, 3 => 2, _ => 0 };
        }
        let custom = self.led.custom == Some(true);
        let root = body.at(0.0, (590.0 - 608.0) / 2.0);

        // painel esquerdo (ColorLight 300 de largura em 48,48)
        let show_color = if custom { !self.led.keys.is_empty() } else { self.led.mode > 0 };
        let panel_h = if self.led.mode == 0 && !custom { 300.0 } else { 540.0 };
        self.rect(ui, root, 48.0, 48.0, 300.0, panel_h, 8.0, c(0x232323));
        let panel = root.boxed(48.0, 48.0, 300.0, panel_h);
        for (x, key, on, to_custom) in [(0.0, "getDefaultRGB", !custom, false), (150.0, "getCustom", custom, true)] {
            let h = self.hit(ui, panel, ("ledtab", key), x, 0.0, 150.0, 60.0, false);
            let t = self.s(key);
            self.text_al(ui, panel, &t, x + 75.0, 30.0, 20.0, if on { ACCENT } else { white(0.6) }, Align2::CENTER_CENTER);
            if on {
                self.rect(ui, panel, x + 24.0, 58.0, 102.0, 2.0, 0.0, ACCENT);
            }
            if h.click && !on {
                if to_custom {
                    self.set_theme(4);
                } else {
                    let color = self.info.as_ref().map_or(0, |i| i.color);
                    self.set_theme([if color == 2 { 6 } else { 5 }, 2, 3][self.led.mode as usize]);
                }
                self.led.custom = Some(to_custom);
            }
        }
        let inner = self.scroll_area(ui, panel, "led", 0.0, 84.0, 300.0, panel_h - 84.0, if show_color { if custom { 425.0 } else if self.led.mode == 1 { 543.0 } else { 617.0 } } else { panel_h - 84.0 });
        let (pres_y, color_y);
        if !custom {
            let t = self.s("getRGBEffect");
            self.text(ui, inner, &t, 24.0, 0.0, 18.0, white(0.6));
            let modes = [self.s("getClose"), self.s("getLightTracing"), self.s("getFireRing")];
            if self.dropdown(ui, inner, "ledmode", 24.0, 30.0, &modes[self.led.mode as usize]) {
                self.open_modal(Modal::Choice { x: 72.0, y: 198.0, items: modes.iter().cloned().enumerate().map(|(n, s)| (n as u8, s)).collect(),
                                                cur: self.led.mode, act: ChoiceAct::LedMode });
            }
            if self.led.mode > 0 {
                let items = vec![(0, self.s("getTracingRGB")), (1, self.s("getBackgroundRGB"))];
                if self.dropdown(ui, inner, "ledct", 24.0, 90.0, &items[self.led.color_type as usize].1.clone()) {
                    self.open_modal(Modal::Choice { x: 72.0, y: 258.0, items, cur: self.led.color_type, act: ChoiceAct::LedColorType });
                }
            }
            if self.led.mode == 2 {
                let t = self.s("getSpeed");
                let hw = cfg.hotwheel(i);
                let sp = if hw.2 == 0 { 7 } else { hw.2.clamp(1, 10) };
                if let Some(v) = self.value_slider(ui, inner, "speed", &t, 12.0, 150.0, sp, 1, 10, None) {
                    let (c, i) = self.cur_mut();
                    let mut h = c.hotwheel(i);
                    h.2 = v;
                    c.set_hotwheel(i, h);
                    self.commit();
                }
            }
            pres_y = if self.led.mode == 1 { 150.0 } else { 224.0 };
            color_y = pres_y + 93.0;
        } else {
            let t = self.s("getSyncColor");
            self.text(ui, inner, &t, 24.0, 0.0, 18.0, white(0.6));
            let h = self.hit(ui, inner, "sync", 258.0, 0.0, 18.0, 18.0, false);
            self.img(ui, inner, if self.led.sync || h.hov { "sticks/whitecheck_selection.png" } else { "sticks/whitecheck_normal.png" }, 258.0, 0.0, 18.0, 18.0);
            if h.click {
                self.led.sync = !self.led.sync;
            }
            pres_y = 32.0;
            color_y = 125.0;
        }
        if show_color {
            let t = self.s("getDefaultRGB");
            self.text(ui, inner, &t, 24.0, pres_y, 18.0, white(0.6));
            for (n, &p) in PRESETS.iter().enumerate() {
                let x = 24.0 + 38.0 * n as f32;
                let h = self.hit(ui, inner, ("preset", n), x, pres_y + 39.0, 24.0, 24.0, false);
                let a = if h.hov { 1.0 } else { 0.6 };
                self.circle(ui, inner, x + 12.0, pres_y + 51.0, 12.0, ca(p, a), white(a), if h.hov { 2.0 } else { 1.0 });
                if h.click {
                    // preset mantém o brilho escolhido no slider Dark/Light
                    let (hh, ss, _) = rgb_to_hsv(p);
                    let col = match self.led.bright { Some(b) if b > 0.05 => hsv_to_rgb(hh, ss, b), _ => p };
                    self.set_led_color(col, true);
                }
            }
            let col = self.led_color(cfg, i);
            self.color_picker(ui, inner.at(0.0, color_y), col);
        }

        // controle + anéis
        let suf = self.color_suffix();
        self.img(ui, root, &format!("led/ultimate2_led{suf}.png"), 428.0, 119.0, 480.0, 370.0);
        let (cfg, i) = self.cur();
        if custom {
            let s = cfg.single(i);
            let mut clicked = None;
            for k in 0..24 {
                let (ox, oy) = if k < 12 { (489.0, 191.0) } else { (695.0, 267.0) };
                let a = ((76 + 30 * (k % 12)) % 360) as f32;
                let b = ((104 + 30 * (k % 12)) % 360) as f32;
                let lp = |d: f32| ((40.0 + 40.0 * (d * PI / 180.0).cos()).trunc(), (40.0 + 40.0 * (d * PI / 180.0).sin()).trunc());
                let pts = arc_pts(40.0, 40.0, 40.0, lp(a), lp(b));
                let sel = self.led.keys.contains(&k);
                let col = if sel { c(s[k] & 0xFFFFFF) } else if s[k] != 0 { ca(s[k] & 0xFFFFFF, 0.6 * 0.6) } else { ca(0x296785, 0.6 * 0.6) };
                self.line(ui, root.at(ox, oy), &pts, 11.0, col);
                // clique no segmento: distância ao centro ~40 e ângulo dentro do arco
                if !self.blocked {
                    if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
                        let (px, py) = root.at(ox, oy).local(p);
                        let (dx, dy) = (px - 40.0, py - 40.0);
                        let dist = dx.hypot(dy);
                        let mut ang = dy.atan2(dx) * 180.0 / PI;
                        if ang < 0.0 { ang += 360.0; }
                        let rel = (ang - a + 360.0) % 360.0;
                        if (34.0..=46.0).contains(&dist) && rel <= 28.0 {
                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                            if ui.input(|i| i.pointer.primary_clicked()) {
                                clicked = Some((k, ui.input(|i| i.modifiers.ctrl)));
                            }
                        }
                    }
                }
            }
            if let Some((k, ctrl)) = clicked {
                if ctrl {
                    if let Some(p) = self.led.keys.iter().position(|&x| x == k) { self.led.keys.remove(p); } else { self.led.keys.push(k); }
                } else {
                    self.led.keys = vec![k];
                }
            }
            let t = self.s("getCtrl");
            self.text_al(ui, root, &t, 1032.0, 608.0 - 97.0, 14.0, white(0.4), Align2::RIGHT_TOP);
        } else {
            let col = if self.led.mode > 0 { c(self.led_color(&cfg, i) & 0xFFFFFF) } else { Color32::BLACK };
            for (cx, cy) in [(529.0, 231.0), (735.0, 307.0)] {
                self.circle(ui, root, cx, cy, 39.5, Color32::TRANSPARENT, col, 11.0);
            }
        }
    }

    /// seletor de cor: roda de cor 176 + brilho + hex/RGB
    fn color_picker(&mut self, ui: &mut Ui, f: Fr, color: u32) {
        if self.picker.color != Some(color) {
            self.picker.color = Some(color);
            self.picker.hsv = rgb_to_hsv(color);
            self.picker.hex = format!("{:06X}", color & 0xFFFFFF);
            self.picker.rgb = [((color >> 16) & 255).to_string(), ((color >> 8) & 255).to_string(), (color & 255).to_string()];
        }
        let (mut h, mut s, mut v) = self.picker.hsv;
        let t = self.s("getColor");
        self.text(ui, f, &t, 24.0, 24.0, 18.0, white(0.6));
        let now = hsv_to_rgb(h, s, v);
        self.rect(ui, f, 249.0, 24.0, 28.0, 28.0, 14.0, c(now));

        // roda: centro branco, borda no matiz, escurecida por v
        let (cx, cy, r) = (24.0 + 88.0, 62.0 + 88.0, 88.0);
        let mut mesh = egui::Mesh::default();
        let g = (255.0 * v) as u8;
        mesh.colored_vertex(f.p(cx, cy), Color32::from_rgb(g, g, g));
        let n = 72;
        for k in 0..=n {
            let t = k as f32 / n as f32 * 360.0;
            mesh.colored_vertex(f.p(cx + r * (t * PI / 180.0).cos(), cy - r * (t * PI / 180.0).sin()), c(hsv_to_rgb(t, 1.0, v)));
            if k > 0 {
                mesh.add_triangle(0, k as u32, k as u32 + 1);
            }
        }
        ui.painter().with_clip_rect(f.clip).add(egui::Shape::mesh(mesh));
        let hw = self.hit(ui, f, "wheel", 24.0, 62.0, 176.0, 176.0, true);
        let mut changed = None;
        if (hw.drag || hw.stop || hw.click) && hw.pos.is_some() {
            let (mx, my) = hw.pos.unwrap();
            let (dx, dy) = (mx - cx, cy - my);
            h = (dy.atan2(dx) * 180.0 / PI + 360.0) % 360.0;
            s = (dx.hypot(dy) / 78.0).min(1.0);
            if v < 0.05 {
                v = 1.0; // com a cor gravada quase preta a roda ficaria inútil
            }
            changed = Some(hw.stop || hw.click);
        }
        let rad = s * 78.0;
        let ang = h * PI / 180.0;
        let (tx, ty) = (cx + rad * ang.cos(), cy - rad * ang.sin());
        self.circle(ui, f, tx, ty, 11.0, Color32::TRANSPARENT, Color32::BLACK, 1.0);
        self.circle(ui, f, tx, ty, 10.0, Color32::TRANSPARENT, Color32::WHITE, 2.0);

        // hex e RGB
        self.text(ui, f, "#", 193.0, 76.0, 16.0, white(0.6));
        self.rect(ui, f, 210.0, 97.0, 70.0, 1.0, 0.0, Color32::WHITE);
        let mut typed = None;
        if !self.blocked {
            let mut hex = self.picker.hex.clone();
            let r = ui.put(f.r(210.0, 74.0, 70.0, 22.0), egui::TextEdit::singleline(&mut hex).frame(false).char_limit(6)
                .font(FontId::proportional(f.s(16.0))).text_color(Color32::WHITE).horizontal_align(Align::Max).id(Id::new("hex")));
            self.picker.hex = hex;
            if r.lost_focus() {
                if let Ok(x) = u32::from_str_radix(&self.picker.hex, 16) {
                    if self.picker.hex.len() == 6 {
                        typed = Some(x);
                    }
                }
            }
        } else {
            let hx = self.picker.hex.clone();
            self.text_al(ui, f, &hx, 280.0, 76.0, 16.0, Color32::WHITE, Align2::RIGHT_TOP);
        }
        for (n, lab) in ["R", "G", "B"].iter().enumerate() {
            let y = 118.0 + 43.0 * n as f32;
            self.text(ui, f, lab, 220.0, y, 16.0, white(0.6));
            self.rect(ui, f, 247.0, y + 21.0, 30.0, 1.0, 0.0, Color32::WHITE);
            if !self.blocked {
                let mut sv = self.picker.rgb[n].clone();
                let r = ui.put(f.r(239.0, y - 2.0, 40.0, 22.0), egui::TextEdit::singleline(&mut sv).frame(false).char_limit(3)
                    .font(FontId::proportional(f.s(16.0))).text_color(Color32::WHITE).horizontal_align(Align::Max).id(Id::new(("rgb", n))));
                self.picker.rgb[n] = sv;
                if r.lost_focus() {
                    let ch: Vec<u32> = self.picker.rgb.iter().map(|x| x.trim().parse::<u32>().unwrap_or(0).min(255)).collect();
                    typed = Some(ch[0] << 16 | ch[1] << 8 | ch[2]);
                }
            } else {
                let t = self.picker.rgb[n].clone();
                self.text_al(ui, f, &t, 277.0, y, 16.0, Color32::WHITE, Align2::RIGHT_TOP);
            }
        }

        // brilho (Dark → Light)
        let t = self.s("getDark");
        self.text(ui, f, &t, 24.0, 255.0, 12.0, Color32::WHITE);
        let t = self.s("getLight");
        self.text(ui, f, &t, 260.0, 255.0, 12.0, Color32::WHITE);
        let br = f.r(24.0, 272.0, 252.0, 24.0);
        let mut m = egui::Mesh::default();
        m.colored_vertex(br.left_top(), Color32::BLACK);
        m.colored_vertex(br.right_top(), Color32::WHITE);
        m.colored_vertex(br.right_bottom(), Color32::WHITE);
        m.colored_vertex(br.left_bottom(), Color32::BLACK);
        m.add_triangle(0, 1, 2);
        m.add_triangle(0, 2, 3);
        ui.painter().with_clip_rect(f.clip.intersect(br.shrink2(vec2(f.s(12.0), 0.0)))).add(egui::Shape::mesh(m));
        self.circle(ui, f, 36.0, 284.0, 12.0, Color32::BLACK, Color32::TRANSPARENT, 0.0);
        self.circle(ui, f, 264.0, 284.0, 12.0, Color32::WHITE, Color32::TRANSPARENT, 0.0);
        let hb = self.hit(ui, f, "bright", 24.0, 272.0, 252.0, 24.0, true);
        if (hb.drag || hb.stop || hb.click) && hb.pos.is_some() {
            v = ((hb.pos.unwrap().0 - 34.0) / 232.0).clamp(0.0, 1.0);
            self.led.bright = Some(v);
            changed = Some(hb.stop || hb.click);
        }
        let bx = 34.0 + 232.0 * v;
        self.circle(ui, f, bx, 284.0, 10.0, Color32::TRANSPARENT, Color32::BLACK, 3.0);
        self.circle(ui, f, bx, 284.0, 10.0, Color32::TRANSPARENT, Color32::WHITE, 2.0);

        if let Some(x) = typed {
            self.picker.color = None;
            self.set_led_color(x, true);
        } else if let Some(done) = changed {
            let col = hsv_to_rgb(h, s, v);
            self.set_led_color(col, done);
            // mantém h/s mesmo quando a cor resultante não os preserva (v = 0)
            self.picker.color = Some(col);
            self.picker.hsv = (h, s, v);
            self.picker.hex = format!("{col:06X}");
            self.picker.rgb = [((col >> 16) & 255).to_string(), ((col >> 8) & 255).to_string(), (col & 255).to_string()];
        }
    }

    // ---------------------------------------------------------------- aba Motion (Motion)

    fn axis_view(&mut self, ui: &Ui, body: Fr, cfg: &Config, i: usize) {
        let root = body.at(0.0, (590.0 - 608.0) / 2.0);
        let sx = cfg.sixaxis(i);
        let card = |me: &Self, x: f32| me.rect(ui, root, x, 48.0, 300.0, 280.0, 8.0, c(0x232323));
        card(self, 48.0);
        let c1 = root.at(48.0, 48.0);
        let t = self.s("getAxisMapping");
        self.text(ui, c1, &t, 24.0, 24.0, 18.0, white(0.36));
        let modes = [(0u8, self.s("getClose")), (2, self.s("getLStick")), (1, self.s("getRStick"))];
        let label = modes.iter().find(|m| m.0 == sx.mapping).map_or(modes[0].1.clone(), |m| m.1.clone());
        if self.dropdown(ui, c1, "axismap", 24.0, 56.0, &label) {
            self.open_modal(Modal::Choice { x: 72.0, y: 140.0, items: modes.to_vec(), cur: sx.mapping, act: ChoiceAct::AxisMapping });
        }
        if sx.mapping == 0 {
            return;
        }
        card(self, 390.0);
        let c2 = root.at(390.0, 48.0);
        let t = self.s("getEnableMethod");
        self.text(ui, c2, &t, 24.0, 24.0, 18.0, white(0.36));
        let items = vec![(1u8, self.s("getHoldEnable")), (2, self.s("getPressEnable"))];
        let lab = if sx.trigger == 2 { items[1].1.clone() } else { items[0].1.clone() };
        if self.dropdown(ui, c2, "axistrig", 24.0, 56.0, &lab) {
            self.open_modal(Modal::Choice { x: 414.0, y: 140.0, items, cur: sx.trigger, act: ChoiceAct::AxisTrigger });
        }
        let t = self.s("getEnableButton");
        self.text(ui, c2, &t, 24.0, 116.0, 18.0, white(0.36));
        let nm = axis_name(sx.key);
        let h = self.hit(ui, c2, "axiskey", 96.0, 148.0, 108.0, 108.0, false);
        self.img(ui, c2, &format!("mapping/mapping_{nm}_{}.png", if h.hov { "selection" } else { "normal" }), 96.0, 148.0, 108.0, 108.0);
        self.rect_line(ui, c2, 96.0, 148.0, 108.0, 108.0, 4.0, 1.0, if h.hov { ACCENT } else { Color32::WHITE });
        if h.click {
            self.open_modal(Modal::AxisPick);
        }
        card(self, 732.0);
        let c3 = root.at(732.0, 48.0);
        let (ts, tl) = (self.s("getSmall"), self.s("getLarge"));
        let t = self.s("getSensitivity");
        if let Some(v) = self.value_slider(ui, c3, "sens", &t, 12.0, 14.0, sx.sensitivity, 1, 10, Some((&ts, &tl))) {
            let (c, i) = self.cur_mut();
            c.set_sixaxis(i, &Sixaxis { sensitivity: v, ..sx });
            self.commit();
        }
        let t = self.s("getDeadCompen");
        if let Some(v) = self.value_slider(ui, c3, "dead", &t, 12.0, 131.0, sx.dead, 0, 100, None) {
            let (c, i) = self.cur_mut();
            c.set_sixaxis(i, &Sixaxis { dead: v, ..sx });
            self.commit();
        }
        let h = self.hit(ui, c3, "axistip", 256.0, 244.0, 24.0, 24.0, false);
        self.img(ui, c3, if h.hov { "mainpage/axis_enter.png" } else { "mainpage/axis_normal.png" }, 256.0, 244.0, 24.0, 24.0);
        if h.hov {
            self.rect(ui, c3, 12.0, 60.0, 276.0, 174.0, 6.0, c(0x363636));
            let t = format!("{}\n\n{}", self.s("getSensitivityTisp"), self.s("getDeadCompensateTisp"));
            self.text_wrap(ui, c3, &t, 44.0, 80.0, 212.0, 14.0, white(0.86));
        }
    }

    /// AxisMappingSetting: grade de 6 colunas, 18 teclas
    fn axis_picker(&mut self, ui: &Ui, content: Fr) {
        self.rect(ui, content, 0.0, 0.0, 1080.0, 738.0, 0.0, BG);
        let l = self.s("getBack");
        if self.back_button(ui, content, &l) {
            self.modal = None;
            return;
        }
        let (cfg, i) = self.cur();
        let sx = cfg.sixaxis(i);
        let grid = content.at(337.0, 227.0);
        for (n, &(v, nm)) in AXIS_KEYS.iter().enumerate() {
            let (x, y) = ((n % 6) as f32 * 76.0, (n / 6) as f32 * 76.0);
            let h = self.hit(ui, grid, ("axispick", n), x, y, 56.0, 56.0, false);
            let on = v == sx.key || h.hov;
            self.img(ui, grid, &format!("mapping/setmapping/settingmapping_{}_{}.png", nm.to_lowercase(), if on { "enter" } else { "normal" }), x, y, 56.0, 56.0);
            if h.click {
                let (c, i) = self.cur_mut();
                c.set_sixaxis(i, &Sixaxis { key: v, ..sx });
                self.modal = None;
                self.commit();
            }
        }
    }

    // ---------------------------------------------------------------- diálogos e menus

    fn modal_ui(&mut self, ui: &mut Ui, win: Fr, content: Fr) {
        let Some(m) = self.modal.take() else { return };
        let mut keep = true;
        let mut item_clicked = false;
        let mut popup_rect = None;
        match &m {
            Modal::SlotMenu(i) => {
                let i = *i;
                let cfg = self.cfg.clone().unwrap();
                let used = cfg.used(i);
                let others: Vec<usize> = (0..3).filter(|&k| k != i).collect();
                let items: Vec<(String, String)> = vec![
                    (self.s(&format!("getMove{}", others[0] + 1)), format!("platform/plat_{}normal.png", others[0] + 1)),
                    (self.s(&format!("getMove{}", others[1] + 1)), format!("platform/plat_{}normal.png", others[1] + 1)),
                    (self.s("getRename"), "platform/rename_normal.png".into()),
                    (self.s("getCopy"), "platform/copy_normal.png".into()),
                    (self.s("getRemove"), "platform/remove_normal.png".into()),
                    (self.s("getDelete"), "platform/delete_normal.png".into()),
                ];
                let (x, y) = (777.0, 80.0 + 114.0 + 52.0 * i as f32 - 52.0);
                popup_rect = Some(content.r(x, y, 220.0, 8.0 + 42.0 * 6.0));
                if let Some(n) = self.menu(ui, content, x, y, &items, used) {
                    item_clicked = true;
                    match n {
                        0 | 1 => {
                            let k = others[n];
                            if cfg.used(k) {
                                self.open_modal(Modal::Tisp { msg: format!("Substituir o perfil {} (\"{}\")?", k + 1, cfg.name(k)), ok: Some(Act::MoveSlot(i, k)) });
                            } else {
                                self.run_act(Act::MoveSlot(i, k));
                            }
                        }
                        2 => self.open_modal(Modal::Rename { text: cfg.name(i), act: RenameAct::Slot(i), focus: true }),
                        3 => {
                            // Copy: manda o perfil do controle para a lista local
                            let mut c = cfg.clone();
                            c.copy_profile(&cfg, i, 0);
                            if !self.macros_ok {
                                c.clear_macros(0);
                            }
                            let _ = profiles::save(None, &cfg.name(i), &c.b, &self.steps[i]);
                            self.files = profiles::list();
                        }
                        _ => self.open_modal(Modal::Tisp { msg: self.s("getDeleteProfile"), ok: Some(Act::DeleteSlot(i)) }),
                    }
                }
            }
            Modal::FileMenu(k, y) => {
                let k = *k;
                let items: Vec<(String, String)> = vec![
                    (self.s("getLoad1"), "platform/plat_1normal.png".into()),
                    (self.s("getLoad2"), "platform/plat_2normal.png".into()),
                    (self.s("getLoad3"), "platform/plat_3normal.png".into()),
                    (self.s("getRename"), "platform/rename_normal.png".into()),
                    (self.s("getCopy"), "platform/copy_normal.png".into()),
                    (self.s("getDelete"), "platform/delete_normal.png".into()),
                ];
                popup_rect = Some(content.r(646.0, *y, 220.0, 8.0 + 42.0 * 6.0));
                if let Some(n) = self.menu(ui, content, 646.0, *y, &items, true) {
                    item_clicked = true;
                    let fp = self.files.get(k).cloned();
                    if let (Some(fp), Some(cfg)) = (fp, self.cfg.clone()) {
                        match n {
                            0..=2 => {
                                if cfg.used(n) {
                                    self.open_modal(Modal::Tisp { msg: format!("Substituir o perfil {} (\"{}\")?", n + 1, cfg.name(n)), ok: Some(Act::LoadFile(k, n)) });
                                } else {
                                    self.run_act(Act::LoadFile(k, n));
                                }
                            }
                            3 => self.open_modal(Modal::Rename { text: fp.name.clone(), act: RenameAct::File(k), focus: true }),
                            4 => {
                                let _ = profiles::save(None, &format!("{} copy", fp.name), &fp.config, &fp.macros);
                                self.files = profiles::list();
                            }
                            _ => self.open_modal(Modal::Tisp { msg: self.s("getDeleteProfile"), ok: Some(Act::DeleteFile(fp.id.clone())) }),
                        }
                    }
                }
            }
            Modal::Choice { x, y, items, cur, act } => {
                let (x, y) = (*x, *y);
                let hgt = 12.0 + 42.0 * items.len() as f32;
                popup_rect = Some(content.r(x, y, 252.0, hgt));
                self.rect(ui, content, x, y, 252.0, hgt, 4.0, c(0x333333));
                for (n, (val, label)) in items.iter().enumerate() {
                    let iy = y + 6.0 + 42.0 * n as f32;
                    let h = self.hit(ui, content, ("choice", n), x, iy, 252.0, 42.0, false);
                    if h.hov {
                        self.rect(ui, content, x, iy, 252.0, 42.0, 4.0, white(0.2));
                    }
                    let col = if val == cur { ACCENT } else if h.hov { Color32::WHITE } else { white(0.5) };
                    self.text_al(ui, content, label, x + 16.0, iy + 21.0, 15.0, col, Align2::LEFT_CENTER);
                    if h.click {
                        item_clicked = true;
                        self.pick_choice(*act, *val);
                    }
                }
            }
            Modal::Tisp { msg, ok } => {
                self.rect(ui, win, 0.0, 0.0, 1080.0, 780.0, 0.0, Color32::from_black_alpha(153));
                let b = win.at((1080.0 - 520.0) / 2.0, (780.0 - 236.0) / 2.0);
                self.rect(ui, b, 0.0, 0.0, 520.0, 236.0, 0.0, c(0x262626));
                self.text_c(ui, b, msg, 55.0, 69.0, 410.0, 18.0, TEXT);
                if let Some(a) = ok {
                    if self.dlg_btn(ui, b, "Yes", 118.0, 163.0, false) {
                        keep = false;
                        self.run_act(a.clone());
                    }
                    if self.dlg_btn(ui, b, "Cancel", 272.0, 163.0, true) {
                        keep = false;
                    }
                } else if self.dlg_btn(ui, b, "OK", 195.0, 163.0, false) {
                    keep = false;
                }
            }
            Modal::OpenTisp(slot) => {
                self.rect(ui, win, 0.0, 0.0, 1080.0, 780.0, 0.0, Color32::from_black_alpha(153));
                let b = win.at((1080.0 - 508.0) / 2.0, (780.0 - 392.0) / 2.0);
                self.rect(ui, b, 0.0, 0.0, 508.0, 392.0, 0.0, c(0x262626));
                let t = self.s("getOpenProfile");
                self.text_c(ui, b, &t, 0.0, 55.0, 508.0, 18.0, Color32::WHITE);
                let suf = self.color_suffix();
                self.img(ui, b, &format!("platform/openplat_{}{suf}.png", slot + 1), (508.0 - 284.0) / 2.0, 103.0, 284.0, 170.0);
                if self.dlg_btn(ui, b, "OK", 189.0, 318.0, false) {
                    keep = false;
                }
            }
            Modal::Rename { text, act, focus } => {
                self.rect(ui, win, 0.0, 0.0, 1080.0, 780.0, 0.0, BG);
                let p = win.at(0.0, 17.0);
                let macro_name = matches!(act, RenameAct::MacroCreate | RenameAct::MacroRename(_) | RenameAct::MacroCopy(_));
                let t = self.s(if macro_name { "getMacros" } else { "getRenameProfile" });
                self.text_c(ui, p, &t, 0.0, 121.0, 1080.0, 32.0, Color32::WHITE);
                let mut s = text.clone();
                let hint = self.s("getProfileName");
                let r = ui.put(p.r(396.0, 280.0, 288.0, 34.0), egui::TextEdit::singleline(&mut s).frame(false).char_limit(16)
                    .font(FontId::proportional(p.s(24.0))).text_color(Color32::WHITE).horizontal_align(Align::Center)
                    .hint_text(egui::RichText::new(hint).color(white(0.5))).id(Id::new("rename")));
                if *focus {
                    r.request_focus();
                }
                // letras, números e chinês (RenameDialog)
                s = s.chars().filter(|c| c.is_ascii_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(c)).take(16).collect();
                self.rect(ui, p, 396.0, 314.0, 288.0, 1.0, 0.0, white(0.1));
                let t = self.s("getSupport");
                self.text_c(ui, p, &t, 0.0, 327.0, 1080.0, 12.0, white(0.6));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let yes = self.dlg_btn(ui, p, "Yes", 398.0, 376.0, false);
                let cancel = self.dlg_btn(ui, p, "Cancel", 556.0, 376.0, true);
                if (yes || enter) && !s.is_empty() {
                    self.run_rename(act.clone(), s);
                    keep = false;
                } else if cancel || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    keep = false;
                } else {
                    self.modal = Some(Modal::Rename { text: s, act: act.clone(), focus: false });
                    return;
                }
            }
            Modal::MapPick(ty) => {
                let ty = *ty;
                self.modal = Some(Modal::MapPick(ty));
                self.map_picker(ui, content, ty);
                return;
            }
            Modal::AxisPick => {
                self.modal = Some(Modal::AxisPick);
                self.axis_picker(ui, content);
                return;
            }
            Modal::MacroMenu { name, y, active, .. } => {
                let (r, clicked) = self.macro_menu(ui, content, *y, &name.clone(), *active);
                popup_rect = r;
                item_clicked = clicked;
            }
        }
        // menus suspensos fecham com qualquer clique (setGridShow do original)
        if m.is_popup() && self.frame > self.modal_frame {
            let clicked = ui.input(|i| i.pointer.any_click());
            if item_clicked || clicked {
                keep = false;
            }
            let _ = popup_rect;
        }
        if keep && self.modal.is_none() {
            self.modal = Some(m);
        } else if item_clicked && self.modal.is_some() {
            // o item abriu outro diálogo: fica com ele
        }
    }

    fn pick_choice(&mut self, act: ChoiceAct, v: u8) {
        match act {
            ChoiceAct::LedMode => {
                // Close_ModeDown & cia: 5 (ou 6 no MM) desliga, 2 Dynamic, 3 Hotwheel
                if v != self.led.mode {
                    let color = self.info.as_ref().map_or(0, |i| i.color);
                    self.set_theme(if v == 0 { if color == 2 { 6 } else { 5 } } else { v + 1 });
                }
                self.led.mode = v;
                self.led.color_type = 0;
            }
            ChoiceAct::LedColorType => self.led.color_type = v,
            ChoiceAct::AxisMapping => {
                let (c, i) = self.cur_mut();
                let mut sx = c.sixaxis(i);
                sx.mapping = v;
                if v != 0 && sx.trigger == 0 { sx.trigger = 1; }
                if v != 0 && sx.key == 0 { sx.key = 0x8000; }
                c.set_sixaxis(i, &sx);
                self.commit();
            }
            ChoiceAct::AxisTrigger => {
                let (c, i) = self.cur_mut();
                let mut sx = c.sixaxis(i);
                sx.trigger = v;
                c.set_sixaxis(i, &sx);
                self.commit();
            }
        }
    }

    /// Setting_Grid: 220 de largura, itens de 42 com ícone à direita
    fn menu(&mut self, ui: &Ui, f: Fr, x: f32, y: f32, items: &[(String, String)], enabled: bool) -> Option<usize> {
        self.rect(ui, f, x, y, 220.0, 8.0 + 42.0 * items.len() as f32, 4.0, c(0x333333));
        let mut out = None;
        for (n, (label, icon)) in items.iter().enumerate() {
            let iy = y + 4.0 + 42.0 * n as f32;
            let h = if enabled { self.hit(ui, f, ("menu", n), x + 4.0, iy, 212.0, 42.0, false) } else { H::default() };
            if h.hov {
                self.rect(ui, f, x + 4.0, iy, 212.0, 42.0, 4.0, c(0x717171));
            }
            let a = if enabled { 1.0 } else { 0.35 };
            self.text_al(ui, f, label, x + 20.0, iy + 21.0, 14.0, ca(0xD6D6D6, a), Align2::LEFT_CENTER);
            if icon.contains("plat_") {
                self.img_a(ui, f, icon, x + 216.0 - 3.0 - 42.0, iy, 42.0, 42.0, a);
            } else {
                self.img_a(ui, f, icon, x + 216.0 - 3.0 - 13.0 - 18.0, iy + 12.0, 18.0, 18.0, a);
            }
            if h.click {
                out = Some(n);
            }
        }
        out
    }

    /// Sure_Normal / Cancel_Normal: 130×40, raio 20
    fn dlg_btn(&mut self, ui: &Ui, f: Fr, label: &str, x: f32, y: f32, cancel: bool) -> bool {
        let h = self.hit(ui, f, ("dlg", label), x, y, 130.0, 40.0, false);
        self.rect(ui, f, x, y, 130.0, 40.0, 20.0, if h.hov { BLUE } else if cancel { c(0x2B2B2B) } else { c(0x333333) });
        self.text_al(ui, f, label, x + 65.0, y + 20.0, 18.0, Color32::WHITE, Align2::CENTER_CENTER);
        h.click
    }

    // ---------------------------------------------------------------- --shot (depuração)

    /// Leva a interface até a tela pedida em --shot
    fn apply_shot_target(&mut self) {
        let Some((target, _)) = self.shot.clone() else { return };
        match target.as_str() {
            "home" => {}
            "profiles" => self.screen = Screen::Profiles,
            "back" => {
                self.go_settings(Slot::Dev(0), Tab::Mapping);
                self.back = true;
            }
            "picker" => {
                self.go_settings(Slot::Dev(0), Tab::Mapping);
                self.modal = Some(Modal::MapPick("A"));
            }
            "ledcustom" => {
                self.go_settings(Slot::Dev(0), Tab::Led);
                self.led.custom = Some(true);
                self.led.keys = vec![0];
            }
            "rename" => {
                self.screen = Screen::Profiles;
                self.modal = Some(Modal::Rename { text: String::new(), act: RenameAct::Create, focus: true });
            }
            // aba Macros com a tecla A escolhida (lista da biblioteca) e o menu da 1ª macro
            "macrokey" | "macromenu" => {
                self.go_settings(Slot::Dev(0), Tab::Macro);
                self.shot_macro_key(target == "macromenu");
            }
            "macroback" => {
                self.go_settings(Slot::Dev(0), Tab::Macro);
                self.shot_macro_key(false);
                self.mac_back(true);
            }
            // tela de gravação com a primeira macro da biblioteca (ou vazia)
            "macroedit" => {
                self.go_settings(Slot::Dev(0), Tab::Macro);
                self.shot_macro_edit();
            }
            "macroname" => {
                self.go_settings(Slot::Dev(0), Tab::Macro);
                self.modal = Some(Modal::Rename { text: String::new(), act: RenameAct::MacroCreate, focus: true });
            }
            t => {
                if let Some(tab) = Tab::from_key(t) {
                    self.go_settings(Slot::Dev(0), tab);
                }
            }
        }
    }

    fn handle_shot(&mut self, ctx: &egui::Context) {
        let Some((_, out)) = self.shot.clone() else { return };
        let shot = ctx.input(|i| i.raw.events.iter().find_map(|e| match e {
            egui::Event::Screenshot { image, .. } => Some(image.clone()),
            _ => None,
        }));
        if let Some(img) = shot {
            let [w, h] = img.size;
            let raw: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
            match image::save_buffer(&out, &raw, w as u32, h as u32, image::ExtendedColorType::Rgba8) {
                Ok(_) => eprintln!("captura salva em {out}"),
                Err(e) => eprintln!("erro ao salvar {out}: {e}"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if !matches!(self.screen, Screen::Loading) {
            self.shot_frames += 1;
            if self.shot_frames == 20 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
            }
            ctx.request_repaint();
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame += 1;
        self.poll(ctx);
        egui::CentralPanel::default().frame(egui::Frame::none().fill(BG)).show(ctx, |ui| {
            self.frame_ui(ui);
        });
        self.handle_shot(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // fechou no meio de uma gravação de macro: devolve o input do controle (s_swichReport(1))
        if self.editor.as_ref().is_some_and(|e| e.recording()) {
            let _ = self.dev.tx.send(Cmd::Record(false));
        }
        let _ = self.dev.tx.send(Cmd::Live { on: false, vib: None });
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Arial do app original → Liberation Sans (métricas iguais); senão a fonte padrão do egui
fn setup_fonts(ctx: &egui::Context) {
    let candidates = ["/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
                      "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
                      "/usr/share/fonts/TTF/LiberationSans-Regular.ttf"];
    let mut fonts = egui::FontDefinitions::default();
    if let Some(data) = candidates.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts.font_data.insert("arial".into(), egui::FontData::from_owned(data));
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "arial".into());
    }
    // chinês nos nomes de perfil, se houver uma fonte CJK
    for p in ["/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"] {
        if let Ok(data) = std::fs::read(p) {
            fonts.font_data.insert("cjk".into(), egui::FontData::from_owned(data));
            fonts.families.entry(egui::FontFamily::Proportional).or_default().push("cjk".into());
            break;
        }
    }
    ctx.set_fonts(fonts);
    ctx.style_mut(|s| {
        s.visuals = egui::Visuals::dark();
        s.visuals.selection.bg_fill = BLUE;
        s.visuals.text_cursor.stroke.color = Color32::WHITE;
    });
    let _ = Vec2::ZERO;
}
