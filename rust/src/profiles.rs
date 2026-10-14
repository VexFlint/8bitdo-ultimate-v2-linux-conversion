//! Lista de perfis local ("Profile List"): mesmo diretório e formato do server.py
//! (~/.local/share/u2tool/perfis/<id>.json com {nome, config em base64}) e, desde as macros,
//! "passos": os passos das 4 macros (passo do struct de configuração).

use crate::config::Step;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::path::PathBuf;

pub type MacroSteps = [Vec<Step>; 4];

#[derive(Clone, Debug)]
pub struct FileProfile {
    pub id: String,
    pub name: String,
    pub config: Vec<u8>,
    pub macros: MacroSteps,
}

fn dir() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME").map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share"));
    base.join("u2tool/perfis")
}

fn steps_json(m: &MacroSteps) -> serde_json::Value {
    m.iter().map(|steps| steps.iter().map(|s| serde_json::json!([s.ms, s.keys, s.trigger, s.left, s.right])).collect::<Vec<_>>())
        .collect::<Vec<_>>().into()
}
fn steps_from_json(j: Option<&serde_json::Value>) -> MacroSteps {
    let mut out: MacroSteps = Default::default();
    for (k, steps) in out.iter_mut().enumerate() {
        if let Some(a) = j.and_then(|j| j.get(k)).and_then(|m| m.as_array()) {
            *steps = a.iter().filter_map(|s| {
                let v: Vec<u16> = s.as_array()?.iter().filter_map(|x| x.as_u64().map(|x| x as u16)).collect();
                (v.len() == 5).then(|| Step { ms: v[0], keys: v[1], trigger: v[2], left: v[3], right: v[4] })
            }).collect();
        }
    }
    out
}

pub fn list() -> Vec<FileProfile> {
    let d = dir();
    let _ = std::fs::create_dir_all(&d);
    let mut v: Vec<_> = std::fs::read_dir(&d).into_iter().flatten().flatten()
        .filter_map(|e| {
            let p = e.path();
            let id = p.file_stem()?.to_str()?.to_string();
            if p.extension()? != "json" { return None; }
            let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).ok()?).ok()?;
            let config = STANDARD.decode(j.get("config")?.as_str()?).ok()?;
            Some(FileProfile { id, name: j.get("nome")?.as_str()?.to_string(), config, macros: steps_from_json(j.get("passos")) })
        })
        .filter(|f| f.config.len() == crate::proto::CFG_SIZE)
        .collect();
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v
}

pub fn save(id: Option<&str>, name: &str, config: &[u8], macros: &MacroSteps) -> std::io::Result<String> {
    let id = id.map(str::to_string).unwrap_or_else(|| {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        format!("{:012x}", t & 0xFFFF_FFFF_FFFF)
    });
    let _ = std::fs::create_dir_all(dir());
    let j = serde_json::json!({ "nome": name.chars().take(16).collect::<String>(), "config": STANDARD.encode(config),
                                "passos": steps_json(macros) });
    std::fs::write(dir().join(format!("{id}.json")), j.to_string())?;
    Ok(id)
}

pub fn delete(id: &str) {
    if id.chars().all(|c| c.is_ascii_alphanumeric()) {
        let _ = std::fs::remove_file(dir().join(format!("{id}.json")));
    }
}
