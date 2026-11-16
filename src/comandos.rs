//! Os comandos e a ferramenta do plugin, que falam com o RustGIS.
//!
//! O comando `passagens` busca tudo o que vem da web (a hora, o nome do lugar, os elementos
//! orbitais e as imagens do STAC) e passa para a ferramenta `calcular`, pelo `gp.run`: só uma
//! ferramenta tem orçamento de instruções para o SGP4 de centenas de satélites.

use rustgis_plugin::{Kind, Request, ToolOutput, Value, geojson, host, json};

use crate::cache;
use crate::calculo::{self, Entrada};
use crate::imagens::{self, Imagem};
use crate::orbita::Categoria;
use crate::perfis::COLECOES;
use crate::tempo;

const PREFIXO: &str = "Passagens: ";

/// O STAC do Planetary Computer.
const STAC: &str = "https://planetarycomputer.microsoft.com/api/stac/v1";

/// Os grupos e nomes do CelesTrak. `resource` tem os de observação da Terra; `weather`, os
/// meteorológicos; os nomes trazem os que faltam nos grupos (os Sentinel-1 novos, os CBERS, o
/// Amazonia-1 e os radares comerciais).
const CELESTRAK: [&str; 7] =
    ["GROUP=resource", "GROUP=weather", "NAME=SENTINEL-1", "NAME=CBERS", "NAME=AMAZONIA", "NAME=ICEYE", "NAME=CAPELLA"];

fn handle(req: Request) -> Result<Value, String> {
    match (req.kind, req.id.as_str()) {
        (Kind::Command, "passagens") => passagens(&req),
        (Kind::Command, "imagem") => imagem(),
        (Kind::Tool, "calcular") => calcular(&req),
        (_, outro) => Err(format!("o Passagens não tem '{outro}'")),
    }
}

rustgis_plugin::export!(handle);

/// Um documento da web, ou `None` se ele ainda está chegando (no app web).
fn buscar(url: &str) -> Result<Option<Value>, String> {
    let r = host::call("host.fetch", json!({"url": url}))?;
    if r.get("pending").and_then(Value::as_bool) == Some(true) {
        return Ok(None);
    }
    Ok(Some(r))
}

/// Se a resposta de um comando espera algo que ainda está chegando.
fn esperando(r: &Value) -> bool {
    r.get("pending").and_then(Value::as_bool) == Some(true) || r.get("downloading").is_some()
}

/// O lugar: `lat` e `lon` dos parâmetros, ou o centro do mapa.
fn lugar(req: &Request) -> Result<(f64, f64), String> {
    match (req.f64("lat"), req.f64("lon")) {
        (Some(lat), Some(lon)) => {
            if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
                return Err(format!("{lat}, {lon} não é uma latitude e longitude"));
            }
            Ok((lat, lon))
        }
        (None, None) => {
            let v = host::view()?;
            let c = &v["center"];
            match (c[1].as_f64(), c[0].as_f64()) {
                (Some(lat), Some(lon)) => Ok((lat, lon)),
                _ => Err("o centro do mapa não é conhecido: dê lat e lon".into()),
            }
        }
        _ => Err("dê lat e lon juntas (ou nenhuma, para o centro do mapa)".into()),
    }
}

/// O nome do lugar (a cidade), da busca reversa do Photon (Komoot).
fn nome_do_lugar(resposta: Option<&Value>) -> Option<String> {
    let p = &resposta?["json"]["features"][0]["properties"];
    p["city"].as_str().or(p["county"].as_str()).or(p["name"].as_str()).map(str::to_string)
}

/// `passagens`: os satélites que passaram sobre o lugar nos últimos dias.
fn passagens(req: &Request) -> Result<Value, String> {
    let (lat, lon) = lugar(req)?;
    let dias = req.f64("dias").unwrap_or(7.0).round().clamp(1.0, 14.0);
    let trazer_imagem = req.bool("imagem").unwrap_or(true);
    let nome_dado = req.str("nome").map(str::trim).filter(|n| !n.is_empty());

    // Tudo da web primeiro: no app web, os pedidos que ainda não chegaram saem juntos, e o
    // comando roda de novo quando eles chegam.
    let meteo =
        buscar(&format!("https://api.open-meteo.com/v1/forecast?latitude={lat:.4}&longitude={lon:.4}&current=cloud_cover&timezone=auto"));
    let photon = match nome_dado {
        Some(_) => Ok(Some(Value::Null)),
        None => buscar(&format!("https://photon.komoot.io/reverse?lon={lon:.5}&lat={lat:.5}&radius=20&limit=1")),
    };
    if matches!(meteo, Ok(None)) || matches!(photon, Ok(None)) {
        return Ok(json!({"pending": true}));
    }
    let mut avisos: Vec<String> = Vec::new();

    // Agora, pelo Open-Meteo (o plugin não tem relógio), e o fuso do lugar. A hora dele é a do
    // começo do quarto de hora.
    let meteo = meteo?.ok_or("o Open-Meteo não respondeu")?;
    let m = &meteo["json"];
    let fuso = m["utc_offset_seconds"].as_f64().unwrap_or(0.0);
    let agora =
        m["current"]["time"].as_str().and_then(tempo::ler).map(|t| t - fuso + 15.0 * 60.0).ok_or("o Open-Meteo não deu a hora de agora")?;
    let nuvem_agora = m["current"]["cloud_cover"].as_f64();
    let nome = match nome_dado {
        Some(n) => n.to_string(),
        None => nome_do_lugar(photon.ok().flatten().as_ref()).unwrap_or_else(|| format!("{lat:.4}, {lon:.4}")),
    };
    let (t0, t1) = (agora - dias * 86_400.0, agora);

    let Some(elementos) = elementos_orbitais(agora, &mut avisos)? else {
        return Ok(json!({"pending": true}));
    };

    // As imagens do Planetary Computer, uma coleção por vez (as respostas são grandes).
    let mut imgs: Vec<Imagem> = Vec::new();
    let caixa = [lon - 0.0005, lat - 0.0005, lon + 0.0005, lat + 0.0005];
    let periodo = format!("{}/{}", tempo::dia(t0 - 3600.0), tempo::dia(t1));
    for c in COLECOES {
        let r = host::run(
            "stac.search",
            json!({"url": STAC, "collections": [c], "bbox": caixa, "datetime": periodo, "limit": 100, "footprints": false}),
        );
        match r {
            Ok(r) if esperando(&r) => return Ok(json!({"pending": true})),
            Ok(r) => imgs.extend(imagens::ler(&r).into_iter().filter(|i| i.t >= t0 - imagens::TOLERANCIA && i.t <= t1)),
            Err(e) => avisos.push(format!("STAC {c}: {e}")),
        }
    }

    // O cálculo, na ferramenta.
    let camada = format!("{PREFIXO}{nome}");
    let camada_lugar = format!("{PREFIXO}{nome} (lugar)");
    remover(&[&camada, &camada_lugar])?;
    let imgs_json: Vec<Value> =
        imgs.iter().map(|i| json!({"id": i.id, "colecao": i.colecao, "t": i.t, "nuvem": i.nuvem, "url": i.url})).collect();
    host::run(
        "gp.run",
        json!({"tool": "plugin:passagens/calcular", "params": {
            "gp": Value::Array(elementos).to_string(),
            "imagens": Value::Array(imgs_json).to_string(),
            "lat": lat, "lon": lon, "inicio": t0, "fim": t1, "fuso": fuso,
            "lugar": nome, "camada": camada, "camadaLugar": camada_lugar, "nuvemAgora": nuvem_agora,
        }}),
    )?;
    // O relatório vem no ponto do lugar, que a ferramenta faz (o gp.run não devolve as mensagens
    // dela).
    let mut relatorio = host::features(&camada_lugar, false)
        .ok()
        .and_then(|fc| fc["features"][0]["properties"]["relatorio"].as_str().map(str::to_string))
        .unwrap_or_else(|| "As passagens estão na camada.".to_string());
    if host::layers()?.as_array().into_iter().flatten().any(|l| l["name"] == camada.as_str()) {
        estilo(&camada);
    }
    let _ = host::run("map.goToXY", json!({"lon": lon, "lat": lat, "scale": 15_000_000}));

    // A imagem óptica mais recente com pouca nuvem: toda imagem do lugar no período veio de uma
    // passagem sobre ele.
    relatorio += &format!(
        "\n\nA camada {camada} tem uma linha por passagem (o traço no chão), colorida pela categoria, com a hora, a distância, o Sol e a imagem. Selecione uma passagem que fotografou e rode Imagem da passagem para vê-la."
    );
    if trazer_imagem {
        let mut opticas: Vec<&Imagem> =
            imgs.iter().filter(|i| i.colecao != "sentinel-1-grd" && i.nuvem.is_some_and(|n| n <= 30.0)).collect();
        opticas.sort_by(|a, b| b.t.total_cmp(&a.t));
        match opticas.first() {
            Some(i) => match adicionar_imagem(i, &calculo::satelite_legivel(&i.id), &tempo::local(i.t, fuso)) {
                Ok(n) => relatorio += &format!("\nA imagem mais recente com pouca nuvem já está no mapa: {n}."),
                Err(e) => avisos.push(format!("imagem {}: {e}", i.id)),
            },
            None => match ultima_vista_limpa(caixa, t1) {
                Some(i) => match adicionar_imagem(&i, &calculo::satelite_legivel(&i.id), &tempo::local(i.t, fuso)) {
                    Ok(n) => {
                        relatorio += &format!(
                            "\nNenhuma imagem óptica do período tem 30% de nuvem ou menos. A última vista limpa ({}) já está no mapa: {n}.",
                            tempo::dia(i.t)
                        )
                    }
                    Err(e) => avisos.push(format!("imagem {}: {e}", i.id)),
                },
                None => relatorio += "\nNenhuma imagem óptica do período tem 30% de nuvem ou menos, nem dos 120 dias antes.",
            },
        }
    }
    if !avisos.is_empty() {
        relatorio += &format!("\n\nAvisos: {}", avisos.join("; "));
    }
    Ok(json!({"report": relatorio}))
}

/// Os elementos orbitais: os do cache, se têm menos de [`cache::VALIDADE`]; senão os do
/// CelesTrak, que vão para o cache. Se o CelesTrak falha (ele bloqueia quem baixa demais), serve
/// um cache de até [`cache::LIMITE`], com um aviso. `None`: um download ainda está chegando.
fn elementos_orbitais(agora: f64, avisos: &mut Vec<String>) -> Result<Option<Vec<Value>>, String> {
    let guardado = host::call("host.settings", json!({})).ok().and_then(|c| cache::expandir(&c["gp"]));
    if let Some((els, quando)) = &guardado
        && agora - quando < cache::VALIDADE
        && !els.is_empty()
    {
        return Ok(Some(els.clone()));
    }
    let gp: Vec<(&str, Result<Option<Value>, String>)> =
        CELESTRAK.iter().map(|q| (*q, buscar(&format!("https://celestrak.org/NORAD/elements/gp.php?{q}&FORMAT=json")))).collect();
    if gp.iter().any(|(_, r)| matches!(r, Ok(None))) {
        return Ok(None);
    }
    // Sem repetir (o Sentinel-3 está em dois grupos) e sem os velhos (mais de 30 dias: satélite
    // reentrado ou abandonado).
    let mut elementos: Vec<Value> = Vec::new();
    let mut vistos: Vec<u64> = Vec::new();
    let mut falhas = Vec::new();
    for (q, r) in gp {
        match r {
            Ok(Some(r)) => {
                for el in r["json"].as_array().into_iter().flatten() {
                    let Some(norad) = el["NORAD_CAT_ID"].as_u64() else { continue };
                    let recente = el["EPOCH"].as_str().and_then(tempo::ler).is_some_and(|ep| agora - ep < 30.0 * 86_400.0);
                    if recente && !vistos.contains(&norad) {
                        vistos.push(norad);
                        elementos.push(el.clone());
                    }
                }
            }
            Ok(None) => {}
            Err(e) => falhas.push(format!("{q}: {e}")),
        }
    }
    if falhas.is_empty() {
        let c = cache::compactar(&elementos, agora);
        if c.to_string().len() <= cache::MAXIMO {
            let _ = host::call("host.setSettings", json!({"values": {"gp": c}}));
        }
        return Ok(Some(elementos));
    }
    // Faltou algum grupo: o cache inteiro, se há um recente o bastante, é melhor que metade.
    if let Some((els, quando)) = guardado
        && agora - quando < cache::LIMITE
        && !els.is_empty()
    {
        avisos.push(format!(
            "o CelesTrak não respondeu ({}); as órbitas são as guardadas há {:.0} h",
            falhas.join("; "),
            (agora - quando) / 3600.0
        ));
        return Ok(Some(els));
    }
    if elementos.is_empty() {
        return Err(format!(
            "o CelesTrak não deu nenhum satélite ({}). Ele bloqueia por algumas horas quem baixa os mesmos dados muitas vezes; tente mais tarde",
            falhas.join("; ")
        ));
    }
    avisos.push(format!("o CelesTrak não deu todos os grupos ({})", falhas.join("; ")));
    Ok(Some(elementos))
}

/// A imagem do Sentinel-2 mais recente com até 10% de nuvem nos 120 dias até `t1`.
fn ultima_vista_limpa(caixa: [f64; 4], t1: f64) -> Option<Imagem> {
    let periodo = format!("{}/{}", tempo::dia(t1 - 120.0 * 86_400.0), tempo::dia(t1));
    let r = host::run(
        "stac.search",
        json!({"url": STAC, "collections": ["sentinel-2-l2a"], "bbox": caixa, "datetime": periodo, "maxCloud": 10,
               "sortby": "-datetime", "limit": 1, "footprints": false}),
    )
    .ok()?;
    imagens::ler(&r).into_iter().max_by(|a, b| a.t.total_cmp(&b.t))
}

/// A ferramenta `calcular`: as passagens, do que o comando `passagens` buscou.
fn calcular(req: &Request) -> Result<Value, String> {
    let texto = |n: &str| req.str(n).ok_or_else(|| format!("falta '{n}': rode o comando Passagens de satélites"));
    let numero = |n: &str| req.f64(n).ok_or_else(|| format!("falta '{n}': rode o comando Passagens de satélites"));
    let gp: Value = serde_json::from_str(texto("gp")?).map_err(|e| format!("gp: {e}"))?;
    let imgs: Value = serde_json::from_str(texto("imagens")?).map_err(|e| format!("imagens: {e}"))?;
    let imgs: Vec<Imagem> = imgs
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| {
            Some(Imagem {
                id: i["id"].as_str()?.into(),
                colecao: i["colecao"].as_str().unwrap_or_default().into(),
                t: i["t"].as_f64()?,
                nuvem: i["nuvem"].as_f64(),
                url: i["url"].as_str().unwrap_or_default().into(),
            })
        })
        .collect();
    let lugar = texto("lugar")?;
    let saida = calculo::calcular(&Entrada {
        gp: gp.as_array().map(Vec::as_slice).unwrap_or_default(),
        imagens: &imgs,
        lon: numero("lon")?,
        lat: numero("lat")?,
        t0: numero("inicio")?,
        t1: numero("fim")?,
        fuso: req.f64("fuso").unwrap_or(0.0),
        nome: lugar,
        nuvem_agora: req.f64("nuvemAgora"),
    });
    let (lat, lon) = (numero("lat")?, numero("lon")?);
    let mut out = ToolOutput::new().layer(
        texto("camadaLugar")?,
        geojson::collection(vec![geojson::feature(
            geojson::point(lon, lat),
            json!({"nome": lugar, "lat": lat, "lon": lon, "relatorio": saida.relatorio}),
        )]),
    );
    if saida.passagens > 0 {
        out = out.layer(texto("camada")?, saida.camada);
    }
    Ok(out.into())
}

/// Remove as camadas `nomes` que estiverem no mapa (de uma rodada anterior).
fn remover(nomes: &[&str]) -> Result<(), String> {
    let camadas = host::layers()?;
    let ids: Vec<Value> = camadas
        .as_array()
        .into_iter()
        .flatten()
        .filter(|l| l["name"].as_str().is_some_and(|n| nomes.contains(&n)))
        .map(|l| l["id"].clone())
        .collect();
    if !ids.is_empty() {
        host::run("layer.remove", json!({"layer": ids}))?;
    }
    Ok(())
}

/// As cores por categoria.
fn estilo(camada: &str) {
    if host::run("layer.symbology", json!({"layer": camada, "type": "unique", "field": "categoria"})).is_err() {
        return;
    }
    for c in Categoria::TODAS {
        let _ = host::run("layer.classStyle", json!({"layer": camada, "value": c.texto(), "color": c.cor()}));
        let _ = host::run("layer.lineStrokes", json!({"layer": camada, "value": c.texto(), "strokes": [{"width": c.largura()}]}));
    }
}

/// Põe a imagem do item no mapa; o nome da camada.
fn adicionar_imagem(i: &Imagem, satelite: &str, quando: &str) -> Result<String, String> {
    let nuvem = i.nuvem.map(|n| format!(", {n:.0}% de nuvem")).unwrap_or_default();
    let nome = format!("{PREFIXO}{satelite} {quando}{nuvem}");
    let mut p = json!({"url": i.url, "name": nome});
    match i.colecao.as_str() {
        "sentinel-2-l2a" => p["asset"] = json!("visual"),
        "sentinel-1-grd" => p["asset"] = json!("vv"),
        _ => {}
    }
    let r = host::run("stac.addAsset", p)?;
    if esperando(&r) {
        return Ok(format!("{nome} (chegando)"));
    }
    Ok(nome)
}

/// `imagem`: a imagem da passagem selecionada.
fn imagem() -> Result<Value, String> {
    let camadas = host::layers()?;
    let mut ordem: Vec<&Value> = camadas
        .as_array()
        .into_iter()
        .flatten()
        .filter(|l| l["name"].as_str().is_some_and(|n| n.starts_with(PREFIXO) && !n.ends_with("(lugar)")) && l["raster"] != true)
        .collect();
    // A ativa primeiro.
    ordem.sort_by_key(|l| l["active"] != true);
    for l in ordem {
        let fc = host::features(l["name"].as_str().unwrap_or_default(), true)?;
        for f in fc["features"].as_array().into_iter().flatten() {
            let p = &f["properties"];
            let Some(url) = p["url"].as_str().filter(|u| !u.is_empty()) else { continue };
            let img = Imagem {
                id: p["imagem"].as_str().unwrap_or_default().into(),
                colecao: p["colecao"].as_str().unwrap_or_default().into(),
                t: 0.0,
                nuvem: p["nuvem"].as_f64(),
                url: url.into(),
            };
            let nome = adicionar_imagem(&img, p["satelite"].as_str().unwrap_or_default(), p["hora_local"].as_str().unwrap_or_default())?;
            return Ok(json!({"report": format!("No mapa: {nome}.")}));
        }
    }
    Err("selecione uma passagem que fotografou (categoria \"fotografou\") numa camada do Passagens".into())
}
