//! As próximas passagens dos satélites de imagem gratuita sobre um lugar, cada uma com a nuvem
//! prevista para a hora dela. Roda na ferramenta `proximas`, como o cálculo do passado.

use serde_json::{Value, json};

use crate::calculo::linha;
use crate::gratuitos::{self, Gratuito};
use crate::orbita::{self, Categoria, Lugar, Satelite};
use crate::perfis;
use crate::tempo;

/// Até quanto de nuvem prevista o céu conta como limpo, e a partir de quanto como nublado (%).
pub const LIMPO: f64 = 30.0;
pub const NUBLADO: f64 = 70.0;

/// O que o cálculo recebe.
pub struct Entrada<'a> {
    /// Os elementos GP (OMM) dos satélites.
    pub gp: &'a [Value],
    pub lon: f64,
    pub lat: f64,
    pub t0: f64,
    pub t1: f64,
    /// O fuso do lugar, em segundos à frente do UTC.
    pub fuso: f64,
    pub nome: &'a str,
    /// A previsão de nuvem hora a hora: (hora UTC, %), em ordem.
    pub nuvens: &'a [(f64, f64)],
    /// Inclui os de resolução grossa que passam todo dia (MODIS, VIIRS, Sentinel-3).
    pub diarios: bool,
    /// As coleções ópticas sem nenhuma imagem do lugar em [`perfis::DIAS_COBERTURA`] dias: os
    /// satélites delas não gravam ali (o mar aberto), e as passagens deles ficam de fora.
    pub sem_cobertura: &'a [String],
}

pub struct Saida {
    pub camada: Value,
    pub relatorio: String,
    pub passagens: usize,
}

/// O céu previsto para uma passagem.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ceu {
    Limpo,
    Parcial,
    Nublado,
    /// Radar: a nuvem não importa.
    Radar,
    /// Além do alcance da previsão.
    SemPrevisao,
}

impl Ceu {
    pub fn de(nuvem: Option<f64>, radar: bool) -> Ceu {
        match (radar, nuvem) {
            (true, _) => Ceu::Radar,
            (_, None) => Ceu::SemPrevisao,
            (_, Some(n)) if n <= LIMPO => Ceu::Limpo,
            (_, Some(n)) if n < NUBLADO => Ceu::Parcial,
            _ => Ceu::Nublado,
        }
    }

    pub fn texto(self) -> &'static str {
        match self {
            Ceu::Limpo => "céu limpo",
            Ceu::Parcial => "parcialmente nublado",
            Ceu::Nublado => "nublado",
            Ceu::Radar => "radar (a nuvem não importa)",
            Ceu::SemPrevisao => "sem previsão de nuvem",
        }
    }

    pub fn cor(self) -> &'static str {
        match self {
            Ceu::Limpo => "#2E9E4F",
            Ceu::Parcial => "#E0A030",
            Ceu::Nublado => "#8A8F98",
            Ceu::Radar => "#3A6FD8",
            Ceu::SemPrevisao => "#B9BDC4",
        }
    }

    pub fn largura(self) -> f64 {
        match self {
            Ceu::Limpo => 3.0,
            Ceu::Radar => 2.0,
            Ceu::Parcial => 2.0,
            _ => 1.2,
        }
    }

    pub const TODOS: [Ceu; 5] = [Ceu::Limpo, Ceu::Parcial, Ceu::Nublado, Ceu::Radar, Ceu::SemPrevisao];
}

/// A nuvem prevista na hora `t`, interpolada entre as horas da previsão; `None` fora dela.
pub fn nuvem_em(nuvens: &[(f64, f64)], t: f64) -> Option<f64> {
    let i = nuvens.partition_point(|(h, _)| *h <= t);
    match (i.checked_sub(1).and_then(|j| nuvens.get(j)), nuvens.get(i)) {
        (Some(a), Some(b)) => Some(a.1 + (b.1 - a.1) * (t - a.0) / (b.0 - a.0)),
        (Some(a), None) if t - a.0 <= 3600.0 => Some(a.1),
        _ => None,
    }
}

/// Uma passagem que vê o lugar.
struct Prevista {
    t: f64,
    satelite: String,
    norad: u64,
    gratuito: &'static Gratuito,
    nuvem: Option<f64>,
    ceu: Ceu,
    distancia: f64,
    direita: bool,
    sol: f64,
}

pub fn calcular(e: &Entrada) -> Saida {
    let satelites: Vec<Satelite> = e.gp.iter().filter_map(Satelite::de_gp).collect();
    let alvo = Lugar::new(e.lon, e.lat);
    let mut previstas: Vec<Prevista> = Vec::new();
    // Os que passam mas não gravam o lugar (o Sentinel-2 e o Landsat no mar aberto).
    let mut nao_gravam: Vec<&str> = Vec::new();
    for s in &satelites {
        let Some(g) = gratuitos::de(&s.nome) else { continue };
        if g.diario && !e.diarios {
            continue;
        }
        let perfil = perfis::perfil(&s.nome);
        let colecao = perfil.as_ref().and_then(|p| p.stac).and_then(perfis::colecao_do_stac);
        let nao_grava = colecao.is_some_and(|c| e.sem_cobertura.iter().any(|s| s == c));
        for p in orbita::passagens(s, perfil.as_ref(), &alvo, e.t0, e.t1) {
            // Só as que veem o lugar: a faixa da câmera, de dia, ou a do radar.
            if p.categoria != Categoria::Provavel {
                continue;
            }
            // Passa, mas não grava: fica de fora, e o relatório diz por quê.
            if nao_grava {
                if !nao_gravam.contains(&g.prefixo) {
                    nao_gravam.push(g.prefixo);
                }
                continue;
            }
            let nuvem = if g.radar { None } else { nuvem_em(e.nuvens, p.t) };
            previstas.push(Prevista {
                t: p.t,
                satelite: p.satelite,
                norad: p.norad,
                gratuito: g,
                nuvem,
                ceu: Ceu::de(nuvem, g.radar),
                distancia: p.distancia,
                direita: p.direita,
                sol: p.sol,
            });
        }
    }
    previstas.sort_by(|a, b| a.t.total_cmp(&b.t));

    // As de céu limpo por cima das outras no mapa.
    let mut ordem: Vec<&Prevista> = previstas.iter().collect();
    ordem.sort_by(|a, b| b.ceu.cmp(&a.ceu).then(a.t.total_cmp(&b.t)));
    let feicoes: Vec<Value> = ordem
        .iter()
        .filter_map(|p| {
            let sat = satelites.iter().find(|s| s.norad == p.norad)?;
            Some(json!({
                "type": "Feature",
                "geometry": linha(&sat.traco(p.t, 150.0, 5))?,
                "properties": {
                    "satelite": p.satelite,
                    "quando": tempo::curto(p.t, e.fuso),
                    "hora_local": tempo::local(p.t, e.fuso),
                    "hora_utc": tempo::iso(p.t),
                    "ceu": p.ceu.texto(),
                    "nuvem_prevista": p.nuvem.map(|n| n.round()),
                    "resolucao": p.gratuito.resolucao,
                    "onde": p.gratuito.onde,
                    "condicao": p.gratuito.condicao.unwrap_or("grava toda a terra que vê de dia"),
                    "dist_km": (p.distancia * 10.0).round() / 10.0,
                    "lado": if p.direita { "direita" } else { "esquerda" },
                    "sol": (p.sol * 10.0).round() / 10.0,
                },
            }))
        })
        .collect();
    let passagens = feicoes.len();
    nao_gravam.sort_unstable();
    Saida { camada: json!({"type": "FeatureCollection", "features": feicoes}), relatorio: relatorio(e, &previstas, &nao_gravam), passagens }
}

fn relatorio(e: &Entrada, previstas: &[Prevista], nao_gravam: &[&str]) -> String {
    let dias = ((e.t1 - e.t0) / 86_400.0).round();
    let mut t = format!(
        "Próximas passagens de satélites de imagem gratuita sobre {}, {} (hora local, nuvem prevista pelo Open-Meteo):\n\n",
        e.nome,
        tempo::periodo(dias, true)
    );
    if previstas.is_empty() {
        t += "Nenhuma passagem com o lugar dentro da faixa de um deles.\n";
    }
    for p in previstas {
        let nuvem = match (p.gratuito.radar, p.nuvem) {
            (true, _) => "radar, a nuvem não importa".to_string(),
            (_, Some(n)) => format!("nuvem prevista {n:.0}%"),
            (_, None) => "sem previsão de nuvem".to_string(),
        };
        let condicao = p.gratuito.condicao.map(|c| format!(" · {c}")).unwrap_or_default();
        t += &format!("- {} · {} ({}) · {nuvem}{condicao}\n", tempo::curto(p.t, e.fuso), p.satelite, p.gratuito.resolucao);
    }
    if !nao_gravam.is_empty() {
        let dias = perfis::DIAS_COBERTURA;
        t += &if nao_gravam.len() == 1 {
            format!(
                "\n{} passa por aqui, mas não grava este lugar: o Planetary Computer não tem imagem dele aqui nos últimos {dias:.0} dias (o mar aberto, longe da costa). As passagens dele ficaram de fora.\n",
                nao_gravam[0]
            )
        } else {
            let (ultimo, outros) = nao_gravam.split_last().unwrap_or((&"", &[]));
            format!(
                "\n{} e {ultimo} passam por aqui, mas não gravam este lugar: o Planetary Computer não tem imagem deles aqui nos últimos {dias:.0} dias (o mar aberto, longe da costa). As passagens deles ficaram de fora.\n",
                outros.join(", ")
            )
        };
    }
    // A melhor aposta: a primeira passagem óptica que vira imagem com certeza e de céu limpo.
    let garantida = |p: &&Prevista| !p.gratuito.radar && p.gratuito.condicao.is_none() && !p.gratuito.diario;
    match previstas.iter().filter(garantida).find(|p| p.ceu == Ceu::Limpo) {
        // Sem nenhuma passagem do Sentinel-2 ou do Landsat, porque eles não gravam aqui: a frase
        // acima já diz isso.
        None if !nao_gravam.is_empty() && !previstas.iter().any(|p| garantida(&p)) => {}
        Some(p) => {
            t += &format!(
                "\nA próxima imagem limpa provável: {} em {}, com {:.0}% de nuvem prevista.\n",
                p.satelite,
                tempo::curto(p.t, e.fuso),
                p.nuvem.unwrap_or_default()
            )
        }
        None => {
            t += &format!(
                "\nNenhuma passagem do Sentinel-2 ou do Landsat tem {LIMPO:.0}% de nuvem prevista ou menos no período da previsão."
            );
            let menos = previstas
                .iter()
                .filter(garantida)
                .filter(|p| p.nuvem.is_some())
                .min_by(|a, b| a.nuvem.unwrap_or(f64::MAX).total_cmp(&b.nuvem.unwrap_or(f64::MAX)));
            if let Some(p) = menos {
                t += &format!(
                    " A de menos nuvem: {} em {}, com {:.0}%.",
                    p.satelite,
                    tempo::curto(p.t, e.fuso),
                    p.nuvem.unwrap_or_default()
                );
            }
            t += "\n";
        }
    }
    if !e.diarios {
        // O painel não tem a opção: só o formulário do comando Próximas imagens, na faixa.
        t += "Os de resolução grossa que passam todo dia (MODIS, VIIRS, Sentinel-3) ficaram de fora: para vê-los, rode Próximas imagens na aba Satélites com Incluir os diários de resolução grossa marcado.\n";
    }
    // Onde baixar, com os satélites de cada fonte.
    let mut onde: Vec<(&str, Vec<&str>)> = Vec::new();
    for p in previstas {
        let fonte = p.gratuito.onde;
        let sat = p.gratuito.prefixo;
        match onde.iter_mut().find(|(o, _)| *o == fonte) {
            Some((_, sats)) if !sats.contains(&sat) => sats.push(sat),
            Some(_) => {}
            None => onde.push((fonte, vec![sat])),
        }
    }
    if !onde.is_empty() {
        t += "\nOnde baixar:\n";
        for (fonte, sats) in onde {
            t += &format!("- {}: {fonte}\n", sats.join(", "));
        }
    }
    t += "A nuvem prevista para mais de 5 a 7 dias é pouco confiável; as horas das passagens erram poucos segundos.";
    t
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::orbita::testes::s2b;

    #[test]
    fn nuvem_interpolada() {
        let n = [(0.0, 10.0), (3600.0, 50.0)];
        assert_eq!(nuvem_em(&n, 1800.0), Some(30.0));
        assert_eq!(nuvem_em(&n, 0.0), Some(10.0));
        assert_eq!(nuvem_em(&n, 3600.0 + 1800.0), Some(50.0));
        assert_eq!(nuvem_em(&n, 3.0 * 3600.0), None);
        assert_eq!(nuvem_em(&n, -1.0), None);
        assert_eq!(Ceu::de(Some(30.0), false), Ceu::Limpo);
        assert_eq!(Ceu::de(Some(80.0), true), Ceu::Radar);
        assert_eq!(Ceu::de(None, false), Ceu::SemPrevisao);
    }

    /// O Sentinel-2B, que passa sobre Viçosa em 13/11 perto das 13:03 UTC, passa de novo dez
    /// dias depois (o ciclo dele), no mesmo traço e na mesma hora.
    #[test]
    fn sentinel_2b_volta_em_dez_dias() {
        let s = s2b();
        let gp = json!([{"OBJECT_NAME":"SENTINEL-2B","OBJECT_ID":"2017-013A","EPOCH":"2026-11-15T19:34:54.704352","MEAN_MOTION":14.30813686,"ECCENTRICITY":0.0001114,"INCLINATION":98.5709,"RA_OF_ASC_NODE":32.4267,"ARG_OF_PERICENTER":90.3469,"MEAN_ANOMALY":269.7841,"EPHEMERIS_TYPE":0,"CLASSIFICATION_TYPE":"U","NORAD_CAT_ID":42063,"ELEMENT_SET_NO":999,"REV_AT_EPOCH":50632,"BSTAR":9.0686e-05,"MEAN_MOTION_DDOT":0,"MEAN_MOTION_DOT":1.94e-06}]);
        assert_eq!(s.nome, "SENTINEL-2B");
        let t0 = tempo::ler("2026-11-20T00:00:00Z").unwrap();
        let t1 = t0 + 10.0 * 86_400.0;
        let alvo = tempo::ler("2026-11-23T13:03:00Z").unwrap();
        let nuvens: Vec<(f64, f64)> = (0..240).map(|h| (t0 + h as f64 * 3600.0, 20.0)).collect();
        let e = Entrada {
            gp: gp.as_array().unwrap(),
            lon: -42.887,
            lat: -20.695,
            t0,
            t1,
            fuso: -3.0 * 3600.0,
            nome: "Viçosa",
            nuvens: &nuvens,
            diarios: false,
            sem_cobertura: &[],
        };
        let s = calcular(&e);
        let fs = s.camada["features"].as_array().unwrap();
        assert!(!fs.is_empty());
        let perto = fs.iter().find(|f| {
            let t = tempo::ler(f["properties"]["hora_utc"].as_str().unwrap()).unwrap();
            (t - alvo).abs() < 10.0 * 60.0
        });
        let perto = perto.expect("a passagem de 23/11");
        assert_eq!(perto["properties"]["ceu"], "céu limpo");
        assert!(perto["properties"]["quando"].as_str().unwrap().starts_with("seg 23/11 10:0"), "{}", perto["properties"]["quando"]);
        assert!(s.relatorio.contains("A próxima imagem limpa provável: SENTINEL-2B"), "{}", s.relatorio);
        // A opção dos diários só existe no formulário do comando, não no painel.
        assert!(s.relatorio.contains("rode Próximas imagens na aba Satélites com Incluir os diários"), "{}", s.relatorio);

        // Onde o Sentinel-2 não grava (o mar aberto), as passagens dele ficam de fora, e o
        // relatório não promete uma imagem limpa dele.
        let sem = ["sentinel-2-l2a".to_string()];
        let mar = calcular(&Entrada { sem_cobertura: &sem, ..e });
        assert_eq!(mar.passagens, 0);
        assert!(mar.relatorio.contains("SENTINEL-2 passa por aqui, mas não grava este lugar"), "{}", mar.relatorio);
        assert!(!mar.relatorio.contains("A próxima imagem limpa provável"), "{}", mar.relatorio);
        assert!(!mar.relatorio.contains("Nenhuma passagem do Sentinel-2 ou do Landsat"), "{}", mar.relatorio);
        // Com uma órbita de teste do Landsat 9 que também passa sobre Viçosa no período: os dois
        // nomes, com "e".
        let dois = ["sentinel-2-l2a".to_string(), "landsat-c2-l2".to_string()];
        let gp2 = json!([gp[0].clone(), {"OBJECT_NAME":"LANDSAT 9","OBJECT_ID":"2021-088A","EPOCH":"2026-11-15T19:34:54.704352","MEAN_MOTION":14.57110,"ECCENTRICITY":0.0001,"INCLINATION":98.2,"RA_OF_ASC_NODE":30.0,"ARG_OF_PERICENTER":90.0,"MEAN_ANOMALY":0.0,"EPHEMERIS_TYPE":0,"CLASSIFICATION_TYPE":"U","NORAD_CAT_ID":49260,"ELEMENT_SET_NO":999,"REV_AT_EPOCH":1,"BSTAR":1e-05,"MEAN_MOTION_DOT":0,"MEAN_MOTION_DDOT":0}]);
        let mar2 = calcular(&Entrada { gp: gp2.as_array().unwrap(), sem_cobertura: &dois, ..e });
        assert!(mar2.relatorio.contains("LANDSAT 9 e SENTINEL-2 passam por aqui, mas não gravam"), "{}", mar2.relatorio);
    }
}
