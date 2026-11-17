//! As imagens do STAC e a passagem que fez cada uma.

use serde_json::Value;

use crate::orbita::{Categoria, Passagem};
use crate::perfis::satelite_do_item;
use crate::tempo;

/// Uma imagem de um item do STAC.
#[derive(Clone, Debug, PartialEq)]
pub struct Imagem {
    pub id: String,
    pub colecao: String,
    pub t: f64,
    pub nuvem: Option<f64>,
    /// O endereço do item (`links` com `rel: self`).
    pub url: String,
}

/// As imagens da resposta do `stac.search` (`{"items": [...]}`).
pub fn ler(resposta: &Value) -> Vec<Imagem> {
    resposta["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let id = item["id"].as_str()?.to_string();
            let t = tempo::ler(item["properties"]["datetime"].as_str()?)?;
            let url = item["links"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|l| l["rel"] == "self")
                .and_then(|l| l["href"].as_str())
                .unwrap_or_default()
                .to_string();
            Some(Imagem {
                id,
                colecao: item["collection"].as_str().unwrap_or_default().to_string(),
                t,
                nuvem: item["properties"]["eo:cloud_cover"].as_f64(),
                url,
            })
        })
        .collect()
}

/// O item da mesma cena do Sentinel-1 na coleção `sentinel-1-rtc` do Planetary Computer, pelo
/// endereço do item do `sentinel-1-grd`. O GeoTIFF do GRD não é georreferenciado (traz só pontos
/// de controle) e o RustGIS não o abre; o RTC é a mesma cena corrigida do terreno, num COG em UTM,
/// com o id do GRD mais `_rtc`.
pub fn endereco_rtc(url: &str) -> Option<String> {
    let (antes, id) = url.split_once("/collections/sentinel-1-grd/items/")?;
    let id = id.trim_end_matches('/');
    (!id.is_empty() && !id.contains('/')).then(|| format!("{antes}/collections/sentinel-1-rtc/items/{id}_rtc"))
}

/// A maior diferença entre a passagem e a hora do item: o `datetime` de um item do Sentinel-2 é
/// o começo da tomada, que pode ter começado minutos antes; as passagens do mesmo satélite sobre
/// o lugar ficam a mais de uma órbita (~100 min) umas das outras.
pub const TOLERANCIA: f64 = 40.0 * 60.0;

/// Liga cada imagem à passagem do mesmo satélite mais próxima no tempo, dentro da
/// [`TOLERANCIA`], e marca essas passagens como [`Categoria::Fotografou`]. Devolve, para cada
/// passagem, as imagens dela (as de menos nuvem primeiro). As imagens sem passagem ficam de fora.
pub fn casar(passagens: &mut [Passagem], imagens: &[Imagem]) -> Vec<Vec<Imagem>> {
    let mut por_passagem = vec![Vec::new(); passagens.len()];
    for img in imagens {
        let sat = satelite_do_item(&img.id);
        let melhor = passagens
            .iter()
            .enumerate()
            .filter(|(_, p)| p.stac == Some(sat))
            .map(|(i, p)| (i, (p.t - img.t).abs()))
            .filter(|(_, dt)| *dt <= TOLERANCIA)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = melhor {
            por_passagem[i].push(img.clone());
        }
    }
    for (p, imgs) in passagens.iter_mut().zip(&mut por_passagem) {
        if !imgs.is_empty() {
            p.categoria = Categoria::Fotografou;
            // Repetidas (o mesmo item em duas buscas) saem; a de menos nuvem fica primeiro.
            imgs.sort_by(|a, b| a.nuvem.unwrap_or(0.0).total_cmp(&b.nuvem.unwrap_or(0.0)).then(a.id.cmp(&b.id)));
            imgs.dedup_by(|a, b| a.id == b.id);
        }
    }
    por_passagem
}

/// Do id de um item do Sentinel-2 no Planetary Computer
/// (`S2C_MSIL2A_20261009T130251_R095_T23KQT_20261009T154802`): o satélite (`S2C`), o dia
/// (`2026-10-09`) e a grade MGRS (`23KQT`).
pub fn partes_s2(id: &str) -> Option<(&str, String, &str)> {
    let p: Vec<&str> = id.split('_').collect();
    let sat = *p.first()?;
    let data = p.get(2)?.get(..8)?;
    if !data.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let dia = format!("{}-{}-{}", &data[..4], &data[4..6], &data[6..8]);
    let grade = p.iter().skip(3).find(|s| s.len() == 6 && s.starts_with('T'))?.get(1..)?;
    Some((sat, dia, grade))
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    fn passagem(stac: &'static str, quando: &str) -> Passagem {
        Passagem {
            satelite: stac.into(),
            norad: 1,
            t: tempo::ler(quando).unwrap(),
            distancia: 50.0,
            elevacao: 80.0,
            sol: 50.0,
            altitude: 786.0,
            direita: true,
            categoria: Categoria::Provavel,
            stac: Some(stac),
        }
    }

    #[test]
    fn le_os_itens_e_casa() {
        let resposta = json!({"items": [
            {"id": "S2B_MSIL2A_20261113T130249_R095_T23KQT_20261113T152136", "collection": "sentinel-2-l2a",
             "links": [{"rel": "self", "href": "https://pc/items/S2B"}],
             "properties": {"datetime": "2026-11-13T13:02:49.024000Z", "eo:cloud_cover": 93.3}},
            {"id": "S2A_MSIL2A_20261110T130251_R095_T23KQT_20261110T192911", "collection": "sentinel-2-l2a",
             "links": [], "properties": {"datetime": "2026-11-10T13:02:51Z", "eo:cloud_cover": 99.9}},
            {"id": "sem data", "properties": {}}
        ]});
        let imgs = ler(&resposta);
        assert_eq!(imgs.len(), 2);
        assert_eq!(imgs[0].url, "https://pc/items/S2B");
        let mut ps =
            vec![passagem("S2B", "2026-11-13T13:06:00Z"), passagem("S2B", "2026-11-13T14:47:00Z"), passagem("S2C", "2026-11-10T13:05:00Z")];
        let casadas = casar(&mut ps, &imgs);
        assert_eq!(casadas[0].len(), 1);
        assert_eq!(ps[0].categoria, Categoria::Fotografou);
        assert_eq!(ps[1].categoria, Categoria::Provavel);
        // A imagem do S2A não é do S2C.
        assert_eq!(ps[2].categoria, Categoria::Provavel);
    }

    #[test]
    fn rtc_da_cena_do_sentinel_1() {
        let grd = "https://planetarycomputer.microsoft.com/api/stac/v1/collections/sentinel-1-grd/items/S1D_IW_GRDH_1SDV_20261113T081637_20261113T081702_005446_00A666";
        assert_eq!(
            endereco_rtc(grd).as_deref(),
            Some(
                "https://planetarycomputer.microsoft.com/api/stac/v1/collections/sentinel-1-rtc/items/S1D_IW_GRDH_1SDV_20261113T081637_20261113T081702_005446_00A666_rtc"
            )
        );
        assert_eq!(endereco_rtc("https://planetarycomputer.microsoft.com/api/stac/v1/collections/sentinel-2-l2a/items/S2B_X"), None);
        assert_eq!(endereco_rtc("https://x/collections/sentinel-1-grd/items/"), None);
    }

    #[test]
    fn partes_do_id_do_sentinel_2() {
        assert_eq!(partes_s2("S2C_MSIL2A_20261009T130251_R095_T23KQT_20261009T154802"), Some(("S2C", "2026-10-09".to_string(), "23KQT")));
        assert_eq!(partes_s2("LC09_L2SP_217074_20261110_02_T1"), None);
        assert_eq!(partes_s2("S2B"), None);
    }
}
