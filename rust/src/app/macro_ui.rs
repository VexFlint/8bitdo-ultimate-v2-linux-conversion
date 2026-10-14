//! Aba Macros e tela de gravação — mesma disposição do app oficial.
//! O modelo (ações, gravação, biblioteca) fica em crate::macros; aqui só a tela e o que cada clique faz no struct/controle.

use super::*;
use crate::config::{Macro, Step};
use crate::macros::{self, Entry, Item, Loop, Recorder, Saved};

/// Estado da aba (MacroView): tecla escolhida (type_temp), frente/verso, lista da biblioteca
#[derive(Default)]
pub(super) struct MacroTab {
    sel: Option<&'static str>,
    back: bool,
    /// nomes na biblioteca (None = reler do disco)
    files: Option<Vec<String>>,
}

/// Estado do MacroSetting
pub(super) struct Editor {
    name: String,
    entries: Vec<Entry>,
    mode: Loop,
    interval: String,
    count: String,
    uniform: bool,
    uniform_text: String,
    rec: Option<(Recorder, Instant)>,
    selected: Option<usize>,
    drag: Option<Drag>,
    popup: bool,
    /// texto das caixas de tempo em edição (por entrada)
    times: HashMap<usize, String>,
}

impl Editor {
    pub(super) fn recording(&self) -> bool {
        self.rec.is_some()
    }
}

#[derive(Clone, Copy)]
struct Drag {
    from: usize,
    start: (f32, f32),
    grab: (f32, f32),
    moved: bool,
}

/// área de blocos: ScrollViewer da linha 1 (y 219..638, margem 0,10,10,0)
const AREA: (f32, f32, f32, f32) = (0.0, 229.0, 1070.0, 409.0);
/// WindowWidth do RePaint (a página tem 1080 de largura)
const WRAP: f32 = 1080.0 - 108.0;

/// RePaint: posição (x, y, largura) de cada bloco visível
fn layout(items: &[Item], entries: &[Entry]) -> Vec<(f32, f32, f32)> {
    let (mut x, mut y) = (30.0, 30.0);
    items.iter().map(|it| {
        let (w, adv) = match it {
            Item::Act { entry, idx } if entries[*entry].actions[*idx].key.wide() => (72.0, 90.0),
            _ => (50.0, 68.0),
        };
        let r = (x, y, w);
        x += adv;
        if x > WRAP {
            x = 30.0;
            y += 95.0;
        }
        r
    }).collect()
}

/// getText (ramo Ultimate 2): nome da tecla no topo da lista
fn key_label(ty: &str) -> &'static str {
    match ty {
        "A" => "A", "B" => "B", "X" => "X", "Y" => "Y", "Up" => "Up", "Down" => "Down", "Left" => "Left", "Right" => "Right",
        "L3" => "L3", "R3" => "R3", "SELECT" => "SELECT", "START" => "START", "L" => "L", "R" => "R", "L2" => "L2", "R2" => "R2",
        "AS_P1" => "PR", "AS_P2" => "PL", "AS_P3" => "L4", "AS_P4" => "R4",
        _ => "",
    }
}

/// MacroDatas1..4: macros ativas do perfil, com o nome que o app usa ("配置N" quando vazio)
fn named_defs(cfg: &Config, i: usize) -> [Option<Macro>; 4] {
    let mut d = cfg.macro_defs(i);
    for (k, m) in d.iter_mut().enumerate() {
        if let Some(m) = m {
            if m.name.is_empty() {
                m.name = format!("配置{}", k + 1);
            }
        }
    }
    d
}

/// MappingView/MacroView.setMapping: imagem M1–M4 da tecla `ty`, se ela dispara uma macro.
/// O número é a posição do nome na lista dos 4 nomes (vazio quando não há), sem repetir.
pub(super) fn macro_badge(defs: &[Option<Macro>; 4], ty: &str) -> Option<String> {
    let mut names: Vec<String> = Vec::new();
    for d in defs {
        let n = d.as_ref().map_or(String::new(), |m| m.name.clone());
        if !names.contains(&n) {
            names.push(n);
        }
    }
    let m = defs.iter().flatten().find(|m| config::key_name(m.key) == ty)?;
    let idx = names.iter().position(|x| *x == m.name).unwrap_or(0);
    Some(format!("mapping/mapping_m{}_selection.png", idx + 1))
}
pub(super) fn macro_defs_named(cfg: &Config, i: usize) -> [Option<Macro>; 4] {
    named_defs(cfg, i)
}

fn num(s: &str, max: u32) -> u32 {
    s.trim().parse::<u32>().unwrap_or(0).min(max)
}

impl App {
    fn macro_mode(&self) -> u16 {
        self.cur().0.mode()
    }
    fn macro_files(&mut self) -> Vec<String> {
        if self.mac.files.is_none() {
            self.mac.files = Some(macros::list(self.macro_mode()));
        }
        self.mac.files.clone().unwrap_or_default()
    }

    /// grava struct + passos: no arquivo do perfil local ou no controle
    fn commit_macros(&mut self, writes: Vec<(usize, Vec<Step>)>) {
        self.mac.files = None;
        match &mut self.slot {
            Slot::File(f) => {
                for (k, s) in writes {
                    f.macros[k] = s;
                }
                self.commit();
            }
            Slot::Dev(i) => {
                let i = *i;
                for (k, s) in &writes {
                    self.steps[i][*k] = s.clone();
                }
                self.commit_macros_dev(i, writes);
            }
        }
    }
    pub(super) fn commit_macros_dev(&mut self, i: usize, writes: Vec<(usize, Vec<Step>)>) {
        if !self.write && !self.offline {
            self.status = "somente leitura — alteração não enviada".into();
            return;
        }
        let Some(cfg) = self.cfg.as_ref() else { return };
        let new = cfg.b.clone();
        let base = std::mem::replace(&mut self.base, new.clone());
        if base != new || !writes.is_empty() {
            self.status = "gravando…".into();
            let steps = writes.into_iter().map(|(k, s)| (i, k, s)).collect();
            let _ = self.dev.tx.send(Cmd::Macros { base, new, steps });
        }
    }

    /// resetMapping: a tecla que recebe (ou perde) a macro volta ao mapeamento padrão
    fn reset_key(c: &mut Config, i: usize, ty: &str) {
        if let Some(idx) = config::SLOTS.iter().position(|s| *s == ty) {
            let d = c.default_keys();
            let mut k = c.keys(i);
            k[idx] = d[idx];
            c.set_keys(i, &k);
        }
    }

    /// Load_Click: "Activate" põe a macro da biblioteca na tecla escolhida; "Deactivate" tira
    fn macro_toggle(&mut self, name: &str, active: bool) {
        let Some(sel) = self.mac.sel else { return };
        let (cfg, i) = self.cur();
        let mut defs = named_defs(&cfg, i);
        if active {
            // resetMacro1: tira a macro da tecla
            for d in defs.iter_mut() {
                if d.as_ref().is_some_and(|m| config::key_name(m.key) == sel) {
                    *d = None;
                }
            }
            let (c, i) = self.cur_mut();
            c.set_macros(i, &defs);
            Self::reset_key(c, i, sel);
            self.commit_macros(vec![]);
            return;
        }
        let Some(saved) = macros::load(self.macro_mode(), name) else { return };
        // o app pega o primeiro registro livre ou com a mesma tecla, nessa ordem; aqui o da mesma
        // tecla tem preferência para não deixar dois registros na mesma tecla
        let k = defs.iter().position(|m| m.as_ref().is_some_and(|m| config::key_name(m.key) == sel))
            .or_else(|| defs.iter().position(|m| m.is_none()));
        let Some(k) = k else {
            self.open_modal(Modal::Tisp { msg: self.s("getMaximumMacro"), ok: None });
            return;
        };
        let steps: Vec<Step> = saved.steps.into_iter().take(config::MACRO_MAX_STEPS).collect();
        if steps.is_empty() {
            self.open_modal(Modal::Tisp { msg: self.s("getCompleteMacro"), ok: None });
            return;
        }
        defs[k] = Some(Macro { name: name.to_string(), key: config::key(sel), steps: steps.len() as u16, cycles: saved.cycles,
                               interval: saved.interval });
        let (c, i) = self.cur_mut();
        c.set_macros(i, &defs);
        Self::reset_key(c, i, sel);
        self.commit_macros(vec![(k, steps)]);
    }

    /// MacroView.DeleteFile
    pub(super) fn delete_macro(&mut self, name: &str) {
        let (cfg, i) = self.cur();
        let mut defs = named_defs(&cfg, i);
        let mut keys = Vec::new();
        for d in defs.iter_mut() {
            if d.as_ref().is_some_and(|m| m.name == name) {
                keys.push(config::key_name(d.as_ref().unwrap().key));
                *d = None;
            }
        }
        macros::delete(self.macro_mode(), name);
        self.mac.files = None;
        if !keys.is_empty() {
            let (c, i) = self.cur_mut();
            c.set_macros(i, &defs);
            for ty in keys {
                Self::reset_key(c, i, ty);
            }
            self.commit_macros(vec![]);
        }
    }

    /// MacroRenameDialog.sure: criar, renomear ou copiar
    pub(super) fn macro_named(&mut self, act: RenameAct, v: String) {
        let mode = self.macro_mode();
        let exists = macros::list(mode).iter().any(|n| n.eq_ignore_ascii_case(&v));
        let same_name = matches!(&act, RenameAct::MacroRename(old) if *old == v);
        if exists && !same_name {
            self.open_modal(Modal::Tisp { msg: self.s("getHasProfile"), ok: None });
            return;
        }
        match act {
            RenameAct::MacroCreate => self.open_editor(v, None),
            RenameAct::MacroRename(old) => {
                if same_name {
                    return;
                }
                if let Err(e) = macros::rename(mode, &old, &v) {
                    self.status = format!("erro: {e}");
                    return;
                }
                let (cfg, i) = self.cur();
                let mut defs = named_defs(&cfg, i);
                let mut changed = false;
                for m in defs.iter_mut().flatten() {
                    if m.name == old {
                        m.name = v.clone();
                        changed = true;
                    }
                }
                self.mac.files = None;
                if changed {
                    let (c, i) = self.cur_mut();
                    c.set_macros(i, &defs);
                    self.commit_macros(vec![]);
                }
            }
            RenameAct::MacroCopy(src) => {
                if let Some(mut m) = macros::load(mode, &src) {
                    m.name = v;
                    if let Err(e) = macros::save(mode, &m) {
                        self.status = format!("erro: {e}");
                    }
                }
                self.mac.files = None;
            }
            _ => {}
        }
    }

    fn open_editor(&mut self, name: String, saved: Option<Saved>) {
        let (steps, cycles, interval) = saved.map_or((Vec::new(), 1, 0), |s| (s.steps, s.cycles, s.interval));
        let mode = macros::loop_mode(cycles, interval);
        let uni = macros::uniform_time(&steps).filter(|&t| t != 0);
        self.editor = Some(Editor {
            name,
            entries: macros::entries_from_steps(&steps),
            mode,
            interval: if mode == Loop::Once { "30".into() } else { interval.to_string() },
            count: if mode == Loop::Times { cycles.to_string() } else { "1".into() },
            uniform: uni.is_some(),
            uniform_text: uni.unwrap_or(30).to_string(),
            rec: None,
            selected: None,
            drag: None,
            popup: false,
            times: HashMap::new(),
        });
        self.screen = Screen::MacroEdit;
    }
    fn close_editor(&mut self) {
        if self.editor.as_ref().is_some_and(|e| e.rec.is_some()) {
            let _ = self.dev.tx.send(Cmd::Record(false));
        }
        self.editor = None;
        self.mac.files = None;
        self.screen = Screen::Settings;
    }

    /// Save_Macro → save(1): biblioteca + macros ativas com o mesmo nome
    fn editor_save(&mut self) {
        let Some(ed) = self.editor.as_ref() else { return };
        if ed.entries.is_empty() {
            self.open_modal(Modal::Tisp { msg: self.s("getCompleteMacro"), ok: None });
            return;
        }
        let uniform = ed.uniform.then(|| num(&ed.uniform_text, 60000) as u16);
        let mut steps = macros::steps_from_entries(&ed.entries, uniform);
        steps.truncate(config::MACRO_MAX_STEPS);
        let (cycles, interval) = match ed.mode {
            Loop::Once => (1, 0),
            Loop::Forever => (config::CYCLES_FOREVER, num(&ed.interval, 60000)),
            Loop::Times => (num(&ed.count, 99).max(1), num(&ed.interval, 60000)),
        };
        let name = ed.name.clone();
        if let Err(e) = macros::save(self.macro_mode(), &Saved { name: name.clone(), cycles, interval, steps: steps.clone() }) {
            self.status = format!("erro: {e}");
            return;
        }
        let (cfg, i) = self.cur();
        let mut defs = named_defs(&cfg, i);
        let mut writes = Vec::new();
        for (k, d) in defs.iter_mut().enumerate() {
            if let Some(m) = d.as_mut().filter(|m| m.name == name) {
                m.steps = steps.len() as u16;
                m.cycles = cycles;
                m.interval = interval;
                writes.push((k, steps.clone()));
            }
        }
        self.close_editor();
        if !writes.is_empty() {
            let (c, i) = self.cur_mut();
            c.set_macros(i, &defs);
            self.commit_macros(writes);
        }
    }

    /// Add(): começa ou para a gravação
    fn editor_record(&mut self) {
        let Some(ed) = self.editor.as_mut() else { return };
        if ed.rec.is_some() {
            ed.rec = None;
            let _ = self.dev.tx.send(Cmd::Record(false));
            return;
        }
        if macros::is_full(&ed.entries) {
            self.open_modal(Modal::Tisp { msg: self.s("getMaximumMacro"), ok: None });
            return;
        }
        ed.selected = None;
        ed.popup = false;
        ed.rec = Some((Recorder::new(!ed.entries.is_empty()), Instant::now()));
        let _ = self.dev.tx.send(Cmd::Record(true));
    }

    /// readKey: cada leitura do controle durante a gravação
    pub(super) fn record_keys(&mut self, k: &Keys, at: Instant) {
        let Some(ed) = self.editor.as_mut() else { return };
        let Some((rec, t0)) = ed.rec.as_mut() else { return };
        let ms = at.saturating_duration_since(*t0).as_millis() as u64;
        if rec.feed(k, ms, &mut ed.entries) && macros::is_full(&ed.entries) {
            ed.rec = None;
            let _ = self.dev.tx.send(Cmd::Record(false));
            self.open_modal(Modal::Tisp { msg: self.s("getMaximumMacro"), ok: None });
        }
    }

    // ---------------------------------------------------------------- aba Macros

    pub(super) fn macro_view(&mut self, ui: &Ui, body: Fr, cfg: &Config, i: usize) {
        let root = body.at(0.0, (590.0 - 608.0) / 2.0);
        let defs = named_defs(cfg, i);
        let macro_of = |ty: &str| defs.iter().flatten().find(|m| config::key_name(m.key) == ty);

        // controle à direita (Keyboard_Grid em x=318; imagem 480×370 centrada com margem -120)
        let kb = root.at(318.0, 0.0);
        let back = self.mac.back;
        let suf = self.color_suffix();
        let bg = if back { format!("mapping/back_ultimate2_{}{suf}.png", if self.variant() == 2 { 1 } else { 2 }) } else { format!("mapping/ultimate2_mapping{suf}.png") };
        self.img(ui, kb, &bg, 110.0, 59.0, 480.0, 370.0);
        // myCanvas: getUltimate2(-190, -148)
        for (n, &(pts, kp, ty, dir)) in U2_MAP.iter().enumerate() {
            if BACK_TYPES.contains(&ty) != back {
                continue;
            }
            let m = macro_of(ty);
            let target = cfg.target(i, ty);
            let highlight = m.is_some() || if ty.starts_with("AS_") { target != "N" } else { target != ty };
            let p: Vec<_> = pts.iter().map(|&(x, y)| (x - 190.0, y - 148.0)).collect();
            self.line(ui, kb, &p, 2.0, if highlight { ACCENT } else { c(0x4A4A4A) });
            let (bx, by) = (kp.0 - 190.0, kp.1 - 148.0);
            let lx = if dir == 'L' { bx - 45.0 } else { bx + 5.0 };
            let h = self.hit(ui, kb, ("mkey", n), lx, by - 20.0, 40.0, 40.0, false);
            let img = if let Some(b) = macro_badge(&defs, ty) {
                b
            } else {
                let state = if highlight { "selection" } else if h.hov || self.mac.sel == Some(ty) { "enter" } else { "normal" };
                format!("mapping/mapping_{}_{state}.png", key_name(target).to_lowercase())
            };
            self.img(ui, kb, &img, lx, by - 20.0, 40.0, 40.0);
            if h.click && self.mac.sel != Some(ty) {
                self.mac.sel = Some(ty);
                self.mac.files = None;
                self.scroll.remove("macrofiles");
            }
        }
        // Front / Back (235×32 centrado, 125 acima do fundo)
        let y = 608.0 - 125.0 - 32.0;
        for (x, key, on, val) in [(0.0, "getFront", !back, false), (139.0, "getBackMapping", back, true)] {
            let bx = (700.0 - 235.0) / 2.0 + x;
            let h = self.hit(ui, kb, ("mside", key), bx, y, 96.0, 32.0, false);
            self.rect(ui, kb, bx, y, 96.0, 32.0, 5.0, if on { BLUE } else { c(0x282828) });
            if h.hov {
                self.rect_line(ui, kb, bx + 1.0, y + 1.0, 94.0, 30.0, 5.0, 2.0, BLUE);
            }
            let t = self.s(key);
            self.text_al(ui, kb, &t, bx + 48.0, y + 16.0, 18.0, if on || h.hov { Color32::WHITE } else { c(0x939393) }, Align2::CENTER_CENTER);
            if h.click {
                self.mac.back = val;
            }
        }

        // lado esquerdo
        let Some(sel) = self.mac.sel else {
            let t = self.s("getSelectTisp");
            for (n, line) in t.lines().enumerate() {
                self.text(ui, root, line, 48.0, 64.0 + 23.0 * n as f32, 20.0, Color32::WHITE);
            }
            return;
        };
        self.text(ui, root, key_label(sel), 48.0, 48.0, 22.0, Color32::WHITE);
        // CreateMacro_grid 222×36 em (48, 84)
        let h = self.hit(ui, root, "mcreate", 48.0, 84.0, 222.0, 36.0, false);
        self.rect(ui, root, 48.0, 84.0, 222.0, 36.0, 18.0, if h.hov { BLUE } else { c(0x333333) });
        let t = self.s("getCreateMacro");
        self.text_al(ui, root, &t, 159.0, 102.0, 12.0, if h.hov { Color32::WHITE } else { ca(0xBBBBBB, 0.5) }, Align2::CENTER_CENTER);
        if h.click {
            self.open_modal(Modal::Rename { text: String::new(), act: RenameAct::MacroCreate, focus: true });
        }
        // lista da biblioteca: a macro da tecla (same) vem primeiro, em azul
        let mut files = self.macro_files();
        let mine = macro_of(sel).map(|m| m.name.clone());
        let same = mine.as_ref().and_then(|n| files.iter().position(|f| f == n));
        if let Some(p) = same {
            let f = files.remove(p);
            files.insert(0, f);
        }
        let inner = self.scroll_area(ui, root, "macrofiles", 0.0, 132.0, 286.0, 382.0, 44.0 * files.len() as f32);
        let menu_open = matches!(self.modal, Some(Modal::MacroMenu { .. }));
        for (k, name) in files.iter().enumerate() {
            let y = 44.0 * k as f32;
            let blue = same.is_some() && k == 0;
            let h = self.hit(ui, inner, ("mfile", k), 48.0, y, 176.0, 36.0, false);
            if h.hov {
                self.rect(ui, inner, 48.0, y, 176.0, 36.0, 4.0, white(0.2));
            } else {
                self.rect(ui, inner, 48.0, y + 35.0, 176.0, 1.0, 0.0, if blue { ACCENT } else { c(0x333333) });
            }
            let col = if h.hov { Color32::WHITE } else if blue { ACCENT } else { white(0.5) };
            self.text_al(ui, inner.boxed(60.0, y, 152.0, 36.0), name, 0.0, 18.0, 14.0, col, Align2::LEFT_CENTER);
            if h.click {
                let saved = macros::load(self.macro_mode(), name);
                self.open_editor(name.clone(), saved);
            }
            let hs = self.hit(ui, inner, ("mfileset", k), 230.0, y, 36.0, 36.0, false);
            let open = menu_open && matches!(self.modal, Some(Modal::MacroMenu { file, .. }) if file == k);
            self.img_a(ui, inner, if open { "platform/setting_selection.png" } else { "platform/setting_normal.png" }, 230.0, y, 36.0, 36.0,
                       if hs.hov || open { 1.0 } else { 0.6 });
            if !open {
                self.rect(ui, inner, 230.0, y + 35.0, 36.0, 1.0, 0.0, if hs.hov { Color32::WHITE } else { c(0x333333) });
            }
            if hs.click {
                // Grid_Setting_Down: Margin.Top da linha + 132 (o topo da lista), sem passar de 525
                let (_, mut top) = root.local(inner.p(0.0, y));
                if top + 176.0 > 525.0 {
                    top = 349.0;
                }
                self.open_modal(Modal::MacroMenu { file: k, name: name.clone(), y: top, active: blue });
            }
        }
    }

    /// Macro_File: Activate/Deactivate, Rename, Copy, Delete (220×176 em x = 272; `top` na raiz da aba)
    pub(super) fn macro_menu(&mut self, ui: &Ui, content: Fr, top: f32, name: &str, active: bool) -> (Option<Rect>, bool) {
        let root = content.at(0.0, 148.0 + (590.0 - 608.0) / 2.0);
        let x = 272.0;
        let rect = root.r(x, top, 220.0, 176.0);
        self.rect(ui, root, x, top, 220.0, 176.0, 4.0, c(0x333333));
        let items = [
            (self.s(if active { "getRemove" } else { "getActivate" }), "platform/jpplat"),
            (self.s("getRename"), "platform/rename"),
            (self.s("getCopy"), "platform/copy"),
            (self.s("getDelete"), "platform/delete"),
        ];
        let mut clicked = false;
        for (n, (label, icon)) in items.iter().enumerate() {
            let iy = top + 4.0 + 42.0 * n as f32;
            let h = self.hit(ui, root, ("mmenu", n), x + 4.0, iy, 212.0, 42.0, false);
            if h.hov {
                self.rect(ui, root, x + 4.0, iy, 212.0, 42.0, 4.0, c(0x717171));
            }
            self.text_al(ui, root, label, x + 20.0, iy + 21.0, 14.0, if h.hov { Color32::WHITE } else { TEXT }, Align2::LEFT_CENTER);
            let st = if h.hov { "enter" } else { "normal" };
            if n == 0 {
                self.img(ui, root, &format!("{icon}_{st}.png"), x + 180.0, iy + 9.0, 24.0, 24.0);
            } else {
                self.img_contain(ui, root, &format!("{icon}_{st}.png"), x + 182.0, iy + 12.0, 18.0, 18.0);
            }
            if h.click {
                clicked = true;
                self.modal = None;
                match n {
                    0 => self.macro_toggle(name, active),
                    1 => self.open_modal(Modal::Rename { text: name.to_string(), act: RenameAct::MacroRename(name.to_string()), focus: true }),
                    2 => {
                        let t: String = format!("{name}{}", self.s("getTranscript")).chars().take(16).collect();
                        self.open_modal(Modal::Rename { text: t, act: RenameAct::MacroCopy(name.to_string()), focus: true });
                    }
                    _ => self.open_modal(Modal::Tisp { msg: self.s("getDeleteMacro"), ok: Some(Act::DeleteMacro(name.to_string())) }),
                }
            }
        }
        (Some(rect), clicked)
    }

    // ---------------------------------------------------------------- gravação de macro

    pub(super) fn editor_view(&mut self, ui: &mut Ui, f: Fr) {
        if self.editor.is_none() {
            self.screen = Screen::Settings;
            return;
        }
        self.rect(ui, f, 0.0, 0.0, 1080.0, 738.0, 0.0, c(0x262626));
        let recording = self.editor.as_ref().is_some_and(|e| e.rec.is_some());
        if recording {
            ui.ctx().request_repaint_after(Duration::from_millis(30));
        }
        // voltar (as imagens são trocadas em relação ao resto do app)
        let h = self.hit(ui, f, "medback", 0.0, 0.0, 80.0, 80.0, false);
        self.img(ui, f, if h.hov { "mainpage/back_normal.png" } else { "mainpage/back_enter.png" }, 0.0, 0.0, 80.0, 80.0);
        let t = self.s("getBack");
        self.text_al(ui, f, &t, 40.0, 48.0, 16.0, if h.hov { Color32::WHITE } else { TEXT }, Align2::CENTER_TOP);
        if h.click && !recording {
            self.close_editor();
            return;
        }
        let name = self.editor.as_ref().unwrap().name.clone();
        self.text_al(ui, f.boxed(340.0, 30.0, 400.0, 24.0), &name, 200.0, 0.0, 16.0, Color32::WHITE, Align2::CENTER_TOP);

        self.editor_top(ui, f, recording);
        self.editor_blocks(ui, f, recording);

        // Clear / Save (92×46 e 191×46, 23 acima do fundo)
        let has = self.editor.as_ref().is_some_and(|e| !e.entries.is_empty());
        let alpha = if recording { 0.5 } else if has { 1.0 } else { 0.4 };
        for (x, w, label, save) in [(1080.0 - 267.0 - 92.0, 92.0, "Clear", false), (1080.0 - 51.0 - 191.0, 191.0, "Save", true)] {
            let y = 738.0 - 23.0 - 46.0;
            let h = self.hit(ui, f, ("mbtn", label), x, y, w, 46.0, false);
            let lit = h.hov && has && !recording;
            self.rect(ui, f, x, y, w, 46.0, 23.0, ca(if lit { 0x296785 } else { 0x333333 }, alpha));
            self.text_al(ui, f, label, x + w / 2.0, y + 13.0, 18.0, white(if lit { 1.0 } else { 0.5 } * alpha), Align2::CENTER_TOP);
            if h.click && !recording {
                if save {
                    self.editor_save();
                    return;
                }
                if let Some(ed) = self.editor.as_mut() {
                    ed.entries.clear();
                    ed.selected = None;
                    ed.times.clear();
                }
            }
        }
        // popup de modo por cima de tudo
        self.editor_mode_popup(ui, f);
    }

    /// Linha de cima: modo de repetição, intervalo/vezes, tempo uniforme, gravar e apagar
    fn editor_top(&mut self, ui: &mut Ui, f: Fr, recording: bool) {
        let ed = self.editor.as_ref().unwrap();
        let (mode, popup) = (ed.mode, ed.popup);
        // settimemain_grid 216×46 em (48, 127)
        let h = if popup { H::default() } else { self.hit(ui, f, "mloop", 48.0, 127.0, 216.0, 46.0, false) };
        let lit = h.hov && !recording;
        self.rect_line(ui, f, 48.5, 127.5, 215.0, 45.0, 4.0, 1.0, white(if lit { 1.0 } else { 0.6 }));
        let (icon, label) = match mode {
            Loop::Once => ("once", "getSingleloop"),
            Loop::Forever => ("infinite", "getRepeatLoop"),
            Loop::Times => ("edittime", "getMultipleloops"),
        };
        self.img(ui, f, &format!("macro/function/{icon}_{}.png", if lit { "enter" } else { "normal" }), 61.0, 137.5, 25.0, 25.0);
        self.img_a(ui, f, "selectplatform/pulldown.png", 225.0, 134.0, 32.0, 32.0, if lit { 1.0 } else { 0.6 });
        let t = self.s(label);
        self.text_al(ui, f, &t, 99.0, 150.0, 18.0, white(if lit { 1.0 } else { 0.6 }), Align2::LEFT_CENTER);
        if h.click && !recording {
            self.editor.as_mut().unwrap().popup = true;
        }

        // Interval [ms] (ms, repeat) [vezes] times — Grid em (284, 141)
        if mode != Loop::Once {
            let mut x = 284.0;
            let t = self.s("getInterval");
            x = self.text(ui, f, &t, x, 141.0, 18.0, Color32::WHITE).max.x;
            x = f.local(pos2(x, 0.0)).0;
            let v = self.editor.as_ref().unwrap().interval.clone();
            let (nx, nv) = self.num_box(ui, f, "minterval", x, 141.0, &v, 16.0, 60000, !recording);
            self.editor.as_mut().unwrap().interval = nv;
            let t = self.s(if mode == Loop::Times { "getRepeat" } else { "getms" });
            x = f.local(pos2(self.text(ui, f, &t, nx, 141.0, 20.0, Color32::WHITE).max.x, 0.0)).0;
            if mode == Loop::Times {
                let v = self.editor.as_ref().unwrap().count.clone();
                let (nx, nv) = self.num_box(ui, f, "mcount", x, 141.0, &v, 16.0, 99, !recording);
                self.editor.as_mut().unwrap().count = nv;
                let t = self.s("getTimes");
                self.text(ui, f, &t, nx, 141.0, 18.0, Color32::WHITE);
            }
        }

        // tempo uniforme: caixinha 29×29 em (48, 190) e texto em (88, 192)
        let uni = self.editor.as_ref().unwrap().uniform;
        let h = self.hit(ui, f, "muni", 48.0, 190.0, 29.0, 29.0, false);
        self.img(ui, f, if uni { "sticks/whitecheck_selection.png" } else { "sticks/whitecheck_normal.png" }, 48.0, 190.0, 29.0, 29.0);
        if h.click && !recording {
            let ed = self.editor.as_mut().unwrap();
            ed.uniform = !ed.uniform;
        }
        let t = self.s("getUniformTime");
        let x = f.local(pos2(self.text(ui, f, &t, 88.0, 192.0, 20.0, Color32::WHITE).max.x, 0.0)).0;
        let v = self.editor.as_ref().unwrap().uniform_text.clone();
        let (nx, nv) = self.num_box(ui, f, "muniv", x, 192.0, &v, 18.0, 60000, uni && !recording);
        self.editor.as_mut().unwrap().uniform_text = nv;
        let t = self.s("getms");
        self.text(ui, f, &t, nx, 192.0, 20.0, Color32::WHITE);

        // gravar / parar (138×46, 51 da direita) e apagar (207 da direita)
        let full = self.editor.as_ref().is_some_and(|e| macros::is_full(&e.entries));
        let h = self.hit(ui, f, "mrec", 891.0, 127.0, 138.0, 46.0, false);
        let st = if h.hov && !full { "enter" } else { "normal" };
        self.img(ui, f, &format!("macro/{}en_{st}.png", if recording { "stop" } else { "record" }), 891.0, 127.0, 138.0, 46.0);
        if h.click {
            self.editor_record();
        }
        let sel = self.editor.as_ref().unwrap().selected;
        if let (Some(s), false) = (sel, recording) {
            let h = self.hit(ui, f, "mdel", 735.0, 127.0, 138.0, 46.0, false);
            self.img(ui, f, if h.hov { "macro/deleteen_enter.png" } else { "macro/deleteen_normal.png" }, 735.0, 127.0, 138.0, 46.0);
            if h.click {
                self.editor_delete(s);
            }
        }
    }

    /// TextBox numérico sobre um sublinhado (Border com margem 5, TextBox com margem 10).
    /// Devolve (x depois da caixa, texto).
    fn num_box(&mut self, ui: &mut Ui, f: Fr, id: &'static str, x: f32, y: f32, v: &str, size: f32, max: u32, enabled: bool) -> (f32, String) {
        let tw = ui.painter().layout_no_wrap(v.to_string(), FontId::proportional(f.s(size)), Color32::WHITE).size().x / f.k;
        let w = tw.max(10.0) + 20.0;
        self.rect(ui, f, x + 5.0, y + 20.0, w, 1.0, 0.0, Color32::WHITE);
        let mut s = v.to_string();
        if enabled && !self.blocked {
            ui.put(f.r(x + 5.0, y - 1.0, w, size + 4.0), egui::TextEdit::singleline(&mut s).frame(false).char_limit(5)
                .font(FontId::proportional(f.s(size))).text_color(Color32::WHITE).horizontal_align(Align::Center).id(Id::new(id)));
            s.retain(|c| c.is_ascii_digit());
            if !s.is_empty() && num(&s, u32::MAX) > max {
                s = max.to_string();
            }
        } else {
            self.text_al(ui, f, v, x + 5.0 + w / 2.0, y, size, white(if enabled { 1.0 } else { 0.5 }), Align2::CENTER_TOP);
        }
        (x + w + 10.0, s)
    }

    /// popup do modo (changemode_grid 216×134 em (48, 179))
    fn editor_mode_popup(&mut self, ui: &Ui, f: Fr) {
        if !self.editor.as_ref().is_some_and(|e| e.popup) {
            return;
        }
        self.rect(ui, f, 48.0, 179.0, 216.0, 134.0, 4.0, c(0x333333));
        let mut picked = None;
        for (n, (icon, label, mode)) in [("once", "getSingleloop", Loop::Once), ("infinite", "getRepeatLoop", Loop::Forever),
                                         ("edittime", "getMultipleloops", Loop::Times)].into_iter().enumerate() {
            let y = 181.0 + 44.0 * n as f32;
            let h = self.hit(ui, f, ("mmode", n), 50.0, y, 212.0, 42.0, false);
            if h.hov {
                self.rect(ui, f, 50.0, y, 212.0, 42.0, 4.0, white(0.3));
            }
            self.img(ui, f, &format!("macro/function/{icon}_{}.png", if h.hov { "enter" } else { "normal" }), 58.0, y + 5.0, 32.0, 32.0);
            let t = self.s(label);
            self.text_al(ui, f, &t, 102.0, y + 21.0, 14.0, white(if h.hov { 1.0 } else { 0.6 }), Align2::LEFT_CENTER);
            if h.click {
                picked = Some(mode);
            }
        }
        let clicked_out = ui.input(|i| i.pointer.any_click()) && !ui.rect_contains_pointer(f.r(48.0, 127.0, 216.0, 186.0));
        let ed = self.editor.as_mut().unwrap();
        if let Some(m) = picked {
            ed.popup = false;
            // Infinite_Down / EditTime_Down: intervalo volta a 30 e vezes a 1
            if m != ed.mode {
                ed.mode = m;
                ed.interval = "30".into();
                ed.count = "1".into();
            }
        } else if clicked_out {
            ed.popup = false;
        }
    }

    fn editor_delete(&mut self, s: usize) {
        let ed = self.editor.as_mut().unwrap();
        let items = macros::items(&ed.entries);
        if let Some(&it) = items.get(s) {
            macros::remove_item(&mut ed.entries, it);
        }
        ed.times.clear();
        // Delete_KeyDown: movetime-- (fica no bloco anterior)
        ed.selected = s.checked_sub(1).filter(|&p| p < macros::items(&ed.entries).len());
    }

    /// Linha do meio: os blocos de ação e de tempo (RePaint), seleção, arrastar e editar tempos
    fn editor_blocks(&mut self, ui: &mut Ui, f: Fr, recording: bool) {
        let ed = self.editor.as_ref().unwrap();
        let (entries, selected, drag, uniform) = (ed.entries.clone(), ed.selected, ed.drag, ed.uniform);
        let uniform_text = ed.uniform_text.clone();
        if entries.is_empty() && !recording {
            let t = self.s("getRecordTisp");
            self.text_al(ui, f, &t, 540.0, 219.0 + 419.0 / 2.0, 20.0, white(0.6), Align2::CENTER_CENTER);
        }
        let items = macros::items(&entries);
        let pos = layout(&items, &entries);
        let content = pos.last().map_or(0.0, |p| p.1 + 45.0 + 30.0);
        // durante a gravação acompanha o fim (ScrollToBottom)
        if recording {
            self.scroll.insert("mblocks", f32::MAX);
        }
        let inner = self.scroll_area(ui, f, "mblocks", AREA.0, AREA.1, AREA.2, AREA.3, content);
        let pointer = ui.input(|i| i.pointer.interact_pos()).map(|p| inner.local(p));
        let area_rect = f.r(AREA.0, AREA.1, AREA.2, AREA.3);
        let in_area = !self.blocked && ui.rect_contains_pointer(area_rect);
        let pressed = ui.input(|i| i.pointer.primary_pressed());
        let down = ui.input(|i| i.pointer.primary_down());
        let released = ui.input(|i| i.pointer.primary_released());
        let mut new_drag = drag;
        let mut new_sel = selected;
        let mut moved_to = None;

        // destino do arraste (inGrid2: em cima do bloco i → i; até 78 px à direita → i + 1)
        let target = |p: (f32, f32), from: usize| -> Option<usize> {
            for (k, &(x, y, w)) in pos.iter().enumerate() {
                if p.1 > y && p.1 < y + 45.0 && k != from {
                    if p.0 > x && p.0 < x + w {
                        return Some(k);
                    }
                    if p.0 - (x + w) >= 0.0 && p.0 - (x + w) <= 78.0 {
                        return Some(k + 1);
                    }
                }
            }
            None
        };

        if !recording && in_area && pressed {
            if let Some(p) = pointer {
                let hit = pos.iter().enumerate().find(|&(k, &(x, y, w))| {
                    let top = if matches!(items[k], Item::Time { .. }) { y + 20.0 } else { y }; // a caixa de texto fica em cima
                    p.0 > x && p.0 < x + w && p.1 > top && p.1 < y + 45.0
                });
                if let Some((k, &(x, y, _))) = hit {
                    new_drag = Some(Drag { from: k, start: p, grab: (p.0 - x, p.1 - y), moved: false });
                } else if !pos.iter().any(|&(x, y, w)| p.0 > x && p.0 < x + w && p.1 > y && p.1 < y + 45.0) {
                    // Grid1_Down: clique fora dos blocos tira a seleção
                    new_sel = None;
                }
            }
        }
        if let (Some(mut d), Some(p)) = (new_drag, pointer) {
            if down {
                if (p.0 - d.start.0).abs() > 20.0 || (p.1 - d.start.1).abs() > 20.0 {
                    d.moved = true;
                }
                new_drag = Some(d);
            }
            if released || !down {
                if d.moved {
                    moved_to = target(p, d.from).map(|t| (d.from, t));
                    new_sel = None;
                } else {
                    new_sel = Some(d.from);
                }
                new_drag = None;
            }
        }

        let dragging = new_drag.filter(|d| d.moved);
        let drop = dragging.and_then(|d| pointer.and_then(|p| target(p, d.from)));
        let mut time_changes = Vec::new();
        let mut blank_time = false;
        for (k, (it, &(x, y, w))) in items.iter().zip(&pos).enumerate() {
            if dragging.is_some_and(|d| d.from == k) {
                continue;
            }
            // o bloco no lugar do destino e os seguintes da linha andam 60 px (Grid_Move)
            let shift = match drop {
                Some(t) if t <= k && pos.get(t).is_some_and(|p| p.1 == y) => 60.0,
                _ => 0.0,
            };
            let x = x + shift;
            let sel = new_sel == Some(k);
            match *it {
                Item::Act { entry, idx } => {
                    let a = entries[entry].actions[idx];
                    self.img(ui, inner, &a.image(sel), x, y, w, 45.0);
                }
                Item::Time { entry } => {
                    self.img(ui, inner, if sel { "macro/timeen_selection.png" } else { "macro/timeen_normal.png" }, x, y, 45.0, 45.0);
                    let cur = self.editor.as_ref().unwrap().times.get(&entry).cloned();
                    let shown = if uniform { uniform_text.clone() } else { cur.clone().unwrap_or_else(|| entries[entry].time.to_string()) };
                    let r = inner.r(x + 2.0, y + 2.0, 41.0, 18.0);
                    if !uniform && !recording && !self.blocked && dragging.is_none() && inner.clip.contains_rect(r) {
                        let mut s = shown.clone();
                        let resp = ui.put(r, egui::TextEdit::singleline(&mut s).frame(false).char_limit(5)
                            .font(FontId::proportional(inner.s(18.0))).text_color(Color32::WHITE).horizontal_align(Align::Center)
                            .id(Id::new(("mtime", entry))));
                        s.retain(|c| c.is_ascii_digit());
                        if num(&s, u32::MAX) > 60000 {
                            s = "60000".into();
                        }
                        if resp.changed() {
                            self.editor.as_mut().unwrap().times.insert(entry, s.clone());
                        }
                        if resp.lost_focus() {
                            // settime: vazio não vale (getInterValNull)
                            if s.is_empty() {
                                blank_time = true;
                            } else {
                                time_changes.push((entry, num(&s, 60000)));
                            }
                            self.editor.as_mut().unwrap().times.remove(&entry);
                        }
                    } else {
                        self.text_al(ui, inner.boxed(x + 2.0, y + 2.0, 41.0, 18.0), &shown, 20.5, 0.0, 18.0, Color32::WHITE, Align2::CENTER_TOP);
                    }
                }
            }
        }
        // destino (blank_Button: borda azul) e o bloco arrastado por cima
        if let (Some(d), Some(p)) = (dragging, pointer) {
            if let Some(t) = drop {
                let (tx, ty) = pos.get(t).map_or_else(|| { let l = pos[pos.len() - 1]; (l.0 + 68.0, l.1) }, |q| (q.0, q.1));
                self.rect_line(ui, inner, tx, ty, 50.0, 45.0, 4.0, 1.0, BLUE);
            }
            let (x, y) = (p.0 - d.grab.0, p.1 - d.grab.1);
            match items[d.from] {
                Item::Act { entry, idx } => {
                    let a = entries[entry].actions[idx];
                    self.img(ui, inner, &a.image(false), x, y, pos[d.from].2, 45.0);
                }
                Item::Time { entry } => {
                    self.img(ui, inner, "macro/timeen_normal.png", x, y, 45.0, 45.0);
                    let t = entries[entry].time.to_string();
                    self.text_al(ui, inner, &t, x + 22.5, y + 2.0, 18.0, Color32::WHITE, Align2::CENTER_TOP);
                }
            }
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if in_area && !recording && pointer.is_some_and(|p| pos.iter().any(|&(x, y, w)| p.0 > x && p.0 < x + w && p.1 > y && p.1 < y + 45.0)) {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }

        // teclas Delete / Backspace apagam o bloco escolhido (fora das caixas de texto)
        let typing = ui.ctx().wants_keyboard_input();
        let del = !typing && !self.blocked && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
        let ed = self.editor.as_mut().unwrap();
        ed.drag = new_drag;
        ed.selected = new_sel.filter(|&s| s < items.len());
        for (e, t) in time_changes {
            if let Some(en) = ed.entries.get_mut(e) {
                en.time = t;
            }
        }
        if let Some((from, to)) = moved_to {
            macros::move_item(&mut ed.entries, from, to);
            ed.times.clear();
        }
        if blank_time {
            self.open_modal(Modal::Tisp { msg: self.s("getInterValNull"), ok: None });
        }
        if del && !recording {
            if let Some(s) = self.editor.as_ref().unwrap().selected {
                self.editor_delete(s);
            }
        }
    }
}

// ------------------------------------------------------------------ --shot
impl App {
    pub(super) fn shot_macro_key(&mut self, menu: bool) {
        self.mac.sel = Some("A");
        if menu {
            if let Some(name) = self.macro_files().first().cloned() {
                self.modal = Some(Modal::MacroMenu { file: 0, name, y: 132.0, active: false });
            }
        }
    }
    pub(super) fn mac_back(&mut self, back: bool) {
        self.mac.back = back;
    }
    pub(super) fn shot_macro_edit(&mut self) {
        let mode = self.macro_mode();
        match self.macro_files().first().cloned() {
            Some(n) => {
                let saved = macros::load(mode, &n);
                self.open_editor(n, saved);
            }
            None => self.open_editor("Macro1".into(), None),
        }
        if let Some(ed) = self.editor.as_mut() {
            ed.selected = Some(1);
        }
    }
}
