//! O cálculo inteiro, sem falar com o RustGIS: dos elementos orbitais e das imagens do STAC à
//! camada de passagens e ao relatório. Roda na ferramenta `calcular`, que tem um orçamento de
//! instruções 60 vezes o de um comando (o SGP4 de ~300 satélites por 7 dias não cabe num comando).

use serde_json::{Value, json};

use crate::imagens::{self, Imagem};
use crate::orbita::{self, Categoria, Lugar, Passagem, Satelite};
use crate::perfis;
use crate::tempo;

/// A margem do traço desenhado de cada lado da maior aproximação, em segundos.
const MEIO_TRACO: f64 = 150.0;

/// O que o cálculo recebe.
pub struct Entrada<'a> {
    /// Os elementos GP (OMM em JSON) do CelesTrak, já sem repetições.
    pub gp: &'a [Value],
    pub imagens: &'a [Imagem],
    pub lon: f64,
    pub lat: f64,
    pub t0: f64,
    pub t1: f64,
    /// O fuso do lugar, em segundos à frente do UTC.
    pub fuso: f64,
    /// O nome do lugar.
    pub nome: &'a str,
    /// A nuvem de agora no lugar, em %.
    pub nuvem_agora: Option<f64>,
}

/// O resultado: a camada (GeoJSON) e o relatório.
pub struct Saida {
    pub camada: Value,
    pub relatorio: String,
    pub passagens: usize,
}

pub fn calcular(e: &Entrada) -> Saida {
    let satelites: Vec<Satelite> = e.gp.iter().filter_map(Satelite::de_gp).collect();
    let alvo = Lugar::new(e.lon, e.lat);
    let mut todas: Vec<Passagem> = Vec::new();
    let mut geo = Vec::new();
    for s in &satelites {
        if s.geoestacionario() {
            if let Some((r, _)) = s.estado(e.t1)
                && alvo.elevacao(r) > 10.0
            {
                geo.push(s.nome.clone());
            }
            continue;
        }
        let perfil = perfis::perfil(&s.nome);
        todas.extend(orbita::passagens(s, perfil.as_ref(), &alvo, e.t0, e.t1));
    }
    todas.sort_by(|a, b| a.t.total_cmp(&b.t));
    let por_passagem = imagens::casar(&mut todas, e.imagens);

    // As feições vão da categoria menos importante à mais, que o mapa desenha por cima; dentro
    // de cada uma, pela hora.
    let mut ordem: Vec<usize> = (0..todas.len()).collect();
    ordem.sort_by(|&a, &b| todas[b].categoria.cmp(&todas[a].categoria).then(todas[a].t.total_cmp(&todas[b].t)));
    let feicoes: Vec<Value> = ordem
        .iter()
        .map(|&i| (&todas[i], &por_passagem[i]))
        .filter_map(|(p, imgs)| {
            let sat = satelites.iter().find(|s| s.norad == p.norad)?;
            let melhor = imgs.first();
            Some(json!({
                "type": "Feature",
                "geometry": linha(&sat.traco(p.t, MEIO_TRACO, 5))?,
                "properties": {
                    "satelite": p.satelite,
                    "norad": p.norad,
                    "categoria": p.categoria.texto(),
                    "hora_utc": tempo::iso(p.t),
                    "hora_local": tempo::local(p.t, e.fuso),
                    "dist_km": (p.distancia * 10.0).round() / 10.0,
                    "elevacao": (p.elevacao * 10.0).round() / 10.0,
                    "sol": (p.sol * 10.0).round() / 10.0,
                    "altitude_km": p.altitude.round(),
                    "lado": if p.direita { "direita" } else { "esquerda" },
                    "imagens": imgs.len(),
                    "imagem": melhor.map(|i| i.id.as_str()).unwrap_or_default(),
                    "colecao": melhor.map(|i| i.colecao.as_str()).unwrap_or_default(),
                    "nuvem": melhor.and_then(|i| i.nuvem).map(|n| (n * 10.0).round() / 10.0),
                    "url": melhor.map(|i| i.url.as_str()).unwrap_or_default(),
                },
            }))
        })
        .collect();
    let passagens = feicoes.len();
    Saida {
        camada: json!({"type": "FeatureCollection", "features": feicoes}),
        relatorio: relatorio(e, &todas, &por_passagem, &geo, satelites.len()),
        passagens,
    }
}

/// Uma linha, cortada onde cruza o antimeridiano.
pub fn linha(traco: &[[f64; 2]]) -> Option<Value> {
    let mut partes: Vec<Vec<[f64; 2]>> = vec![Vec::new()];
    for p in traco {
        let atual = partes.last_mut()?;
        if let Some(u) = atual.last()
            && (p[0] - u[0]).abs() > 180.0
        {
            partes.push(vec![*p]);
        } else {
            atual.push(*p);
        }
    }
    partes.retain(|p| p.len() >= 2);
    match partes.len() {
        0 => None,
        1 => Some(json!({"type": "LineString", "coordinates": partes[0]})),
        _ => Some(json!({"type": "MultiLineString", "coordinates": partes})),
    }
}

fn relatorio(e: &Entrada, todas: &[Passagem], por_passagem: &[Vec<Imagem>], geo: &[String], n_satelites: usize) -> String {
    let dias = ((e.t1 - e.t0) / 86_400.0).round();
    let sats = |c: Categoria| {
        let mut v: Vec<&str> = todas.iter().filter(|p| p.categoria == c).map(|p| p.satelite.as_str()).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let mut distintos: Vec<u64> = todas.iter().filter(|p| p.categoria != Categoria::PassouPorCima).map(|p| p.norad).collect();
    distintos.sort_unstable();
    distintos.dedup();
    let n_imagens: usize = por_passagem.iter().map(Vec::len).sum();
    let mut t = format!(
        "Nos últimos {dias:.0} dias, {} satélites de observação podem ter visto {}; {n_imagens} imagens públicas foram encontradas.\n\n",
        distintos.len(),
        e.nome
    );
    let explica = [
        (Categoria::Fotografou, "há imagem pública"),
        (Categoria::Provavel, "a faixa da câmera cobriu o lugar, sem imagem publicada"),
        (Categoria::PoderiaApontar, "câmera apontável ao alcance"),
        (Categoria::PassouPorCima, "cruzou o céu, a mais de 45° do horizonte"),
    ];
    for (c, o_que) in explica {
        let n = todas.iter().filter(|p| p.categoria == c).count();
        let s = sats(c);
        let exemplos = match s.len() {
            0 => String::new(),
            1..=6 => format!(": {}", s.join(", ")),
            _ => format!(": {} e mais {}", s[..6].join(", "), s.len() - 6),
        };
        t += &format!("- {n} passagens: {} ({o_que}){exemplos}\n", c.texto());
    }
    if !geo.is_empty() {
        t += &format!("\n{} geoestacionários veem o lugar o tempo todo: {}.\n", geo.len(), geo.join(", "));
    }
    if let Some(n) = e.nuvem_agora {
        t += &format!("Nuvens agora: {n:.0}%.\n");
    }
    t += &format!("\n{n_satelites} satélites calculados (CelesTrak, SGP4); imagens do Microsoft Planetary Computer.");
    t
}

/// O nome legível do satélite de um item do STAC (`S2B` → Sentinel-2B, `LC09` → Landsat 9).
pub fn satelite_legivel(id: &str) -> String {
    let s = perfis::satelite_do_item(id);
    match s.as_bytes() {
        [b'S', m @ (b'1' | b'2'), l] => format!("Sentinel-{}{}", *m as char, *l as char),
        [b'L', _, b'0', n] => format!("Landsat {}", *n as char),
        _ => s.to_string(),
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn nomes_e_linhas() {
        assert_eq!(satelite_legivel("S2B_MSIL2A_20261113T130249_R095_T23KQT"), "Sentinel-2B");
        assert_eq!(satelite_legivel("LC09_L2SP_217074_20261110_02_T1"), "Landsat 9");
        assert_eq!(satelite_legivel("S1C_IW_GRDH_1SDV"), "Sentinel-1C");
        let l = linha(&[[179.0, 1.0], [179.9, 2.0], [-179.8, 3.0], [-179.0, 4.0]]).unwrap();
        assert_eq!(l["type"], "MultiLineString");
        assert_eq!(linha(&[[1.0, 1.0], [2.0, 2.0]]).unwrap()["type"], "LineString");
        assert!(linha(&[[1.0, 1.0]]).is_none());
    }
}
