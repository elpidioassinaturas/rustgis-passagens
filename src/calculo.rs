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
    /// De onde vieram as órbitas, para o relatório ("do CelesTrak, baixadas agora").
    pub orbitas: &'a str,
    /// As coleções ópticas ([`perfis::OPTICAS`]) sem nenhuma imagem do lugar em
    /// [`perfis::DIAS_COBERTURA`] dias: os satélites delas não gravam ali (o mar aberto), e as
    /// passagens deles não vão virar imagem.
    pub sem_cobertura: &'a [String],
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
    let passagens = match todas.len() {
        1 => "1 passagem de satélite de imagem gratuita viu".to_string(),
        n => format!("{n} passagens de satélites de imagem gratuita viram"),
    };
    let imagens = match n_imagens {
        0 => "nenhuma delas tem a imagem no Planetary Computer".to_string(),
        1 => "1 delas tem a imagem no Planetary Computer".to_string(),
        n => format!("{n} delas têm a imagem no Planetary Computer"),
    };
    // "No último dia" ou "Nos últimos 7 dias", no começo da frase.
    let periodo = tempo::periodo(dias, false).replacen('n', "N", 1);
    let mut t = format!("{periodo}, {passagens} {}; {imagens} (hora local):\n\n", e.nome);
    // As de resolução fina uma a uma, das mais novas às mais velhas; as diárias, resumidas.
    let mut ordem: Vec<usize> = (0..todas.len()).collect();
    ordem.sort_by(|&a, &b| todas[b].t.total_cmp(&todas[a].t));
    let mut diarias: Vec<(&str, usize)> = Vec::new();
    let mut finas = 0;
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
            // Sem nenhuma imagem da coleção aqui em meses: o satélite não grava este lugar.
            None if p.stac.and_then(perfis::colecao_do_stac).is_some_and(|c| e.sem_cobertura.iter().any(|s| s == c)) => format!(
                "não grava este lugar: o Planetary Computer não tem imagem dele aqui nos últimos {:.0} dias",
                perfis::DIAS_COBERTURA
            ),
            // Gravou se estava no plano da missão: a imagem, então, está no catálogo dela.
            None if g.condicao.is_some() => format!("se gravou: {}", g.onde),
            None => format!("ainda não no Planetary Computer: {}", g.onde),
        };
        t += &format!("- {} · {} · {imagem}\n", tempo::curto(p.t, e.fuso), p.satelite);
        finas += 1;
    }
    // Sem nenhuma de resolução fina, a lista acima ficava vazia (uma linha em branco).
    if finas == 0 {
        t += "Nenhuma passagem de resolução fina (Sentinel-1 e 2, Landsat, CBERS, Amazonia-1) no período.\n";
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
    t += &format!("\nÓrbitas {}, calculadas com o SGP4.", e.orbitas);
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
    fn relatorio_sem_passagens_de_resolucao_fina() {
        let t0 = tempo::ler("2026-11-09T00:00:00Z").unwrap();
        let e = Entrada {
            gp: &[],
            imagens: &[],
            lon: -42.87,
            lat: -20.75,
            t0,
            t1: t0 + 7.0 * 86_400.0,
            fuso: -3.0 * 3600.0,
            nome: "Viçosa",
            nuvem_agora: None,
            orbitas: "de teste",
            sem_cobertura: &[],
        };
        let r = calcular(&e).relatorio;
        assert!(r.starts_with("Nos últimos 7 dias, 0 passagens"), "{r}");
        assert!(r.contains("Nenhuma passagem de resolução fina"), "{r}");
        assert!(!r.contains("\n\n\n"), "{r}");
        let um_dia = calcular(&Entrada { t1: t0 + 86_400.0, ..e }).relatorio;
        assert!(um_dia.starts_with("No último dia, "), "{um_dia}");
    }

    #[test]
    fn satelite_que_nao_grava_o_lugar() {
        // O Sentinel-2B passou sobre Viçosa em 13/11. Sem imagem dele ali em meses (como no mar
        // aberto), a passagem não diz "ainda não no Planetary Computer".
        let gp = [
            json!({"OBJECT_NAME":"SENTINEL-2B","OBJECT_ID":"2017-013A","EPOCH":"2026-11-15T19:34:54.704352","MEAN_MOTION":14.30813686,"ECCENTRICITY":0.0001114,"INCLINATION":98.5709,"RA_OF_ASC_NODE":32.4267,"ARG_OF_PERICENTER":90.3469,"MEAN_ANOMALY":269.7841,"EPHEMERIS_TYPE":0,"CLASSIFICATION_TYPE":"U","NORAD_CAT_ID":42063,"ELEMENT_SET_NO":999,"REV_AT_EPOCH":50632,"BSTAR":9.0686e-05,"MEAN_MOTION_DOT":1.94e-06,"MEAN_MOTION_DDOT":0}),
        ];
        let t0 = tempo::ler("2026-11-13T00:00:00Z").unwrap();
        let sem = ["sentinel-2-l2a".to_string()];
        let e = Entrada {
            gp: &gp,
            imagens: &[],
            lon: -42.887,
            lat: -20.695,
            t0,
            t1: t0 + 86_400.0,
            fuso: -3.0 * 3600.0,
            nome: "Viçosa",
            nuvem_agora: None,
            orbitas: "de teste",
            sem_cobertura: &sem,
        };
        let r = calcular(&e).relatorio;
        assert!(r.contains("SENTINEL-2B · não grava este lugar"), "{r}");
        let com_cobertura = calcular(&Entrada { sem_cobertura: &[], ..e }).relatorio;
        assert!(com_cobertura.contains("SENTINEL-2B · ainda não no Planetary Computer"), "{com_cobertura}");
    }

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
