//! Mede o custo do cálculo com os grupos reais do CelesTrak (em $TEMP/g_*.json, baixados à mão).
//! cargo test --release --test medida -- --ignored --nocapture
use passagens::orbita::{self, Lugar, Satelite};
use passagens::{perfis, tempo};
use std::time::Instant;

#[test]
#[ignore]
fn custo() {
    let dir = std::env::var("TEMP").unwrap();
    let mut sats = Vec::new();
    for g in ["resource", "weather"] {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{dir}/g_{g}.json")).unwrap()).unwrap();
        let t = Instant::now();
        for el in v.as_array().unwrap() {
            if let Some(s) = Satelite::de_gp(el)
                && !sats.iter().any(|x: &Satelite| x.norad == s.norad)
            {
                sats.push(s);
            }
        }
        println!("{g}: ler {:?}", t.elapsed());
    }
    let lugar = Lugar::new(-42.887, -20.695);
    let t1 = tempo::ler("2026-11-16T00:00:00Z").unwrap();
    let t0 = t1 - 7.0 * 86400.0;
    let t = Instant::now();
    let mut n = 0;
    let mut por = std::collections::BTreeMap::new();
    for s in &sats {
        if s.geoestacionario() {
            continue;
        }
        let p = orbita::passagens(s, perfis::perfil(&s.nome).as_ref(), &lugar, t0, t1);
        for x in &p {
            *por.entry(x.categoria.texto()).or_insert(0) += 1;
        }
        n += p.len();
    }
    println!(
        "{} satélites, {n} passagens {por:?}, {:?}, {} SGP4",
        sats.len(),
        t.elapsed(),
        orbita::CHAMADAS.load(std::sync::atomic::Ordering::Relaxed)
    );
    let t = Instant::now();
    for _ in 0..10000 {
        std::hint::black_box(sats[0].estado(t0 + 1000.0));
    }
    println!("1 SGP4: {:?}", t.elapsed() / 10000);
}
