//! Thread única que fala com o controle. A interface manda comandos e recebe eventos;
//! assim nunca há dois leitores no mesmo hidraw (a causa do atraso no ao vivo).

use crate::config::{Config, Step};
use crate::proto::{self, Dev, Keys};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// Passos das 4 macros de cada um dos 3 perfis
pub type Steps = [[Vec<Step>; 4]; 3];

#[derive(Clone, Debug)]
pub struct Info {
    pub pid: u16,
    pub config: Vec<u8>,
    pub theme: u8,
    pub variant: u8,
    pub color: u8,
    pub macros: Steps,
    /// erro lendo os passos das macros (a configuração em si foi lida)
    pub macros_err: Option<String>,
}

pub enum Cmd {
    Load,
    /// grava em `new` só o que difere de `base`
    Apply { base: Vec<u8>, new: Vec<u8> },
    /// como Apply e depois os passos: (perfil, macro, passos) — gravação do struct e dos passos
    Macros { base: Vec<u8>, new: Vec<u8>, steps: Vec<(usize, usize, Vec<Step>)> },
    Theme(u8),
    Slot,
    /// ao vivo ligado/desligado; `vib` = níveis 0-5 para o teste de vibração
    Live { on: bool, vib: Option<(u8, u8)> },
    /// gravação de macro: pausa o input do controle (s_swichReport(0)) e lê as teclas
    Record(bool),
}

pub enum Event {
    Loaded(Info),
    NotConnected,
    Applied { fields: usize, slot: u16 },
    Slot(u16),
    /// estado ao vivo e quando foi lido (a gravação de macro mede o tempo por aqui)
    Keys(Keys, Instant),
    Error(String),
}

pub struct Handle {
    pub tx: Sender<Cmd>,
    pub rx: Receiver<Event>,
}

/// leitura de macros: passos das macros ativas (flag ligada, tecla válida, até 200 passos)
fn read_macros(d: &mut Dev, cfg: &[u8]) -> std::io::Result<Steps> {
    let c = Config::new(cfg.to_vec());
    let mut out: Steps = Default::default();
    for (i, slot) in out.iter_mut().enumerate() {
        for (k, steps) in slot.iter_mut().enumerate() {
            if let Some(m) = c.active_macro(i, k) {
                if m.steps > 0 {
                    *steps = d.read_macro(c.mode(), i, k, m.steps as usize)?;
                }
            }
        }
    }
    Ok(out)
}

/// `offline`: caminho de um .bin para editar sem controle. `write`: permite gravar.
pub fn spawn(offline: Option<String>, write: bool, repaint: impl Fn() + Send + 'static) -> Handle {
    let (tx, crx) = mpsc::channel::<Cmd>();
    let (etx, rx) = mpsc::channel::<Event>();
    std::thread::spawn(move || {
        let send = |e: Event| {
            let _ = etx.send(e);
            repaint();
        };
        if let Some(path) = offline {
            return run_offline(path, crx, send);
        }
        let mut dev: Option<Dev> = None;
        let mut live = false;
        let mut recording = false;
        let mut misses = 0u32;
        let mut conn_t = Instant::now();
        let mut off = false;
        let mut vib: Option<(u8, u8)> = None;
        let mut last_keys: Option<Keys> = None;
        let mut motor = (0u8, 0u8);
        let mut motor_t = Instant::now();
        loop {
            let cmd = if live || recording {
                match crx.try_recv() {
                    Ok(c) => Some(c),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(_) => return,
                }
            } else {
                match crx.recv_timeout(Duration::from_millis(200)) {
                    Ok(c) => Some(c),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(_) => return,
                }
            };
            if dev.is_none() {
                dev = proto::find_hidraw().and_then(|(p, _)| Dev::open(&p).ok());
            }
            let Some(d) = dev.as_mut() else {
                if matches!(cmd, Some(Cmd::Load)) {
                    send(Event::NotConnected);
                }
                if live || recording {
                    std::thread::sleep(Duration::from_millis(100));
                }
                continue;
            };
            let mut lost = false;
            match cmd {
                Some(Cmd::Load) => {
                    let pid = proto::find_hidraw().map_or(0, |x| x.1);
                    let r = d.session(|d| {
                        if d.is_connected() != Some(true) {
                            return Ok(None);
                        }
                        let config = d.read_config()?;
                        let (macros, macros_err) = match read_macros(d, &config) {
                            Ok(m) => (m, None),
                            Err(e) => (Default::default(), Some(e.to_string())),
                        };
                        Ok(Some(Info { pid, config, theme: d.read_theme().unwrap_or(5), variant: d.read_variant(), color: d.read_color(),
                                       macros, macros_err }))
                    });
                    match r {
                        Ok(Some(info)) => send(Event::Loaded(info)),
                        Ok(None) => { send(Event::NotConnected); lost = true; }
                        Err(e) => { send(Event::Error(e.to_string())); lost = true; }
                    }
                }
                Some(Cmd::Apply { base, new }) => {
                    if !write {
                        send(Event::Error("somente leitura — alteração não enviada".into()));
                        continue;
                    }
                    match d.session(|d| d.apply_changes(&base, &new)) {
                        Ok((f, slot)) => send(Event::Applied { fields: f.len(), slot }),
                        Err(e) => send(Event::Error(e.to_string())),
                    }
                }
                Some(Cmd::Macros { base, new, steps }) => {
                    if !write {
                        send(Event::Error("somente leitura — macro não enviada".into()));
                        continue;
                    }
                    let mode = Config::new(new.clone()).mode();
                    let r = d.session(|d| {
                        let (f, slot) = d.apply_changes(&base, &new)?;
                        for (i, k, s) in &steps {
                            d.write_macro(mode, *i, *k, s)?;
                        }
                        Ok((f.len() + steps.len(), slot))
                    });
                    match r {
                        Ok((fields, slot)) => send(Event::Applied { fields, slot }),
                        Err(e) => { send(Event::Error(e.to_string())); lost = true; }
                    }
                }
                Some(Cmd::Theme(t)) => {
                    if !write {
                        send(Event::Error("somente leitura — efeito não enviado".into()));
                        continue;
                    }
                    if let Err(e) = d.write_theme(t) {
                        send(Event::Error(e.to_string()));
                        lost = true;
                    }
                }
                Some(Cmd::Slot) => {
                    if let Ok(h) = d.session(|d| d.read_range(0, 0x14)) {
                        send(Event::Slot(u16::from_le_bytes([h[0x12], h[0x13]])));
                    }
                }
                Some(Cmd::Live { on, vib: v }) => {
                    live = on;
                    vib = if write { v } else { None };
                    last_keys = None;
                    if !on && motor != (0, 0) {
                        d.rumble(0, 0, 1000);
                        motor = (0, 0);
                    }
                }
                Some(Cmd::Record(on)) => {
                    if on != recording {
                        d.drain();
                        let _ = d.report_state(!on);
                        recording = on;
                        last_keys = None;
                    }
                }
                None => {}
            }
            if lost {
                dev = None;
                continue;
            }
            let Some(d) = dev.as_mut() else { continue };
            if live || recording {
                // Controle desligado: o dongle (modo 6013) devolve o último quadro; mostra centro.
                if conn_t.elapsed() > Duration::from_millis(500) {
                    conn_t = Instant::now();
                    off = d.is_connected() == Some(false);
                    if off {
                        let k = Keys { lx: 128, ly: 127, rx: 128, ry: 127, ..Default::default() };
                        if last_keys != Some(k) {
                            last_keys = Some(k);
                            send(Event::Keys(k, Instant::now()));
                        }
                    }
                }
                if off {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                }
                let keys = d.read_keys();
                // ~1 s sem resposta: dongle reconectado (nó hidraw antigo) ou controle desligado; reabre.
                misses = if keys.is_some() { 0 } else { misses + 1 };
                if misses >= 20 {
                    misses = 0;
                    dev = None;
                    last_keys = None;
                    continue;
                }
                if let Some(k) = keys {
                    let at = Instant::now();
                    if let (Some((lv, rv)), false) = (vib, recording) {
                        let lp = k.lt > 0 || k.buttons & 0x4000 != 0;
                        let rp = k.rt > 0 || k.buttons & 0x8000 != 0;
                        let want = (if lp { (lv as u32 * 255 / 5) as u8 } else { 0 }, if rp { (rv as u32 * 255 / 5) as u8 } else { 0 });
                        if want != motor || (want != (0, 0) && motor_t.elapsed() > Duration::from_millis(500)) {
                            d.rumble(want.0, want.1, 1000);
                            motor = want;
                            motor_t = Instant::now();
                        }
                    }
                    if Some(k) != last_keys {
                        last_keys = Some(k);
                        send(Event::Keys(k, at));
                    }
                }
                std::thread::sleep(Duration::from_millis(4));
            }
        }
    });
    Handle { tx, rx }
}

/// Passos das macros no modo offline: arquivo ao lado do .bin (`x.bin.macros.json`)
fn sidecar(path: &str) -> String {
    format!("{path}.macros.json")
}
fn load_sidecar(path: &str) -> Steps {
    let mut out: Steps = Default::default();
    let Some(j) = std::fs::read_to_string(sidecar(path)).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) else {
        return out;
    };
    for (i, slot) in out.iter_mut().enumerate() {
        for (k, steps) in slot.iter_mut().enumerate() {
            if let Some(a) = j.get(i).and_then(|s| s.get(k)).and_then(|m| m.as_array()) {
                *steps = a.iter().filter_map(|s| {
                    let v: Vec<u16> = s.as_array()?.iter().filter_map(|x| x.as_u64().map(|x| x as u16)).collect();
                    (v.len() == 5).then(|| Step { ms: v[0], keys: v[1], trigger: v[2], left: v[3], right: v[4] })
                }).collect();
            }
        }
    }
    out
}
fn save_sidecar(path: &str, m: &Steps) {
    let j: Vec<Vec<Vec<_>>> = m.iter().map(|slot| slot.iter().map(|steps| {
        steps.iter().map(|s| serde_json::json!([s.ms, s.keys, s.trigger, s.left, s.right])).collect()
    }).collect()).collect();
    let _ = std::fs::write(sidecar(path), serde_json::Value::from(j).to_string());
}

fn run_offline(path: String, crx: Receiver<Cmd>, send: impl Fn(Event)) {
    let mut theme = 5u8;
    let apply = |base: &[u8], new: &[u8]| {
        let mut cur = std::fs::read(&path).unwrap_or(base.to_vec());
        let ch = proto::diff_fields(base, new);
        for &(o, n) in &ch {
            cur[o..o + n].copy_from_slice(&new[o..o + n]);
        }
        let _ = std::fs::write(&path, &cur);
        (ch.len(), u16::from_le_bytes([cur[0x12], cur[0x13]]))
    };
    while let Ok(cmd) = crx.recv() {
        match cmd {
            Cmd::Load => match std::fs::read(&path) {
                Ok(config) => send(Event::Loaded(Info { pid: 0x310B, config, theme, variant: 1, color: 0, macros: load_sidecar(&path),
                                                        macros_err: None })),
                Err(e) => send(Event::Error(e.to_string())),
            },
            Cmd::Apply { base, new } => {
                let (fields, slot) = apply(&base, &new);
                send(Event::Applied { fields, slot });
            }
            Cmd::Macros { base, new, steps } => {
                let (fields, slot) = apply(&base, &new);
                let mut m = load_sidecar(&path);
                for (i, k, s) in &steps {
                    m[*i][*k] = s.clone();
                }
                save_sidecar(&path, &m);
                send(Event::Applied { fields: fields + steps.len(), slot });
            }
            Cmd::Theme(t) => theme = t,
            Cmd::Slot => {}
            Cmd::Live { .. } | Cmd::Record(_) => {
                send(Event::Keys(Keys { lx: 128, ly: 128, rx: 128, ry: 128, ..Default::default() }, Instant::now()))
            }
        }
    }
}
