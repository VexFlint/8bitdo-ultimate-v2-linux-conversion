//! Macros do Ultimate 2.
//!
//! O controle guarda cada macro como uma lista de passos (`config::Step`): o estado completo dos
//! botões e sticks e quanto tempo segurar. O editor do app mostra outra coisa: uma fila de ações
//! "aperta X" / "solta X" separadas por tempos. Aqui ficam as duas conversões, o gravador
//! (readKey + compareKey + AddMacro) e a biblioteca local de macros (MacroData/Controller/...).

use crate::config::{CYCLES_FOREVER, MACRO_MAX_STEPS, Step};
use crate::proto::Keys;
use std::path::PathBuf;

/// Teclas que entram numa macro, na ordem de compareKeys (decide a ordem das ações ao abrir).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    A, B, X, Y, L1, L2, L3, R1, R2, R3, Up, Down, Left, Right,
    LsUp, LsDown, LsLeft, LsRight, RsUp, RsDown, RsLeft, RsRight, Select, Start,
}
pub const ALL_KEYS: [Key; 24] = [Key::A, Key::B, Key::X, Key::Y, Key::L1, Key::L2, Key::L3, Key::R1, Key::R2, Key::R3,
    Key::Up, Key::Down, Key::Left, Key::Right, Key::LsUp, Key::LsDown, Key::LsLeft, Key::LsRight,
    Key::RsUp, Key::RsDown, Key::RsLeft, Key::RsRight, Key::Select, Key::Start];

impl Key {
    /// bit em Step.keys (getMapKey), 0 para direções de stick
    pub fn button(self) -> u16 {
        match self {
            Key::A => 0x2000, Key::B => 0x1000, Key::X => 0x10, Key::Y => 0x20, Key::L1 => 0x400, Key::R1 => 0x800,
            Key::L2 => 0x4000, Key::R2 => 0x8000, Key::L3 => 0x2, Key::R3 => 0x4, Key::Up => 0x200, Key::Down => 0x100,
            Key::Left => 0x80, Key::Right => 0x40, Key::Select => 0x8, Key::Start => 0x1,
            _ => 0,
        }
    }
    /// bit de direção de stick (getStick): LS 1 cima, 2 baixo, 4 esquerda, 8 direita; RS ×0x10
    pub fn stick(self) -> u8 {
        match self {
            Key::LsUp => 1, Key::LsDown => 2, Key::LsLeft => 4, Key::LsRight => 8,
            Key::RsUp => 0x10, Key::RsDown => 0x20, Key::RsLeft => 0x40, Key::RsRight => 0x80,
            _ => 0,
        }
    }
    /// nome da imagem em resource/macro/ (MacroImageEngineer.getMacroImageString)
    pub fn image(self) -> &'static str {
        match self {
            Key::A => "a", Key::B => "b", Key::X => "x", Key::Y => "y", Key::L1 => "l", Key::R1 => "r",
            Key::L2 => "l2", Key::R2 => "r2", Key::L3 => "l3", Key::R3 => "r3", Key::Up => "up", Key::Down => "down",
            Key::Left => "left", Key::Right => "right", Key::LsUp => "lup", Key::LsDown => "ldown", Key::LsLeft => "lleft",
            Key::LsRight => "lright", Key::RsUp => "rup", Key::RsDown => "rdown", Key::RsLeft => "rleft",
            Key::RsRight => "rright", Key::Select => "select", Key::Start => "start",
        }
    }
    /// isMouseDirect: as ações de stick ocupam 72 px em vez de 50
    pub fn wide(self) -> bool {
        self.stick() != 0
    }
}

/// MacroAction: aperta (down) ou solta uma tecla
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Action {
    pub key: Key,
    pub down: bool,
}
impl Action {
    pub fn image(self, selected: bool) -> String {
        format!("macro/{}_{}_{}.png", self.key.image(), if self.down { "down" } else { "up" },
                if selected { "selection" } else { "normal" })
    }
}

/// Um item de macroList: as ações de um instante e o tempo até o próximo (Time[0]).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Entry {
    pub actions: Vec<Action>,
    pub time: u32,
}

/// Teclas apertadas num passo (setKey + setLeftStick/setRightStick + getKeys, que separa as diagonais)
fn pressed(s: &Step) -> Vec<Key> {
    let mut v: Vec<Key> = ALL_KEYS.iter().copied().filter(|k| k.button() != 0 && s.keys & k.button() != 0).collect();
    for (val, base) in [(s.left, 0u8), (s.right, 4)] {
        let dirs: &[u8] = match val {
            127 => &[0],             // cima
            65407 => &[1],           // baixo
            32512 => &[2],           // esquerda
            32767 => &[3],           // direita
            0 => &[2, 0],            // esquerda-cima
            255 => &[3, 0],          // direita-cima
            65280 => &[2, 1],        // esquerda-baixo
            65535 => &[3, 1],        // direita-baixo
            _ => &[],
        };
        for &d in dirs {
            v.push([Key::LsUp, Key::LsDown, Key::LsLeft, Key::LsRight, Key::RsUp, Key::RsDown, Key::RsLeft, Key::RsRight][(base + d) as usize]);
        }
    }
    v
}

/// MacroSetting.RefreshData: passos gravados → fila de ações do editor
pub fn entries_from_steps(steps: &[Step]) -> Vec<Entry> {
    let mut prev: Vec<Key> = Vec::new();
    steps.iter().map(|s| {
        let now = pressed(s);
        let mut actions: Vec<Action> = ALL_KEYS.iter().filter(|k| now.contains(k) && !prev.contains(k))
            .map(|&key| Action { key, down: true }).collect();
        actions.extend(ALL_KEYS.iter().filter(|k| !now.contains(k) && prev.contains(k)).map(|&key| Action { key, down: false }));
        prev = now;
        Entry { actions, time: s.ms as u32 }
    }).collect()
}

/// getSticks + getLeftStick/getRightStick: bits de direção (1 cima, 2 baixo, 4 esq., 8 dir.) → valor do stick
fn stick_value(bits: u8) -> u16 {
    let mut b = bits & 0xF;
    let mut found = Vec::new();
    loop {
        let d = if b & 1 != 0 {
            if b & 4 != 0 { b &= !5; "lu" } else if b & 8 != 0 { b &= !9; "ru" } else { b &= !1; "u" }
        } else if b & 2 != 0 {
            if b & 4 != 0 { b &= !6; "ld" } else if b & 8 != 0 { b &= !10; "rd" } else { b &= !2; "d" }
        } else if b & 4 != 0 {
            b &= !4;
            "l"
        } else if b & 8 != 0 {
            b &= !8;
            "r"
        } else {
            break;
        };
        found.push(d);
    }
    // getLeftStick: o último "if" que casar ganha
    let mut v = Step::CENTER;
    for (d, val) in [("u", 127), ("d", 65407), ("l", 32512), ("r", 32767), ("lu", 0), ("ru", 255), ("ld", 65280), ("rd", 65535)] {
        if found.contains(&d) {
            v = val;
        }
    }
    v
}

/// MacroSetting.RefreshMacro + settingMacro: fila do editor → passos.
/// `uniform`: "Use uniform interval time" ligado (todo tempo diferente de 0 vira esse valor).
/// L2/R2 vão como bits em `keys`: o app decide pelo gamepad_mode de UltimateBTAdvance, que no
/// Ultimate 2 nunca é lido (fica 0), então trigger_value sai sempre 0.
pub fn steps_from_entries(entries: &[Entry], uniform: Option<u16>) -> Vec<Step> {
    let (mut keys, mut sticks) = (0u16, 0u8);
    entries.iter().map(|e| {
        for a in &e.actions {
            if a.down {
                keys |= a.key.button();
                sticks |= a.key.stick();
            } else {
                keys &= !a.key.button();
                sticks &= !a.key.stick();
            }
        }
        let mut ms = e.time.min(u16::MAX as u32) as u16;
        if let Some(u) = uniform {
            if ms != 0 {
                ms = u;
            }
        }
        Step { ms, keys, trigger: 0, left: stick_value(sticks & 0xF), right: stick_value(sticks >> 4) }
    }).collect()
}

/// Modo de repetição do editor (circul): 0 uma vez, 1 repetir, 2 N vezes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loop {
    Once,
    Forever,
    Times,
}
/// MacroSetting.setnum
pub fn loop_mode(cycles: u32, interval: u32) -> Loop {
    if cycles == 1 && interval == 0 {
        Loop::Once
    } else if cycles == CYCLES_FOREVER {
        Loop::Forever
    } else {
        Loop::Times
    }
}

/// setnum: se todos os tempos (fora o último e os zeros) forem iguais, liga o tempo uniforme
pub fn uniform_time(steps: &[Step]) -> Option<u16> {
    let first = steps.first()?;
    let t = if first.ms == 0 { steps.get(1).map_or(0, |s| s.ms) } else { first.ms };
    let n = steps.len();
    steps.iter().enumerate().all(|(k, s)| s.ms == t || k == n - 1 || s.ms == 0).then_some(t)
}

// ------------------------------------------------------------------ gravação

/// Estado do readKey/compareKey/RefreshList do MacroSetting
pub struct Recorder {
    last: [i32; 8],
    start_ms: u64,
    append: bool,
}
const MIN_STICK: i32 = 67;
const MAX_STICK: i32 = 187;

impl Recorder {
    /// `append`: já havia passos (hasmacro = 1); a primeira mudança abre uma entrada nova com tempo 0
    pub fn new(append: bool) -> Recorder {
        Recorder { last: [0, 0, 127, 127, 127, 127, 0, 0], start_ms: 0, append }
    }

    /// (LT, RT, LX, LY, RX, RY, botões byte 0, byte 1) — mesmos índices de readByte no app
    fn sample(k: &Keys) -> [i32; 8] {
        [k.lt as i32, k.rt as i32, k.lx as i32, k.ly as i32, k.rx as i32, k.ry as i32,
         (k.buttons & 0xFF) as i32, ((k.buttons >> 8) & 0xFF) as i32]
    }

    /// compareKey: houve mudança que vira ação?
    fn changed(&self, s: &[i32; 8]) -> bool {
        let l = &self.last;
        let trig = |v: i32, lv: i32, bit: i32| {
            let on = s[7] & bit != 0 || v != 0;
            let was = l[7] & bit != 0 || lv != 0;
            on != was
        };
        if trig(s[0], l[0], 0x40) || trig(s[1], l[1], 0x80) {
            return true;
        }
        if s[6] != l[6] || (s[7] & 0x3F) != (l[7] & 0x3F) {
            return true;
        }
        (2..6).any(|a| {
            let (v, lv) = (s[a], l[a]);
            (v < MIN_STICK && lv > MIN_STICK) || (v > MAX_STICK && lv < MAX_STICK)
                || (v < MAX_STICK && lv > MAX_STICK) || (v > MIN_STICK && lv < MIN_STICK)
        })
    }

    /// AddMacro: as ações entre a amostra anterior e esta, na ordem do app
    fn actions(&self, s: &[i32; 8]) -> Vec<Action> {
        let l = &self.last;
        let mut v = Vec::new();
        let mut push = |key: Key, down: bool| v.push(Action { key, down });
        let (b1, b2, lb1, lb2) = (s[6], s[7], l[6], l[7]);
        for (val, last, bit, key) in [(s[0], l[0], 0x40, Key::L2), (s[1], l[1], 0x80, Key::R2)] {
            if (b2 & bit != 0 || val != 0) && lb2 & bit == 0 && last == 0 {
                push(key, true);
            }
            if (last != 0 || lb2 & bit != 0) && b2 & bit == 0 && val == 0 {
                push(key, false);
            }
        }
        // sticks: X (direita/esquerda) e Y (baixo/cima) de cada lado
        for (xi, yi, [up, down, left, right]) in [(2, 3, [Key::LsUp, Key::LsDown, Key::LsLeft, Key::LsRight]),
                                                  (4, 5, [Key::RsUp, Key::RsDown, Key::RsLeft, Key::RsRight])] {
            let (x, lx, y, ly) = (s[xi], l[xi], s[yi], l[yi]);
            if x < MAX_STICK && lx > MAX_STICK { push(right, false); }
            if x > MIN_STICK && lx < MIN_STICK { push(left, false); }
            if x < MIN_STICK && lx > MIN_STICK { push(left, true); }
            if x > MAX_STICK && lx < MAX_STICK { push(right, true); }
            if y < MAX_STICK && ly > MAX_STICK { push(down, false); }
            if y > MIN_STICK && ly < MIN_STICK { push(up, false); }
            if y < MIN_STICK && ly > MIN_STICK { push(up, true); }
            if y > MAX_STICK && ly < MAX_STICK { push(down, true); }
        }
        let mut edge = |now: i32, before: i32, bit: i32, key: Key| {
            if now & bit != 0 && before & bit == 0 {
                push(key, true);
            }
            if now & bit == 0 && before & bit != 0 {
                push(key, false);
            }
        };
        edge(b1, lb1, 2, Key::L3);
        edge(b1, lb1, 4, Key::R3);
        edge(b2, lb2, 4, Key::L1);
        edge(b2, lb2, 8, Key::R1);
        edge(b1, lb1, 1, Key::Start);
        edge(b1, lb1, 8, Key::Select);
        // direcional: primeiro os apertos de cima/baixo, depois as solturas (como no app)
        if b2 & 2 != 0 && lb2 & 2 == 0 { push(Key::Up, true); }
        if b2 & 1 != 0 && lb2 & 1 == 0 { push(Key::Down, true); }
        if b2 & 2 == 0 && lb2 & 2 != 0 { push(Key::Up, false); }
        if b2 & 1 == 0 && lb2 & 1 != 0 { push(Key::Down, false); }
        if b1 & 0x80 != 0 && lb1 & 0x80 == 0 { push(Key::Left, true); }
        if b1 & 0x40 != 0 && lb1 & 0x40 == 0 { push(Key::Right, true); }
        if b1 & 0x80 == 0 && lb1 & 0x80 != 0 { push(Key::Left, false); }
        if b1 & 0x40 == 0 && lb1 & 0x40 != 0 { push(Key::Right, false); }
        let mut edge = |now: i32, before: i32, bit: i32, key: Key| {
            if now & bit != 0 && before & bit == 0 {
                v.push(Action { key, down: true });
            }
            if now & bit == 0 && before & bit != 0 {
                v.push(Action { key, down: false });
            }
        };
        edge(b1, lb1, 0x20, Key::Y);
        edge(b1, lb1, 0x10, Key::X);
        edge(b2, lb2, 0x20, Key::A);
        edge(b2, lb2, 0x10, Key::B);
        v
    }

    /// Uma leitura do controle (cada ~10 ms). Devolve true se a lista mudou.
    /// O tempo da entrada anterior é o intervalo até esta mudança (Environment.TickCount).
    pub fn feed(&mut self, k: &Keys, now_ms: u64, entries: &mut Vec<Entry>) -> bool {
        let s = Self::sample(k);
        if !self.changed(&s) {
            return false;
        }
        if self.append {
            self.append = false;
        } else if let Some(last) = entries.last_mut() {
            // o app guarda (ushort)(agora - início); aqui satura em vez de dar a volta
            last.time = now_ms.saturating_sub(self.start_ms).min(u16::MAX as u64) as u32;
        }
        self.start_ms = now_ms;
        let actions = self.actions(&s);
        entries.push(Entry { actions, time: 0 });
        self.last = s;
        true
    }
}

/// Add/readKey: o app para de gravar em max_step - 1 entradas
pub fn is_full(entries: &[Entry]) -> bool {
    entries.len() >= MACRO_MAX_STEPS - 1
}

// ------------------------------------------------------------------ itens da tela do editor

/// Um bloco desenhado no editor: ação (50 ou 72 px) ou tempo (50 px com caixa de texto)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Act { entry: usize, idx: usize },
    Time { entry: usize },
}

/// RePaint: ordem dos blocos (os tempos 0 não aparecem)
pub fn items(entries: &[Entry]) -> Vec<Item> {
    let mut v = Vec::new();
    for (e, en) in entries.iter().enumerate() {
        for idx in 0..en.actions.len() {
            v.push(Item::Act { entry: e, idx });
        }
        if en.time != 0 {
            v.push(Item::Time { entry: e });
        }
    }
    v
}

/// Delete_KeyDown: remove o bloco e a entrada que ficar vazia
pub fn remove_item(entries: &mut Vec<Entry>, it: Item) {
    match it {
        Item::Act { entry, idx } => {
            entries[entry].actions.remove(idx);
            if entries[entry].actions.is_empty() && entries[entry].time == 0 {
                entries.remove(entry);
            }
        }
        Item::Time { entry } => {
            entries[entry].time = 0;
            if entries[entry].actions.is_empty() {
                entries.remove(entry);
            }
        }
    }
}

/// Grid_Up + Macro2toMacro: arrasta o bloco `from` para a posição `to` da lista de blocos visíveis.
/// A lista é achatada (ações e tempos, inclusive os tempos 0 escondidos) e remontada cortando
/// em cada tempo.
pub fn move_item(entries: &mut Vec<Entry>, from: usize, to: usize) {
    #[derive(Clone, Copy)]
    enum Flat {
        A(Action),
        T(u32, bool),
    }
    let mut flat = Vec::new();
    for e in entries.iter() {
        flat.extend(e.actions.iter().map(|&a| Flat::A(a)));
        flat.push(Flat::T(e.time, e.time != 0));
    }
    // índice de bloco visível → índice na lista achatada
    let vis: Vec<usize> = flat.iter().enumerate().filter(|(_, f)| !matches!(f, Flat::T(_, false))).map(|(k, _)| k).collect();
    if from >= vis.len() || from == to {
        return;
    }
    let src = vis[from];
    let item = flat.remove(src);
    let dst = if to >= vis.len() {
        flat.len().saturating_sub(1) // antes do tempo final (sempre há um)
    } else {
        let d = vis[to];
        if d > src { d - 1 } else { d }
    };
    flat.insert(dst.min(flat.len()), item);
    if !matches!(flat.last(), Some(Flat::T(..))) {
        flat.push(Flat::T(0, false));
    }
    let mut out = Vec::new();
    let mut cur = Entry::default();
    for f in flat {
        match f {
            Flat::A(a) => cur.actions.push(a),
            Flat::T(t, _) => {
                cur.time = t;
                if !cur.actions.is_empty() || t != 0 {
                    out.push(std::mem::take(&mut cur));
                }
                cur = Entry::default();
            }
        }
    }
    *entries = out;
}

// ------------------------------------------------------------------ biblioteca local

/// Macro salva na biblioteca local (o .ini do app guarda passo (.ini))
#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    pub name: String,
    pub cycles: u32,
    pub interval: u32,
    pub steps: Vec<Step>,
}

/// Pasta local: ~/.local/share/u2tool/macros/
pub fn dir(mode: u16) -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME").map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share"));
    let sub = match mode {
        1 => "Controller/Android",
        3 => "Controller/Windows",
        _ => "Controller",
    };
    base.join("u2tool/macros").join(sub)
}

fn file_of(mode: u16, name: &str) -> Option<PathBuf> {
    // nomes só com letras, números e chinês (MacroRenameDialog); nada de caminho
    if name.is_empty() || name.contains(['/', '\\', '.']) {
        return None;
    }
    Some(dir(mode).join(format!("{name}.json")))
}

pub fn list(mode: u16) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir(mode)).into_iter().flatten().flatten()
        .filter_map(|e| {
            let p = e.path();
            (p.extension()? == "json").then(|| p.file_stem()?.to_str().map(str::to_string))?
        })
        .collect();
    v.sort_by_key(|s| s.to_lowercase());
    v
}

pub fn load(mode: u16, name: &str) -> Option<Saved> {
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(file_of(mode, name)?).ok()?).ok()?;
    let steps = j.get("passos")?.as_array()?.iter().filter_map(|s| {
        let a: Vec<u16> = s.as_array()?.iter().filter_map(|x| x.as_u64().map(|x| x as u16)).collect();
        (a.len() == 5).then(|| Step { ms: a[0], keys: a[1], trigger: a[2], left: a[3], right: a[4] })
    }).take(MACRO_MAX_STEPS).collect();
    Some(Saved {
        name: name.to_string(),
        cycles: j.get("ciclos")?.as_u64()? as u32,
        interval: j.get("intervalo")?.as_u64()? as u32,
        steps,
    })
}

pub fn save(mode: u16, m: &Saved) -> std::io::Result<()> {
    let p = file_of(mode, &m.name).ok_or_else(|| std::io::Error::other("nome de macro inválido"))?;
    std::fs::create_dir_all(dir(mode))?;
    let passos: Vec<_> = m.steps.iter().map(|s| serde_json::json!([s.ms, s.keys, s.trigger, s.left, s.right])).collect();
    let j = serde_json::json!({ "ciclos": m.cycles, "intervalo": m.interval, "passos": passos });
    std::fs::write(p, j.to_string())
}

pub fn delete(mode: u16, name: &str) {
    if let Some(p) = file_of(mode, name) {
        let _ = std::fs::remove_file(p);
    }
}

pub fn rename(mode: u16, from: &str, to: &str) -> std::io::Result<()> {
    match (file_of(mode, from), file_of(mode, to)) {
        (Some(a), Some(b)) => std::fs::rename(a, b),
        _ => Err(std::io::Error::other("nome de macro inválido")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(ms: u16, keys: u16) -> Step {
        Step { ms, keys, trigger: 0, left: Step::CENTER, right: Step::CENTER }
    }

    #[test]
    fn steps_e_acoes_ida_e_volta() {
        // aperta A, 50 ms, aperta B, 30 ms, solta os dois
        let steps = vec![st(50, 0x2000), st(30, 0x3000), st(0, 0)];
        let e = entries_from_steps(&steps);
        assert_eq!(e[0].actions, vec![Action { key: Key::A, down: true }]);
        assert_eq!(e[1].actions, vec![Action { key: Key::B, down: true }]);
        assert_eq!(e[2].actions, vec![Action { key: Key::A, down: false }, Action { key: Key::B, down: false }]);
        assert_eq!(steps_from_entries(&e, None), steps);
    }

    #[test]
    fn sticks_codificados_como_no_app() {
        assert_eq!(stick_value(0), 0x7F7F);
        assert_eq!(stick_value(1), 127);
        assert_eq!(stick_value(2), 65407);
        assert_eq!(stick_value(4), 32512);
        assert_eq!(stick_value(8), 32767);
        assert_eq!(stick_value(1 | 4), 0);
        assert_eq!(stick_value(1 | 8), 255);
        assert_eq!(stick_value(2 | 4), 65280);
        assert_eq!(stick_value(2 | 8), 65535);
        let e = vec![Entry { actions: vec![Action { key: Key::RsLeft, down: true }, Action { key: Key::RsUp, down: true }], time: 20 }];
        let s = steps_from_entries(&e, None);
        assert_eq!((s[0].left, s[0].right), (0x7F7F, 0));
        assert_eq!(entries_from_steps(&s)[0].actions.len(), 2);
    }

    #[test]
    fn tempo_uniforme() {
        let e = vec![Entry { actions: vec![Action { key: Key::A, down: true }], time: 70 },
                     Entry { actions: vec![Action { key: Key::A, down: false }], time: 0 }];
        let s = steps_from_entries(&e, Some(30));
        assert_eq!((s[0].ms, s[1].ms), (30, 0));
        assert_eq!(uniform_time(&s), Some(30));
        assert_eq!(uniform_time(&[st(10, 1), st(20, 0), st(0, 0)]), None);
    }

    #[test]
    fn gravador_mede_o_tempo_entre_mudancas() {
        let mut r = Recorder::new(false);
        let mut e = Vec::new();
        let k = |buttons: u32, lx: u8| Keys { buttons, lx, ly: 128, rx: 128, ry: 128, lt: 0, rt: 0 };
        assert!(!r.feed(&k(0, 128), 0, &mut e));
        assert!(r.feed(&k(0x2000, 128), 100, &mut e)); // A
        assert!(!r.feed(&k(0x2000, 128), 110, &mut e));
        assert!(r.feed(&k(0x2000, 255), 160, &mut e)); // LS direita
        assert!(r.feed(&k(0, 128), 200, &mut e)); // solta tudo
        assert_eq!(e.len(), 3);
        assert_eq!(e[0], Entry { actions: vec![Action { key: Key::A, down: true }], time: 60 });
        assert_eq!(e[1], Entry { actions: vec![Action { key: Key::LsRight, down: true }], time: 40 });
        assert_eq!(e[2].actions, vec![Action { key: Key::LsRight, down: false }, Action { key: Key::A, down: false }]);
        let s = steps_from_entries(&e, None);
        assert_eq!(s[1], Step { ms: 40, keys: 0x2000, trigger: 0, left: 32767, right: 0x7F7F });
    }

    #[test]
    fn gatilho_analogico_vira_l2() {
        let mut r = Recorder::new(false);
        let mut e = Vec::new();
        let k = |lt: u8| Keys { buttons: 0, lx: 128, ly: 128, rx: 128, ry: 128, lt, rt: 0 };
        assert!(r.feed(&k(40), 0, &mut e));
        assert!(!r.feed(&k(200), 5, &mut e));
        assert!(r.feed(&k(0), 30, &mut e));
        assert_eq!(steps_from_entries(&e, None).iter().map(|s| s.keys).collect::<Vec<_>>(), vec![0x4000, 0]);
    }

    #[test]
    fn mover_e_apagar_blocos() {
        let a = |key, down| Action { key, down };
        let mut e = vec![Entry { actions: vec![a(Key::A, true)], time: 50 }, Entry { actions: vec![a(Key::A, false)], time: 0 }];
        assert_eq!(items(&e).len(), 3);
        // leva "solta A" para o começo
        move_item(&mut e, 2, 0);
        assert_eq!(e[0].actions, vec![a(Key::A, false), a(Key::A, true)]);
        assert_eq!(e[0].time, 50);
        remove_item(&mut e, Item::Time { entry: 0 });
        assert_eq!(e, vec![Entry { actions: vec![a(Key::A, false), a(Key::A, true)], time: 0 }]);
    }

    #[test]
    fn modo_de_repeticao() {
        assert_eq!(loop_mode(1, 0), Loop::Once);
        assert_eq!(loop_mode(CYCLES_FOREVER, 30), Loop::Forever);
        assert_eq!(loop_mode(5, 30), Loop::Times);
    }
}
