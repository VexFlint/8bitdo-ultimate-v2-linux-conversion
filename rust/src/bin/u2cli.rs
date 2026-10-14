//! Linha de comando para conferir o protocolo em Rust contra o u2tool.py.
//!   u2cli dump saida.bin   — lê a configuração inteira
//!   u2cli keys             — mostra o estado ao vivo por 3 s
//!   u2cli gate             — liga/desliga um bit e confere (teste de escrita)
//!   u2cli macros           — lista as macros e lê os passos (só leitura)
//!   u2cli macro-teste N    — grava uma macro de teste na 4ª macro do perfil N (1-3), relê e desfaz

use std::time::{Duration, Instant};
use u2tool::config::{Config, Macro, Step, key_name};
use u2tool::proto::{self, Dev};

fn print_macros(d: &mut Dev, cfg: &Config) -> std::io::Result<()> {
    println!("modo {} (0 Switch, 1 DInput, 2 Mac, 3 XInput), perfil ativo {}", cfg.mode(), cfg.cur_slot());
    for i in 0..3 {
        if !cfg.macros_enabled(i) {
            println!("perfil {}: sem macros", i + 1);
            continue;
        }
        for k in 0..4 {
            let Some(m) = cfg.active_macro(i, k) else { continue };
            println!("perfil {} macro {}: \"{}\" tecla {} ({:#x}) passos {} ciclos {:#x} intervalo {} ms",
                     i + 1, k + 1, m.name, key_name(m.key), m.key, m.steps, m.cycles, m.interval);
            if m.steps > 0 {
                for (n, s) in d.read_macro(cfg.mode(), i, k, m.steps as usize)?.iter().enumerate() {
                    println!("  {n:3}: {:5} ms  botões {:#06x}  gatilho {:#06x}  LS {:#06x}  RS {:#06x}", s.ms, s.keys, s.trigger, s.left, s.right);
                }
            }
        }
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (path, pid) = proto::find_hidraw().expect("Ultimate 2 não encontrado");
    eprintln!("{path} (pid {pid:04x})");
    let mut d = Dev::open(&path).expect("abrir hidraw");
    match args.get(1).map(String::as_str) {
        Some("dump") => {
            let cfg = d.session(|d| d.read_config()).expect("leitura");
            std::fs::write(&args[2], &cfg).unwrap();
            println!("{} bytes", cfg.len());
        }
        Some("keys") => {
            let t0 = Instant::now();
            let (mut n, mut ok) = (0, 0);
            while t0.elapsed() < Duration::from_secs(3) {
                n += 1;
                if d.read_keys().is_some() {
                    ok += 1;
                }
            }
            println!("{ok}/{n} leituras em 3 s; {:?}", d.read_keys());
        }
        Some("gate") => {
            d.session(|d| {
                let base = d.read_config()?;
                let mut new = base.clone();
                new[0xC8 + 4] ^= 1;
                let (ch, slot) = d.apply_changes(&base, &new)?;
                println!("gravado {:x?}, perfil ativo {slot}", ch);
                let now = d.read_config()?;
                println!("só mudou {:x?}", proto::diff_fields(&base, &now));
                d.apply_changes(&now, &base)?;
                println!("restaurado: {}", d.read_config()? == base);
                Ok(())
            })
            .expect("teste");
        }
        Some("macros") => {
            d.session(|d| {
                let cfg = Config::new(d.read_config()?);
                print_macros(d, &cfg)
            })
            .expect("leitura das macros");
        }
        Some("macro-teste") => {
            let i: usize = args.get(2).and_then(|s| s.parse::<usize>().ok()).filter(|n| (1..=3).contains(n)).expect("uso: u2cli macro-teste 1|2|3") - 1;
            d.session(|d| {
                let base = d.read_config()?;
                let cfg = Config::new(base.clone());
                if cfg.cur_slot() as usize == i {
                    return Err(std::io::Error::other("escolha um perfil que não esteja ativo no controle"));
                }
                if !cfg.used(i) {
                    return Err(std::io::Error::other("o perfil precisa existir (crie no app antes)"));
                }
                let mut defs = cfg.macro_defs(i);
                if defs[3].is_some() {
                    return Err(std::io::Error::other("a 4ª macro desse perfil está em uso; escolha outro perfil"));
                }
                // A por 50 ms, solta, 50 ms — na tecla P4 (R4)
                let steps = vec![Step { ms: 50, keys: 0x2000, trigger: 0, left: Step::CENTER, right: Step::CENTER },
                                 Step { ms: 50, keys: 0, trigger: 0, left: Step::CENTER, right: Step::CENTER }];
                defs[3] = Some(Macro { name: "teste".into(), key: 0x4000_0000, steps: 2, cycles: 1, interval: 0 });
                let mut new = cfg.clone();
                new.set_macros(i, &defs);
                println!("1/4 gravando o registro da macro (cmd 1)…");
                d.apply_changes(&base, &new.b)?;
                println!("2/4 apagando e gravando os passos (cmd 0x104 / 0x103) e relendo (cmd 0x102)…");
                d.write_macro(cfg.mode(), i, 3, &steps)?;
                println!("3/4 conferido. Como ficou:");
                print_macros(d, &new)?;
                println!("4/4 desfazendo o registro (os passos ficam na flash, sem tecla)…");
                let now = d.read_config()?;
                d.apply_changes(&now, &base)?;
                println!("restaurado: {}", d.read_config()? == base);
                Ok(())
            })
            .expect("teste de macro");
        }
        _ => eprintln!("uso: u2cli dump arquivo | keys | gate | macros | macro-teste 1|2|3"),
    }
}
