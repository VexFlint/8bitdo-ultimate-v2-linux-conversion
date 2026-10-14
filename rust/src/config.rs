//! Struct de configuração do controle (1592 bytes); layout em PROTOCOLO.md.

pub const ENABLE: u32 = 0x2020_0911; // flag de perfil gravado
pub const NOT_ENABLE: u32 = 538_509_312; // notenableFlag

pub mod off {
    pub const FLAG: usize = 0x000;
    pub const MODE: usize = 0x010;
    pub const SLOT: usize = 0x012;
    pub const NAME: usize = 0x014;
    pub const RUMBLE: usize = 0x074;
    pub const JOY: usize = 0x098;
    pub const TRIG: usize = 0x0B0;
    pub const SPECIAL: usize = 0x0C8;
    pub const MAPKEY: usize = 0x0E0;
    pub const MACRO: usize = 0x1F4;
    pub const XRUMBLE: usize = 0x47C;
    pub const SIXAXIS: usize = 0x494;
    pub const DYNAMIC: usize = 0x4B8;
    pub const HOTWHEEL: usize = 0x4DC;
    pub const SINGLE: usize = 0x50C;
}

/// (offset, tamanho) de cada bloco por perfil — para copiar perfil inteiro
pub const PER_SLOT: [(usize, usize); 12] = [(off::NAME, 32), (off::RUMBLE, 12), (off::JOY, 8), (off::TRIG, 8),
    (off::SPECIAL, 8), (off::MAPKEY, 92), (off::MACRO, 216), (off::XRUMBLE, 8), (off::SIXAXIS, 12),
    (off::DYNAMIC, 12), (off::HOTWHEEL, 16), (off::SINGLE, 100)];

/// Recursos especiais (bits)
pub mod feat {
    pub const LX: u32 = 0x1;
    pub const LY: u32 = 0x2;
    pub const RX: u32 = 0x4;
    pub const RY: u32 = 0x8;
    pub const SWAP_STICKS: u32 = 0x10;
    pub const SWAP_TRIGGERS: u32 = 0x80;
    pub const DPAD_LSTICK: u32 = 0x100;
}

/// Bits de destino do firmware do Ultimate 2 .
pub const KEYS: &[(&str, u32)] = &[
    ("A", 0x2000), ("B", 0x1000), ("X", 0x10), ("Y", 0x20), ("L", 0x400), ("R", 0x800), ("L2", 0x4000),
    ("R2", 0x8000), ("L3", 0x2), ("R3", 0x4), ("SELECT", 0x8), ("START", 0x1), ("Home", 0x20000),
    ("Share", 0x10000), ("Up", 0x200), ("Down", 0x100), ("Left", 0x80), ("Right", 0x40),
    ("screenshot", 0x400000), ("turbo3", 0x800000), ("AutoTurbo", 0x1000000),
    ("AS_P1", 0x2000000), ("AS_P2", 0x4000000), ("AS_P3", 0x200000), ("AS_P4", 0x4000_0000),
    ("LS_Up", 134217744), ("LS_Down", 134217760), ("LS_Left", 134217792), ("LS_Right", 134217856),
    ("RS_Up", 134217729), ("RS_Down", 134217730), ("RS_Left", 134217732), ("RS_Right", 134217736),
    ("N", 0),
];
pub fn key(name: &str) -> u32 {
    KEYS.iter().find(|k| k.0 == name).map_or(0, |k| k.1)
}
pub fn key_name(v: u32) -> &'static str {
    KEYS.iter().find(|k| k.1 == v).map_or("N", |k| k.0)
}

/// Ordem física das 22 posições de map_key (readkey_map / swtichData)
pub const SLOTS: [&str; 22] = ["A", "B", "X", "Y", "L", "R", "L2", "R2", "L3", "R3", "SELECT", "START",
    "Share", "Home", "Up", "Down", "Left", "Right", "AS_P1", "AS_P2", "AS_P3", "AS_P4"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sixaxis {
    pub key: u32,
    pub trigger: u8,
    pub sensitivity: u8,
    pub dead: u8,
    pub mapping: u8,
}

/// registro de macro (52 bytes): o que o struct guarda de cada macro. Os passos vão à parte.
#[derive(Clone, Debug, PartialEq)]
pub struct Macro {
    pub name: String,
    pub key: u32,
    pub steps: u16,
    pub cycles: u32,
    pub interval: u32,
}

/// Passos por macro (limite do app) e área de cada macro na flash (leitura de macro/escrita de macro).
pub const MACRO_MAX_STEPS: usize = 200;
pub const MACRO_BLOCK: usize = 4096;
/// cycles_num: 1 = uma vez; u32::MAX = repetir até soltar; outro = N vezes
pub const CYCLES_FOREVER: u32 = u32::MAX;

/// passo (10 bytes): um passo da macro.
/// `keys` usa os bits do KeyMap (A 0x2000, B 0x1000, ..., L2 0x4000, R2 0x8000);
/// sticks: byte baixo = X, alto = Y (0x00 esquerda/cima, 0x7F centro, 0xFF direita/baixo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub ms: u16,
    pub keys: u16,
    pub trigger: u16,
    pub left: u16,
    pub right: u16,
}
impl Step {
    pub const CENTER: u16 = 0x7F7F;
    /// passo neutro de 0 ms (completa o bloco gravado até múltiplo de 32)
    pub const NEUTRAL: Step = Step { ms: 0, keys: 0, trigger: 0, left: Step::CENTER, right: Step::CENTER };

    pub fn to_bytes(&self) -> [u8; 10] {
        let mut b = [0u8; 10];
        for (k, v) in [self.ms, self.keys, self.trigger, self.left, self.right].into_iter().enumerate() {
            b[2 * k..2 * k + 2].copy_from_slice(&v.to_le_bytes());
        }
        b
    }
    pub fn from_bytes(b: &[u8]) -> Step {
        let w = |k: usize| u16::from_le_bytes([b[2 * k], b[2 * k + 1]]);
        Step { ms: w(0), keys: w(1), trigger: w(2), left: w(3), right: w(4) }
    }
}

pub fn steps_to_bytes(steps: &[Step]) -> Vec<u8> {
    steps.iter().flat_map(|s| s.to_bytes()).collect()
}
pub fn steps_from_bytes(b: &[u8]) -> Vec<Step> {
    b.chunks_exact(10).map(Step::from_bytes).collect()
}

/// Bloco que o app grava: getCount(count×10) arredonda para múltiplo de 32. O app manda o que
/// estiver na memória depois do array; aqui o resto é completado com passos neutros.
pub fn macro_payload(steps: &[Step]) -> Vec<u8> {
    let n = steps.len() * 10;
    let total = n.div_ceil(32) * 32;
    let mut b = steps_to_bytes(steps);
    while b.len() < total {
        b.extend(Step::NEUTRAL.to_bytes());
    }
    b.truncate(total);
    b
}

/// Teclas que podem disparar macro (teclas válidas de macro)
pub const MACRO_TRIGGER_KEYS: [u32; 22] = [0x2000, 0x1000, 0x10, 0x20, 0x200, 0x100, 0x80, 0x40, 0x200_0000, 0x400_0000,
    0x20_0000, 0x4000_0000, 0x10_0000, 0x400, 0x4000, 0x2, 0x800, 0x8000, 0x4, 0x8, 0x1, 0x1_0000];
pub fn is_macro_key(k: u32) -> bool {
    MACRO_TRIGGER_KEYS.contains(&k)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub b: Vec<u8>,
}

impl Config {
    pub fn new(b: Vec<u8>) -> Self {
        Config { b }
    }
    pub fn u32(&self, o: usize) -> u32 {
        u32::from_le_bytes(self.b[o..o + 4].try_into().unwrap())
    }
    pub fn set_u32(&mut self, o: usize, v: u32) {
        self.b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn u16(&self, o: usize) -> u16 {
        u16::from_le_bytes([self.b[o], self.b[o + 1]])
    }
    fn set_u16(&mut self, o: usize, v: u16) {
        self.b[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn f32(&self, o: usize) -> f32 {
        f32::from_le_bytes(self.b[o..o + 4].try_into().unwrap())
    }
    fn set_f32(&mut self, o: usize, v: f32) {
        self.b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    pub fn mode(&self) -> u16 {
        self.u16(off::MODE) // 0 Switch, 1 DInput, 2 Mac, 3 XInput
    }
    pub fn cur_slot(&self) -> u16 {
        self.u16(off::SLOT) // 0-2; 3 = nenhum
    }
    pub fn set_cur_slot(&mut self, s: u16) {
        self.set_u16(off::SLOT, s)
    }
    pub fn used(&self, i: usize) -> bool {
        self.u32(off::FLAG + 4 * i) == ENABLE
    }

    /// S_File.stringToBytes: UTF-16 big-endian em 32 bytes
    pub fn str_at(&self, o: usize, n: usize) -> String {
        if self.b[o] == 0xFF && self.b[o + 1] == 0xFF {
            return String::new();
        }
        let units: Vec<u16> = (0..n / 2).map(|k| u16::from_be_bytes([self.b[o + 2 * k], self.b[o + 2 * k + 1]]))
            .take_while(|&c| c != 0).collect();
        String::from_utf16_lossy(&units)
    }
    pub fn set_str_at(&mut self, o: usize, s: &str, n: usize) {
        self.b[o..o + n].fill(0);
        for (k, c) in s.encode_utf16().take(n / 2).enumerate() {
            self.b[o + 2 * k..o + 2 * k + 2].copy_from_slice(&c.to_be_bytes());
        }
    }
    pub fn name(&self, i: usize) -> String {
        self.str_at(off::NAME + 32 * i, 32)
    }
    pub fn set_name(&mut self, i: usize, s: &str) {
        self.set_str_at(off::NAME + 32 * i, s, 32)
    }

    // ----- botões
    pub fn default_keys(&self) -> [u32; 22] {
        let mut k = [0u32; 22];
        for (n, s) in SLOTS.iter().enumerate() {
            k[n] = if s.starts_with("AS_") { 0 } else if *s == "Share" {
                if self.mode() == 0 { key("screenshot") } else { 0x10000 }
            } else { key(s) };
        }
        if self.mode() == 0 {
            k[..4].copy_from_slice(&[key("B"), key("A"), key("Y"), key("X")]);
        }
        k
    }
    pub fn keys(&self, i: usize) -> [u32; 22] {
        let o = off::MAPKEY + 92 * i;
        if self.u32(o) != ENABLE {
            return self.default_keys();
        }
        std::array::from_fn(|k| self.u32(o + 4 + 4 * k))
    }
    pub fn set_keys(&mut self, i: usize, k: &[u32; 22]) {
        let o = off::MAPKEY + 92 * i;
        self.set_u32(o, ENABLE);
        for (n, v) in k.iter().enumerate() {
            self.set_u32(o + 4 + 4 * n, *v);
        }
    }
    pub fn target(&self, i: usize, slot: &str) -> &'static str {
        let idx = SLOTS.iter().position(|s| *s == slot).unwrap();
        key_name(self.keys(i)[idx])
    }
    pub fn set_target(&mut self, i: usize, slot: &str, target: &str) {
        let idx = SLOTS.iter().position(|s| *s == slot).unwrap();
        let mut k = self.keys(i);
        k[idx] = key(target);
        self.set_keys(i, &k);
    }

    // ----- sticks 0..128 / gatilhos 0..255
    pub fn pair(&self, base: usize, i: usize, side: usize) -> (u8, u8) {
        let o = base + 8 * i + 4 + 2 * side;
        (self.b[o], self.b[o + 1])
    }
    pub fn set_pair(&mut self, base: usize, i: usize, side: usize, a: u8, z: u8) {
        let o = base + 8 * i;
        self.set_u32(o, ENABLE);
        self.b[o + 4 + 2 * side] = a;
        self.b[o + 5 + 2 * side] = z;
    }
    pub fn feature(&self, i: usize) -> u32 {
        self.u32(off::SPECIAL + 8 * i + 4)
    }
    pub fn set_feature(&mut self, i: usize, bits: u32) {
        self.set_u32(off::SPECIAL + 8 * i, ENABLE);
        self.set_u32(off::SPECIAL + 8 * i + 4, bits);
    }

    // ----- vibração: zoom_out = nível × 0,2
    pub fn rumble_level(&self, i: usize, side: usize) -> u8 {
        let f = self.f32(off::RUMBLE + 12 * i + 4 + 4 * side);
        if f.is_finite() { ((f / 0.2 + 1e-4).floor() as i32).clamp(0, 5) as u8 } else { 5 }
    }
    pub fn set_rumble_level(&mut self, i: usize, side: usize, lv: u8) {
        let o = off::RUMBLE + 12 * i;
        self.set_u32(o, ENABLE);
        self.set_f32(o + 4 + 4 * side, [0.0, 0.2, 0.4, 0.6, 0.8, 1.0][lv as usize]);
    }

    // ----- giroscópio
    pub fn sixaxis(&self, i: usize) -> Sixaxis {
        let o = off::SIXAXIS + 12 * i;
        Sixaxis { key: self.u32(o + 4), trigger: self.b[o + 8], sensitivity: self.b[o + 9].clamp(1, 10),
                  dead: self.b[o + 10].min(100), mapping: self.b[o + 11] }
    }
    pub fn set_sixaxis(&mut self, i: usize, s: &Sixaxis) {
        let o = off::SIXAXIS + 12 * i;
        self.set_u32(o, if s.mapping != 0 { ENABLE } else { NOT_ENABLE });
        self.set_u32(o + 4, s.key);
        self.b[o + 8..o + 12].copy_from_slice(&[s.trigger, s.sensitivity, s.dead, s.mapping]);
    }

    // ----- LEDs
    pub fn dynamic(&self, i: usize) -> (u32, u32) {
        let o = off::DYNAMIC + 12 * i;
        (self.u32(o + 4), self.u32(o + 8))
    }
    pub fn set_dynamic(&mut self, i: usize, c: (u32, u32)) {
        let o = off::DYNAMIC + 12 * i;
        self.set_u32(o, ENABLE);
        self.set_u32(o + 4, c.0);
        self.set_u32(o + 8, c.1);
    }
    pub fn hotwheel(&self, i: usize) -> (u32, u32, u8) {
        let o = off::HOTWHEEL + 16 * i;
        (self.u32(o + 4), self.u32(o + 8), self.b[o + 12])
    }
    pub fn set_hotwheel(&mut self, i: usize, h: (u32, u32, u8)) {
        let o = off::HOTWHEEL + 16 * i;
        self.set_u32(o, ENABLE);
        self.set_u32(o + 4, h.0);
        self.set_u32(o + 8, h.1);
        self.b[o + 12] = h.2;
    }
    pub fn single(&self, i: usize) -> [u32; 24] {
        let o = off::SINGLE + 100 * i;
        std::array::from_fn(|k| self.u32(o + 4 + 4 * k))
    }
    pub fn set_single(&mut self, i: usize, c: &[u32; 24]) {
        let o = off::SINGLE + 100 * i;
        self.set_u32(o, ENABLE);
        for (k, v) in c.iter().enumerate() {
            self.set_u32(o + 4 + 4 * k, *v);
        }
    }

    // ----- macros
    pub fn macros(&self, i: usize) -> Vec<Macro> {
        let o = off::MACRO + 216 * i;
        (0..4).map(|k| {
            let r = o + 8 + 52 * k;
            Macro { name: self.str_at(r, 32), key: self.u32(r + 40), steps: self.u16(r + 34), cycles: self.u32(r + 44),
                    interval: self.u32(r + 48) }
        }).collect()
    }
    pub fn macros_enabled(&self, i: usize) -> bool {
        self.u32(off::MACRO + 216 * i) == ENABLE
    }
    /// Macro `k` do perfil `i` que o controle executa (leitura de macros: flag ligada e tecla válida)
    pub fn active_macro(&self, i: usize, k: usize) -> Option<Macro> {
        let m = self.macros(i).swap_remove(k);
        (self.macros_enabled(i) && is_macro_key(m.key)).then_some(m)
    }

    /// ao gravar: o app sempre remonta o bloco de macros inteiro (flag, total_cnt = 4,
    /// cada registro zerado com o modo do controle) e preenche só as macros com tecla e passos.
    pub fn set_macros(&mut self, i: usize, defs: &[Option<Macro>; 4]) {
        let o = off::MACRO + 216 * i;
        let mode = self.mode() as u8;
        self.b[o..o + 216].fill(0);
        self.set_u32(o, ENABLE);
        self.b[o + 4] = 4;
        for (k, d) in defs.iter().enumerate() {
            let r = o + 8 + 52 * k;
            self.b[r + 32] = mode;
            let Some(m) = d else { continue };
            if m.steps == 0 || !is_macro_key(m.key) {
                continue;
            }
            self.set_str_at(r, &m.name, 32);
            self.set_u16(r + 34, m.steps.min(MACRO_MAX_STEPS as u16));
            self.set_u16(r + 36, (k * MACRO_BLOCK) as u16);
            self.set_u32(r + 40, m.key);
            self.set_u32(r + 44, m.cycles);
            self.set_u32(r + 48, m.interval);
        }
    }
    /// As 4 macros ativas do perfil, no formato de set_macros
    pub fn macro_defs(&self, i: usize) -> [Option<Macro>; 4] {
        std::array::from_fn(|k| self.active_macro(i, k))
    }
    pub fn clear_macros(&mut self, k: usize) {
        let m = off::MACRO + 216 * k;
        self.b[m + 4] = 0;
        for r in 0..4 {
            let o = m + 8 + 52 * r;
            self.set_u16(o + 34, 0);
            self.set_u32(o + 40, 0);
        }
    }

    // ----- perfis
    /// Perfil novo com os padrões do app
    pub fn create_profile(&mut self, i: usize, name: &str) {
        self.set_u32(off::FLAG + 4 * i, ENABLE);
        self.set_name(i, name);
        let r = off::RUMBLE + 12 * i;
        self.set_u32(r, ENABLE);
        self.set_f32(r + 4, 1.0);
        self.set_f32(r + 8, 1.0);
        let x = off::XRUMBLE + 8 * i;
        self.set_u32(x, ENABLE);
        self.b[x + 4..x + 8].copy_from_slice(&[1, 100, 1, 100]);
        let j = off::JOY + 8 * i;
        self.set_u32(j, ENABLE);
        self.b[j + 4..j + 8].copy_from_slice(&[17, 128, 17, 128]);
        let t = off::TRIG + 8 * i;
        self.set_u32(t, ENABLE);
        let trig: [u8; 4] = if self.mode() == 0 { [77, 255, 77, 255] } else { [0, 255, 0, 255] };
        self.b[t + 4..t + 8].copy_from_slice(&trig);
        self.set_feature(i, 0);
        let dk = self.default_keys();
        self.set_keys(i, &dk);
        let m = off::MACRO + 216 * i;
        self.b[m..m + 216].fill(0);
        self.set_u32(m, ENABLE);
        let mode = self.mode() as u8;
        for k in 0..4 {
            self.b[m + 8 + 52 * k + 32] = mode;
        }
        self.set_sixaxis(i, &Sixaxis { key: 0, trigger: 0, sensitivity: 10, dead: 40, mapping: 0 });
        self.set_dynamic(i, (2490418, 544336));
        self.set_hotwheel(i, (13394432, 13369344, 7));
        self.set_single(i, &[8270336; 24]);
    }
    /// PlatFormView.Delete_Down: notenableFlag + clearConfigWith
    pub fn remove_profile(&mut self, i: usize) {
        self.set_u32(off::FLAG + 4 * i, NOT_ENABLE);
        self.set_name(i, "");
        let dk = self.default_keys();
        self.set_keys(i, &dk);
        self.set_u32(off::MAPKEY + 92 * i, 0);
        let j = off::JOY + 8 * i;
        self.b[j + 4..j + 8].copy_from_slice(&[0, 128, 0, 128]);
        self.set_u32(j, 0);
        let t = off::TRIG + 8 * i;
        self.b[t + 4..t + 8].copy_from_slice(&[0, 255, 0, 255]);
        self.set_u32(t, 0);
        let r = off::RUMBLE + 12 * i;
        self.set_f32(r + 4, 1.0);
        self.set_f32(r + 8, 1.0);
        self.set_u32(r, 0);
        let x = off::XRUMBLE + 8 * i;
        self.b[x + 4..x + 8].copy_from_slice(&[1, 100, 1, 100]);
        self.set_u32(x, 0);
        self.set_u32(off::SPECIAL + 8 * i + 4, 0);
        self.set_u32(off::SPECIAL + 8 * i, 0);
        self.set_u32(off::MACRO + 216 * i, 0);
    }
    /// Copia o perfil inteiro, inclusive o registro das macros.
    /// Os passos vão à parte (gravação dos passos); sem eles, quem chama usa clear_macros.
    pub fn copy_profile(&mut self, from: &Config, i: usize, k: usize) {
        let flag = from.u32(off::FLAG + 4 * i);
        self.set_u32(off::FLAG + 4 * k, flag);
        for (o, n) in PER_SLOT {
            let src = from.b[o + n * i..o + n * (i + 1)].to_vec();
            self.b[o + n * k..o + n * (k + 1)].copy_from_slice(&src);
        }
    }
}
