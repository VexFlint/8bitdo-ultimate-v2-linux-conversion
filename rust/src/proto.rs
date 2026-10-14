//! Protocolo de configuração do 8BitDo Ultimate 2 — port do u2tool.py.
//! Detalhes do protocolo em ../PROTOCOLO.md.
//!
//! Lições do controle real que este código precisa manter:
//! - `len` do cabeçalho nunca passa de 45 (nem na leitura);
//! - conferir o offset ecoado (bytes 14-17) de cada resposta de leitura/escrita;
//! - CRC-16/MODBUS (0xA001, init 0xFFFF);
//! - nunca escrever 0x0C-0x13; flag[] em três campos de 4 bytes;
//! - antes de ler o estado ao vivo, esvaziar a fila e ficar com a resposta mais nova.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::time::{Duration, Instant};

pub const VID: u16 = 0x2DC8;
pub const CFG_SIZE: usize = 0x638; // 1592 = sizeof(struct de configuração)
const CHUNK: usize = 45;

const CMD_WRITE: u16 = 1;
const CMD_READ: u16 = 2;
const CMD_KEYS: u16 = 3;
const CMD_SAVE: u16 = 6;
const CMD_REPORT_STATE: u16 = 7;
const CMD_RUMBLE: u16 = 8;
const CMD_THEME_WRITE: u16 = 0x40;
const CMD_THEME_READ: u16 = 0x41;
const CMD_VARIANT: u16 = 0x105;
const CMD_CONNECT: u16 = 0x120;
const SAVE_SUB: u16 = 0x123;
const KEYS_SUB: u16 = 0x123;
// passos de macro (leitura / escrita / apagar de macro do biblioteca nativa do app oficial)
const CMD_MACRO_READ: u16 = 0x102;
const CMD_MACRO_WRITE: u16 = 0x103;
const CMD_MACRO_ERASE: u16 = 0x104;
/// o app (nos comandos de macro) corta em 32, não em 45
const MACRO_CHUNK: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Keys {
    pub buttons: u32,
    pub lx: u8,
    pub ly: u8,
    pub rx: u8,
    pub ry: u8,
    pub rt: u8,
    pub lt: u8,
}

pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xA001 } else { crc >> 1 };
        }
    }
    crc
}

/// Campos na granularidade em que o app grava (offset, tamanho). 0x0C-0x13 fica de fora.
pub fn fields() -> Vec<(usize, usize)> {
    let mut f = vec![(0x000, 4), (0x004, 4), (0x008, 4)];
    for (base, size) in [(0x014, 32), (0x074, 12), (0x098, 8), (0x0B0, 8), (0x0C8, 8), (0x0E0, 92),
                         (0x1F4, 216), (0x47C, 8), (0x494, 12), (0x4B8, 12), (0x4DC, 16), (0x50C, 100)] {
        for i in 0..3 {
            f.push((base + size * i, size));
        }
    }
    f
}

pub fn diff_fields(old: &[u8], new: &[u8]) -> Vec<(usize, usize)> {
    fields().into_iter().filter(|&(o, n)| old[o..o + n] != new[o..o + n]).collect()
}

/// 81 04 <cmd u16> <sub u16> <len u16> <crc u16> <total u32> <offset u32> <dados>, 64 bytes
pub fn packet(cmd: u16, sub: u16, len: u16, total: u32, offset: u32, data: &[u8]) -> Vec<u8> {
    let crc = if data.is_empty() { 0 } else { crc16(data) };
    let mut p = vec![0x81u8, 0x04];
    p.extend(cmd.to_le_bytes());
    p.extend(sub.to_le_bytes());
    p.extend(len.to_le_bytes());
    p.extend(crc.to_le_bytes());
    p.extend(total.to_le_bytes());
    p.extend(offset.to_le_bytes());
    p.extend(data);
    p.resize(64, 0);
    p
}

/// sub dos comandos de macro: (gamepad_mode << 8) | perfil
pub fn macro_sub(mode: u16, slot: usize) -> u16 {
    ((mode & 0xFF) << 8) | (slot as u16 & 0xFF)
}

/// leitura de macro: pacotes de leitura dos passos da macro `idx` (`n` bytes = passos × 10).
/// O app manda `len` bytes de lixo da pilha (0xCC, /RTC do MSVC) com CRC; aqui igual.
/// offset e total vão truncados em 16 bits (o app usa ushort).
pub fn macro_read_packet(sub: u16, idx: usize, pos: usize, n: usize) -> (u32, Vec<u8>) {
    let len = MACRO_CHUNK.min(n - pos);
    let at = (idx * crate::config::MACRO_BLOCK + pos) as u16 as u32;
    (at, packet(CMD_MACRO_READ, sub, len as u16, n as u16 as u32, at, &vec![0xCC; len]))
}

/// escrita de macro: `data` já completo até múltiplo de 32 (config::macro_payload). Todo pacote leva 32
/// bytes; total = idx×4096 + tamanho (é o "fim" no app, não o tamanho).
pub fn macro_write_packets(sub: u16, idx: usize, data: &[u8]) -> Vec<(u32, Vec<u8>)> {
    let base = idx * crate::config::MACRO_BLOCK;
    let total = (base + data.len()) as u16 as u32;
    data.chunks(MACRO_CHUNK).enumerate().map(|(k, chunk)| {
        let at = (base + k * MACRO_CHUNK) as u16 as u32;
        (at, packet(CMD_MACRO_WRITE, sub, chunk.len() as u16, total, at, chunk))
    }).collect()
}

/// apagar macro: só cabeçalho; o app põe o tamanho (4096) no campo len/crc e o offset no fim
pub fn macro_erase_packet(sub: u16, idx: usize) -> Vec<u8> {
    let base = idx * crate::config::MACRO_BLOCK;
    packet(CMD_MACRO_ERASE, sub, crate::config::MACRO_BLOCK as u16, 0, base as u32, &[])
}

/// Interface de configuração: página vendor (06 xx FF) com relatório de saída 0x81.
pub fn find_hidraw() -> Option<(String, u16)> {
    let mut dirs: Vec<_> = fs::read_dir("/sys/class/hidraw").ok()?.flatten().map(|e| e.path()).collect();
    dirs.sort();
    for dev in dirs {
        let uevent = fs::read_to_string(dev.join("device/uevent")).unwrap_or_default();
        let rd = fs::read(dev.join("device/report_descriptor")).unwrap_or_default();
        let Some(id) = uevent.lines().find_map(|l| l.strip_prefix("HID_ID=")) else { continue };
        let parts: Vec<u32> = id.split(':').filter_map(|x| u32::from_str_radix(x, 16).ok()).collect();
        if parts.len() != 3 || parts[1] != VID as u32 {
            continue;
        }
        let vendor = rd.len() > 2 && rd[0] == 0x06 && rd[2] == 0xFF;
        if vendor && rd.windows(2).any(|w| w == [0x85, 0x81]) {
            return Some((format!("/dev/{}", dev.file_name()?.to_string_lossy()), parts[2] as u16));
        }
    }
    None
}

pub struct Dev {
    f: File,
    /// última resposta com o comando certo mas outro offset (para a mensagem de erro)
    stray_offset: Option<u32>,
}

fn wait_readable(fd: i32, ms: i32) -> bool {
    let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    unsafe { libc::poll(&mut p, 1, ms) > 0 && (p.revents & libc::POLLIN) != 0 }
}

impl Dev {
    pub fn open(path: &str) -> io::Result<Dev> {
        let f = OpenOptions::new().read(true).write(true).custom_flags(libc::O_NONBLOCK).open(path)?;
        Ok(Dev { f, stray_offset: None })
    }

    fn fd(&self) -> i32 {
        self.f.as_raw_fd()
    }

    fn read_one(&mut self) -> Option<Vec<u8>> {
        let mut b = [0u8; 64];
        match self.f.read(&mut b) {
            Ok(n) if n > 0 => Some(b[..n].to_vec()),
            _ => None,
        }
    }

    pub fn drain(&mut self) {
        while wait_readable(self.fd(), 0) {
            if self.read_one().is_none() {
                break;
            }
        }
    }

    fn send(&mut self, cmd: u16, sub: u16, len: u16, total: u32, offset: u32, data: &[u8]) -> io::Result<()> {
        self.f.write_all(&packet(cmd, sub, len, total, offset, data))
    }

    /// Espera 02 04 <tipo> <cmd> ...; com `offset`, descarta respostas de outro offset.
    fn recv(&mut self, typ: u16, cmd: u16, ms: u64, offset: Option<u32>) -> Option<(Vec<u8>, usize)> {
        let end = Instant::now() + Duration::from_millis(ms);
        loop {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() || !wait_readable(self.fd(), left.as_millis().max(1) as i32) {
                return None;
            }
            let Some(r) = self.read_one() else { continue };
            if r.len() >= 18 && r[0] == 0x02 && r[1] == 0x04 {
                let t = u16::from_le_bytes([r[2], r[3]]);
                let c = u16::from_le_bytes([r[4], r[5]]);
                let n = u16::from_le_bytes([r[6], r[7]]) as usize;
                let off = u32::from_le_bytes([r[14], r[15], r[16], r[17]]);
                if t == typ && c == cmd && offset.is_none_or(|o| o == off) {
                    return Some((r, n));
                }
                if t == typ && c == cmd {
                    self.stray_offset = Some(off);
                }
            }
        }
    }

    fn recv_cmd(&mut self, cmd: u16, ms: u64) -> Option<Vec<u8>> {
        let end = Instant::now() + Duration::from_millis(ms);
        loop {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() || !wait_readable(self.fd(), left.as_millis().max(1) as i32) {
                return None;
            }
            let Some(r) = self.read_one() else { continue };
            if r.len() >= 19 && r[0] == 0x02 && r[1] == 0x04 && u16::from_le_bytes([r[4], r[5]]) == cmd {
                return Some(r);
            }
        }
    }

    pub fn report_state(&mut self, on: bool) -> io::Result<()> {
        self.send(CMD_REPORT_STATE, on as u16, 0, 0, 0, &[])?;
        std::thread::sleep(Duration::from_millis(30));
        Ok(())
    }

    /// Envolve uma operação de configuração em report_state(0) … report_state(1), como o app.
    pub fn session<T>(&mut self, f: impl FnOnce(&mut Dev) -> io::Result<T>) -> io::Result<T> {
        self.drain();
        self.report_state(false)?;
        self.drain();
        let r = f(self);
        let _ = self.report_state(true);
        r
    }

    pub fn is_connected(&mut self) -> Option<bool> {
        for _ in 0..5 {
            self.send(CMD_CONNECT, 0, 0, 0, 0, &[]).ok()?;
            if let Some((r, _)) = self.recv(4, CMD_CONNECT, 500, None) {
                return Some(r[18] == 1);
            }
        }
        None
    }

    pub fn read_range(&mut self, off: usize, n: usize) -> io::Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(n);
        let mut tries = 0;
        while buf.len() < n {
            let at = off + buf.len();
            let len = CHUNK.min(n - buf.len());
            self.send(CMD_READ, 0, len as u16, CFG_SIZE as u32, at as u32, &[])?;
            match self.recv(4, CMD_READ, 500, Some(at as u32)) {
                Some((r, got)) if got > 0 => {
                    tries = 0;
                    let take = got.min(n - buf.len());
                    buf.extend_from_slice(&r[18..18 + take]);
                }
                _ => {
                    tries += 1;
                    if tries >= 30 {
                        return Err(io::Error::other(format!("sem resposta lendo offset {at}")));
                    }
                }
            }
        }
        Ok(buf)
    }

    pub fn read_config(&mut self) -> io::Result<Vec<u8>> {
        self.read_range(0, CFG_SIZE)
    }

    fn write_range(&mut self, off: usize, data: &[u8]) -> io::Result<()> {
        let mut pos = 0;
        let mut tries = 0;
        while pos < data.len() {
            let chunk = &data[pos..data.len().min(pos + CHUNK)];
            let at = (off + pos) as u32;
            self.send(CMD_WRITE, 0, chunk.len() as u16, CFG_SIZE as u32, at, chunk)?;
            match self.recv(4, CMD_WRITE, 500, Some(at)) {
                Some((_, n)) => {
                    tries = 0;
                    pos += if n > 0 { n } else { chunk.len() };
                }
                None => {
                    tries += 1;
                    if tries >= 30 {
                        return Err(io::Error::other(format!("sem confirmação escrevendo offset {at}")));
                    }
                }
            }
        }
        Ok(())
    }

    fn save(&mut self) -> io::Result<()> {
        std::thread::sleep(Duration::from_millis(100));
        self.send(CMD_SAVE, SAVE_SUB, 0, 0, 0, &[])?;
        self.recv(4, CMD_SAVE, 4000, None).map(|_| ()).ok_or_else(|| io::Error::other("o controle não confirmou o salvamento"))
    }

    /// Grava só os campos em que `new` difere de `base`, salva e relê esses trechos.
    /// Devolve (campos gravados, perfil ativo).
    pub fn apply_changes(&mut self, base: &[u8], new: &[u8]) -> io::Result<(Vec<(usize, usize)>, u16)> {
        let changed = diff_fields(base, new);
        for &(o, n) in &changed {
            self.write_range(o, &new[o..o + n])?;
        }
        if !changed.is_empty() {
            self.save()?;
            for &(o, n) in &changed {
                if self.read_range(o, n)? != new[o..o + n] {
                    return Err(io::Error::other(format!("releitura não bate no campo 0x{o:x}")));
                }
            }
        }
        let head = self.read_range(0, 0x14)?;
        Ok((changed, u16::from_le_bytes([head[0x12], head[0x13]])))
    }

    pub fn read_theme(&mut self) -> Option<u8> {
        for _ in 0..5 {
            self.send(CMD_THEME_READ, 0, 0, 0, 0, &[]).ok()?;
            if let Some((r, _)) = self.recv(4, CMD_THEME_READ, 500, None) {
                return Some(r[18]);
            }
        }
        None
    }

    pub fn write_theme(&mut self, t: u8) -> io::Result<()> {
        for _ in 0..5 {
            self.send(CMD_THEME_WRITE, t as u16, 0, 0, 0, &[])?;
            if self.recv(4, CMD_THEME_WRITE, 500, None).is_some() {
                return Ok(());
            }
        }
        Err(io::Error::other("o controle não confirmou o efeito de LED"))
    }

    fn stray(&mut self) -> String {
        match self.stray_offset.take() {
            Some(o) => format!(" (o controle respondeu com offset {o})"),
            None => String::new(),
        }
    }

    /// leitura de macro: `steps` passos da macro `idx` do perfil `slot`. Mesmas regras da leitura do
    /// struct: até 30 tentativas por pacote e offset da resposta conferido.
    pub fn read_macro(&mut self, mode: u16, slot: usize, idx: usize, steps: usize) -> io::Result<Vec<crate::config::Step>> {
        let n = steps.min(crate::config::MACRO_MAX_STEPS) * 10;
        let sub = macro_sub(mode, slot);
        let mut buf = Vec::with_capacity(n);
        let mut tries = 0;
        while buf.len() < n {
            let want = MACRO_CHUNK.min(n - buf.len());
            let (at, p) = macro_read_packet(sub, idx, buf.len(), n);
            self.f.write_all(&p)?;
            match self.recv(4, CMD_MACRO_READ, 500, Some(at)) {
                Some((r, got)) if got > 0 => {
                    tries = 0;
                    buf.extend_from_slice(&r[18..18 + got.min(want).min(r.len() - 18)]);
                }
                _ => {
                    tries += 1;
                    if tries >= 30 {
                        let s = self.stray();
                        return Err(io::Error::other(format!("sem resposta lendo a macro {} (offset {at}){s}", idx + 1)));
                    }
                }
            }
        }
        Ok(crate::config::steps_from_bytes(&buf))
    }

    /// apagar macro: apaga os 4096 bytes da macro; o app espera a resposta por até 3 s
    fn erase_macro(&mut self, mode: u16, slot: usize, idx: usize) -> io::Result<()> {
        self.f.write_all(&macro_erase_packet(macro_sub(mode, slot), idx))?;
        self.recv(4, CMD_MACRO_ERASE, 3000, None).map(|_| ())
            .ok_or_else(|| io::Error::other(format!("o controle não confirmou o apagamento da macro {}", idx + 1)))
    }

    /// gravação dos passos: apaga a área da macro, grava os passos em pacotes de 32 bytes, salva (cmd 6)
    /// e relê para conferir. Sem passos não faz nada (o app também pula count == 0).
    pub fn write_macro(&mut self, mode: u16, slot: usize, idx: usize, steps: &[crate::config::Step]) -> io::Result<()> {
        let steps = &steps[..steps.len().min(crate::config::MACRO_MAX_STEPS)];
        if steps.is_empty() {
            return Ok(());
        }
        self.erase_macro(mode, slot, idx)?;
        let data = crate::config::macro_payload(steps);
        for (at, p) in macro_write_packets(macro_sub(mode, slot), idx, &data) {
            let mut tries = 0;
            loop {
                self.f.write_all(&p)?;
                if self.recv(4, CMD_MACRO_WRITE, 500, Some(at)).is_some() {
                    break;
                }
                tries += 1;
                if tries >= 30 {
                    let s = self.stray();
                    return Err(io::Error::other(format!("sem confirmação gravando a macro {} (offset {at}){s}", idx + 1)));
                }
            }
        }
        self.save()?;
        if self.read_macro(mode, slot, idx, steps.len())? != steps {
            return Err(io::Error::other(format!("releitura da macro {} não bate", idx + 1)));
        }
        Ok(())
    }

    /// m_mode: 1 ou 2 (imagem do verso e se a aba Vibration aparece).
    pub fn read_variant(&mut self) -> u8 {
        for _ in 0..5 {
            if self.send(CMD_VARIANT, 0, 0, 0, 0, &[]).is_err() {
                break;
            }
            if let Some(r) = self.recv_cmd(CMD_VARIANT, 500) {
                return if r[18] == 1 { 1 } else { 2 };
            }
        }
        1
    }

    /// m_ControllerColor (2 MM, 3 XT, 4 LT; resto = padrão): comando cru 81 05 00 21 01.
    pub fn read_color(&mut self) -> u8 {
        let mut p = vec![0x81u8, 0x05, 0x00, 0x21, 0x01];
        p.resize(64, 0);
        if self.f.write_all(&p).is_err() {
            return 0;
        }
        let end = Instant::now() + Duration::from_millis(500);
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            if !wait_readable(self.fd(), left.as_millis().max(1) as i32) {
                break;
            }
            if let Some(r) = self.read_one() {
                if r.len() > 6 && r[0] == 0x02 && r[1] == 0x22 {
                    return r[6];
                }
            }
        }
        0
    }

    /// Estado ao vivo (readKeys): esvazia a fila, pergunta e fica com a resposta mais nova.
    pub fn read_keys(&mut self) -> Option<Keys> {
        self.drain();
        self.send(CMD_KEYS, KEYS_SUB, 0, 0, 0, &[]).ok()?;
        let (mut r, mut n) = self.recv(4, CMD_KEYS, 50, None)?;
        while wait_readable(self.fd(), 0) {
            let Some(x) = self.read_one() else { break };
            if x.len() >= 18 && x[0] == 0x02 && x[1] == 0x04
                && u16::from_le_bytes([x[2], x[3]]) == 4 && u16::from_le_bytes([x[4], x[5]]) == CMD_KEYS {
                n = u16::from_le_bytes([x[6], x[7]]) as usize;
                r = x;
            }
        }
        if n < 10 {
            return None;
        }
        let k = &r[18..];
        Some(Keys { buttons: u32::from_le_bytes([k[0], k[1], k[2], k[3]]), lx: k[4], ly: k[5], rx: k[6], ry: k[7], rt: k[8], lt: k[9] })
    }

    /// setRUMBLE: cmd 8, sub = duração (ms), intensidades 0-255 no campo len. Não grava nada.
    pub fn rumble(&mut self, left: u8, right: u8, ms: u16) {
        let _ = self.send(CMD_RUMBLE, ms, left as u16 | (right as u16) << 8, 0, 0, &[]);
        let _ = self.recv_cmd(CMD_RUMBLE, 100);
    }
}

/// Mesma trava do server.py: só uma instância (Python ou Rust) fala com o controle.
pub fn single_instance() -> Option<File> {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let f = File::create(format!("{dir}/u2tool.lock")).ok()?;
    let ok = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    ok.then_some(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Step, macro_payload};

    fn u16_at(p: &[u8], o: usize) -> u16 {
        u16::from_le_bytes([p[o], p[o + 1]])
    }
    fn u32_at(p: &[u8], o: usize) -> u32 {
        u32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]])
    }

    #[test]
    fn crc_modbus() {
        assert_eq!(crc16(b"123456789"), 0x4B37);
    }

    #[test]
    fn cabecalho_do_pacote() {
        let p = packet(1, 0, 3, 0x638, 0x1F4, &[1, 2, 3]);
        assert_eq!(p.len(), 64);
        assert_eq!(&p[..2], &[0x81, 0x04]);
        assert_eq!((u16_at(&p, 2), u16_at(&p, 4), u16_at(&p, 6)), (1, 0, 3));
        assert_eq!(u16_at(&p, 8), crc16(&[1, 2, 3]));
        assert_eq!((u32_at(&p, 10), u32_at(&p, 14)), (0x638, 0x1F4));
        assert_eq!(&p[18..21], &[1, 2, 3]);
        assert!(p[21..].iter().all(|&b| b == 0));
        // sem dados: crc 0
        assert_eq!(u16_at(&packet(6, 0x123, 0, 0, 0, &[]), 8), 0);
    }

    #[test]
    fn pacotes_de_macro() {
        let sub = macro_sub(3, 1);
        assert_eq!(sub, 0x0301);
        // leitura: 25 passos = 250 bytes da macro 2 (offset 4096)
        let (at, p) = macro_read_packet(sub, 1, 0, 250);
        assert_eq!(at, 4096);
        assert_eq!((u16_at(&p, 2), u16_at(&p, 4), u16_at(&p, 6)), (0x102, 0x0301, 32));
        assert_eq!((u32_at(&p, 10), u32_at(&p, 14)), (250, 4096));
        assert_eq!(&p[18..50], &[0xCC; 32]);
        assert_eq!(u16_at(&p, 8), crc16(&[0xCC; 32]));
        let (at, p) = macro_read_packet(sub, 1, 224, 250);
        assert_eq!((at, u16_at(&p, 6)), (4096 + 224, 26));

        // apagar: len = 4096, sem dados nem crc, offset no fim
        let e = macro_erase_packet(sub, 3);
        assert_eq!((u16_at(&e, 2), u16_at(&e, 6), u16_at(&e, 8)), (0x104, 0x1000, 0));
        assert_eq!((u32_at(&e, 10), u32_at(&e, 14)), (0, 3 * 4096));

        // gravar: 7 passos = 70 bytes → 96 (múltiplo de 32), 3 pacotes de 32
        let steps = vec![Step { ms: 30, keys: 0x2000, trigger: 0, left: Step::CENTER, right: Step::CENTER }; 7];
        let data = macro_payload(&steps);
        assert_eq!(data.len(), 96);
        assert_eq!(&data[70..80], &Step::NEUTRAL.to_bytes());
        let w = macro_write_packets(sub, 2, &data);
        assert_eq!(w.len(), 3);
        for (k, (at, p)) in w.iter().enumerate() {
            assert_eq!(*at, (2 * 4096 + 32 * k) as u32);
            assert_eq!((u16_at(p, 2), u16_at(p, 6)), (0x103, 32));
            assert_eq!(u32_at(p, 10), (2 * 4096 + 96) as u32);
            assert_eq!(u32_at(p, 14), *at);
            assert_eq!(&p[18..50], &data[32 * k..32 * k + 32]);
            assert_eq!(u16_at(p, 8), crc16(&data[32 * k..32 * k + 32]));
        }
    }

    #[test]
    fn passo_em_bytes() {
        let s = Step { ms: 0x1234, keys: 0x8000, trigger: 0, left: 0x007F, right: 0xFF7F };
        assert_eq!(s.to_bytes(), [0x34, 0x12, 0x00, 0x80, 0, 0, 0x7F, 0x00, 0x7F, 0xFF]);
        assert_eq!(Step::from_bytes(&s.to_bytes()), s);
        assert_eq!(macro_payload(&[]).len(), 0);
        assert_eq!(macro_payload(&vec![s; 16]).len(), 160);
    }
}
