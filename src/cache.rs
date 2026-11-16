//! Os elementos orbitais guardados nas configurações do plugin (`host.setSettings`, até 64 KB),
//! para não baixar os mesmos dados do CelesTrak a cada lugar: ele bloqueia quem baixa o mesmo
//! grupo mais de uma vez a cada duas horas. Cada satélite vira uma linha de números.

use serde_json::{Value, json};

use crate::tempo;

/// A versão do formato.
const VERSAO: u64 = 1;

/// Por quanto tempo o cache vale sem perguntar ao CelesTrak (que atualiza os elementos ao longo
/// do dia).
pub const VALIDADE: f64 = 6.0 * 3600.0;

/// Até quando um cache velho ainda serve, quando o CelesTrak não responde.
pub const LIMITE: f64 = 5.0 * 86_400.0;

/// O maior cache que o plugin grava (as configurações levam 64 KB, com as chaves).
pub const MAXIMO: usize = 63_000;

/// Os campos numéricos do OMM, na ordem da linha (depois do nome e da época).
const CAMPOS: [&str; 10] = [
    "NORAD_CAT_ID",
    "MEAN_MOTION",
    "ECCENTRICITY",
    "INCLINATION",
    "RA_OF_ASC_NODE",
    "ARG_OF_PERICENTER",
    "MEAN_ANOMALY",
    "BSTAR",
    "MEAN_MOTION_DOT",
    "MEAN_MOTION_DDOT",
];

/// O cache de `elementos` (OMM do CelesTrak) baixados em `quando`.
pub fn compactar(elementos: &[Value], quando: f64) -> Value {
    let linhas: Vec<Value> = elementos
        .iter()
        .filter_map(|e| {
            let epoca = tempo::ler(e["EPOCH"].as_str()?)?;
            let mut l = vec![json!(e["OBJECT_NAME"].as_str()?), json!((epoca * 1000.0).round() / 1000.0)];
            for c in CAMPOS {
                l.push(e[c].clone());
            }
            Some(Value::Array(l))
        })
        .collect();
    json!({"v": VERSAO, "t": quando.round(), "s": linhas})
}

/// Os elementos (OMM) e a hora do cache, se ele é deste formato.
pub fn expandir(cache: &Value) -> Option<(Vec<Value>, f64)> {
    if cache["v"].as_u64() != Some(VERSAO) {
        return None;
    }
    let quando = cache["t"].as_f64()?;
    let elementos = cache["s"]
        .as_array()?
        .iter()
        .filter_map(|l| {
            let l = l.as_array()?;
            let mut e = json!({
                "OBJECT_NAME": l.first()?.as_str()?,
                "EPOCH": tempo::iso_ms(l.get(1)?.as_f64()?),
                "EPHEMERIS_TYPE": 0,
                "CLASSIFICATION_TYPE": "U",
                "ELEMENT_SET_NO": 999,
                "REV_AT_EPOCH": 0,
            });
            for (i, c) in CAMPOS.iter().enumerate() {
                e[*c] = l.get(i + 2)?.clone();
            }
            Some(e)
        })
        .collect();
    Some((elementos, quando))
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::orbita::Satelite;

    #[test]
    fn ida_e_volta() {
        let s2b = json!({"OBJECT_NAME":"SENTINEL-2B","OBJECT_ID":"2017-013A","EPOCH":"2026-11-15T19:34:54.704352","MEAN_MOTION":14.30813686,"ECCENTRICITY":0.0001114,"INCLINATION":98.5709,"RA_OF_ASC_NODE":32.4267,"ARG_OF_PERICENTER":90.3469,"MEAN_ANOMALY":269.7841,"EPHEMERIS_TYPE":0,"CLASSIFICATION_TYPE":"U","NORAD_CAT_ID":42063,"ELEMENT_SET_NO":999,"REV_AT_EPOCH":50632,"BSTAR":9.0686e-05,"MEAN_MOTION_DOT":1.94e-06,"MEAN_MOTION_DDOT":0});
        let c = compactar(std::slice::from_ref(&s2b), 1_794_800_000.0);
        let (els, quando) = expandir(&c).unwrap();
        assert_eq!(quando, 1_794_800_000.0);
        let (a, b) = (Satelite::de_gp(&s2b).unwrap(), Satelite::de_gp(&els[0]).unwrap());
        assert_eq!((a.nome.as_str(), a.norad), (b.nome.as_str(), b.norad));
        let t = a.epoca + 3.0 * 86_400.0;
        assert_eq!(a.estado(t).unwrap().0, b.estado(t).unwrap().0);
        assert!(expandir(&json!({"v": 99, "t": 0, "s": []})).is_none());
    }

    /// Os ~330 satélites cabem nos 64 KB das configurações.
    #[test]
    fn cabe_nas_configuracoes() {
        let el = json!({"OBJECT_NAME":"COSMO-SKYMED SECOND GEN 2 (CSG-2)","EPOCH":"2026-11-15T19:34:54.704352","MEAN_MOTION":14.30813686,"ECCENTRICITY":0.0001114,"INCLINATION":98.5709,"RA_OF_ASC_NODE":32.4267,"ARG_OF_PERICENTER":90.3469,"MEAN_ANOMALY":269.7841,"NORAD_CAT_ID":42063,"BSTAR":9.0686e-05,"MEAN_MOTION_DOT":1.94e-06,"MEAN_MOTION_DDOT":0});
        let todos = vec![el; 330];
        let tamanho = compactar(&todos, 0.0).to_string().len();
        assert!(tamanho < MAXIMO - 8_000, "{tamanho}");
    }
}
