//! O cálculo inteiro, sem falar com o RustGIS: dos elementos orbitais e das imagens do STAC à
//! camada de passagens e ao relatório. Roda na ferramenta `calcular`, que tem um orçamento de
//! instruções 60 vezes o de um comando (o SGP4 de ~300 satélites por 7 dias não cabe num comando).

use serde_json::{Value, json};

use crate::gratuitos;
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
    // Só os satélites com imagens que qualquer um baixa, e só as passagens em que a câmera (ou o
    // radar) viu o lugar.
    let satelites: Vec<Satelite> =
        e.gp.iter().filter_map(Satelite::de_gp).filter(|s| gratuitos::de(&s.nome).is_some() && !s.geoestacionario()).collect();
    let alvo = Lugar::new(e.lon, e.lat);
    let mut todas: Vec<Passagem> = Vec::new();
    for s in &satelites {
        let perfil = perfis::perfil(&s.nome);
        todas.extend(orbita::passagens(s, perfil.as_ref(), &alvo, e.t0, e.t1).into_iter().filter(|p| p.categoria == Categoria::Provavel));
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
    Saida { camada: json!({"type": "FeatureCollection", "features": feicoes}), relatorio: relatorio(e, &todas, &por_passagem), passagens }
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

fn relatorio(e: &Entrada, todas: &[Passagem], por_passagem: &[Vec<Imagem>]) -> String {
    let dias = ((e.t1 - e.t0) / 86_400.0).round();
    let n_imagens = por_passagem.iter().filter(|i| !i.is_empty()).count();
    let mut t = format!(
        "Nos últimos {dias:.0} dias, {} passagens de satélites de imagem gratuita viram {}; {n_imagens} delas têm a imagem no Planetary Computer (hora local):\n\n",
        todas.len(),
        e.nome
    );
    // As de resolução fina uma a uma, das mais novas às mais velhas; as diárias, resumidas.
    let mut ordem: Vec<usize> = (0..todas.len()).collect();
    ordem.sort_by(|&a, &b| todas[b].t.total_cmp(&todas[a].t));
    let mut diarias: Vec<(&str, usize)> = Vec::new();
    for i in ordem {
        let p = &todas[i];
        let Some(g) = gratuitos::de(&p.satelite) else { continue };
        if g.diario {
            match diarias.iter_mut().find(|(s, _)| *s == p.satelite) {
                Some((_, n)) => *n += 1,
                None => diarias.push((&p.satelite, 1)),
            }
            continue;
        }
        let imagem = match por_passagem[i].first() {
            Some(img) => {
                format!("imagem no Planetary Computer{}", img.nuvem.map(|n| format!(", {n:.0}% de nuvem na cena")).unwrap_or_default())
            }
            // Gravou se estava no plano da missão: a imagem, então, está no catálogo dela.
            None if g.condicao.is_some() => format!("se gravou: {}", g.onde),
            None => format!("ainda não no Planetary Computer: {}", g.onde),
        };
        t += &format!("- {} · {} · {imagem}\n", tempo::curto(p.t, e.fuso), p.satelite);
    }
    if !diarias.is_empty() {
        let lista: Vec<String> = diarias.iter().map(|(s, n)| format!("{s} {n}×")).collect();
        t += &format!(
            "\nDiários de resolução grossa (MODIS, VIIRS, Sentinel-3): {}; no NASA Worldview e no Copernicus Browser.\n",
            lista.join(", ")
        );
    }
    if let Some(n) = e.nuvem_agora {
        t += &format!("Nuvens agora: {n:.0}%.\n");
    }
    t += "\nÓrbitas do CelesTrak, calculadas com o SGP4.";
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
