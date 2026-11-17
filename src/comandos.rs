//! Os comandos e a ferramenta do plugin, que falam com o RustGIS.
//!
//! O comando `passagens` busca tudo o que vem da web (a hora, o nome do lugar, os elementos
//! orbitais e as imagens do STAC) e passa para a ferramenta `calcular`, pelo `gp.run`: só uma
//! ferramenta tem orçamento de instruções para o SGP4 de centenas de satélites.

use rustgis_plugin::{Kind, Request, ToolOutput, Value, geojson, host, json};

use crate::cache;
use crate::calculo::{self, Entrada};
use crate::galeria;
use crate::gratuitos;
use crate::imagens::{self, Imagem};
use crate::municipios;
use crate::nomes;
use crate::orbita::Categoria;
use crate::perfis::COLECOES;
use crate::proximas::{self, Ceu};
use crate::tempo;

use crate::nomes::{PREFIXO, PREFIXO_IMAGENS, PREFIXO_PROXIMAS};

/// O STAC do Planetary Computer.
const STAC: &str = "https://planetarycomputer.microsoft.com/api/stac/v1";

/// Os grupos e nomes do CelesTrak. `resource` tem os de observação da Terra; `weather`, os
/// meteorológicos; os nomes trazem os que faltam nos grupos (os Sentinel-1 novos, os CBERS, o
/// Amazonia-1 e os radares comerciais).
const CELESTRAK: [&str; 7] =
    ["GROUP=resource", "GROUP=weather", "NAME=SENTINEL-1", "NAME=CBERS", "NAME=AMAZONIA", "NAME=ICEYE", "NAME=CAPELLA"];

/// As mesmas órbitas, compactadas (o formato do [`cache`]) por um workflow do repositório do
/// plugin, que baixa do CelesTrak uma vez por dia. Assim ninguém é bloqueado pelo CelesTrak, e o
/// GitHub deixa o app web ler o arquivo (o CelesTrak não manda CORS).
const ORBITAS: &str = "https://raw.githubusercontent.com/elpidioassinaturas/rustgis-passagens/orbitas/orbitas.json";

/// Quantas datas o comando `imagens` confere no pixel do lugar. Uma chamada de plugin faz no
/// máximo 64 consultas web (`host.fetch`), com 60 s para todas, e o comando já faz até 2 (o
/// Open-Meteo e o Photon). Uma consulta do pixel nova leva uns 1,4 s (medido no app desktop):
/// numa rodada cabem umas 40, e as que passam dos 60 s falham ([`FALHAS`]). As respostas ficam
/// guardadas 10 minutos e voltam sem custo, e por isso a rodada seguinte confere até 48.
const CONFERIR: usize = 48;

/// Depois de tantas consultas do pixel falhando em seguida (em geral, o tempo das consultas da
/// chamada acabou), o comando para de consultar e o relatório diz para rodar de novo.
const FALHAS: usize = 3;

fn handle(req: Request) -> Result<Value, String> {
    match (req.kind, req.id.as_str()) {
        (Kind::Command, "passagens") => passagens(&req),
        (Kind::Command, "proximas") => proximas(&req),
        (Kind::Command, "imagem") => imagem(),
        (Kind::Command, "imagens") => imagens_do_lugar(&req),
        // O painel: o clique no mapa e os botões, que rodam sem formulário.
        (Kind::Command, "marcar") => marcar(&req),
        (Kind::Command, "estado") => estado(),
        (Kind::Command, "ver-passagens") => no_painel(passagens(&req)),
        (Kind::Command, "ver-imagens") => no_painel(imagens_do_lugar(&req)),
        (Kind::Command, "ver-proximas") => no_painel(proximas(&req)),
        (Kind::Tool, "calcular") => calcular(&req),
        (Kind::Tool, "proximas") => calcular_proximas(&req),
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

/// Um lugar escolhido: o município, ou um ponto clicado no mapa.
struct Lugar {
    lat: f64,
    lon: f64,
    /// O nome das camadas (o município), quando se sabe.
    nome: Option<String>,
    /// O código do IBGE, quando é um município.
    codigo: Option<String>,
}

impl Lugar {
    fn de_municipio(m: &municipios::Municipio) -> Lugar {
        Lugar { lat: m.lat, lon: m.lon, nome: Some(m.nome.to_string()), codigo: Some(m.codigo.to_string()) }
    }

    /// O texto do painel.
    fn texto(&self) -> String {
        let municipio = self.codigo.as_deref().and_then(municipios::por_codigo).map(|m| format!("{}\n", m.rotulo()));
        let nome = municipio.or_else(|| self.nome.as_ref().map(|n| format!("{n}\n"))).unwrap_or_default();
        format!("{nome}{:.5}, {:.5}", self.lat, self.lon)
    }
}

/// O lugar guardado nas configurações do plugin (o último escolhido no painel).
fn lugar_guardado() -> Option<Lugar> {
    let s = host::call("host.settings", json!({})).ok()?;
    let l = &s["lugar"];
    Some(Lugar {
        lat: l["lat"].as_f64()?,
        lon: l["lon"].as_f64()?,
        nome: l["nome"].as_str().map(str::to_string),
        codigo: l["codigo"].as_str().map(str::to_string),
    })
}

fn guardar_lugar(l: &Lugar) {
    let _ = host::call("host.setSettings", json!({"values": {"lugar": {"lat": l.lat, "lon": l.lon, "nome": l.nome, "codigo": l.codigo}}}));
}

/// O lugar, nesta ordem: `lat` e `lon` dos parâmetros; o `municipio` dos parâmetros (o nome, como
/// "Ubaporanga" ou "Viçosa/MG", ou o código do IBGE); o último escolhido no painel; o centro do
/// mapa.
///
/// Só o painel guarda o lugar ([`marcar`]): um formulário que desse um município e o guardasse
/// mudaria o lugar dos botões sem mudar o que o painel mostra (o painel diria Viçosa e os botões
/// rodariam para Ubaporanga).
fn lugar(req: &Request) -> Result<Lugar, String> {
    match (req.f64("lat"), req.f64("lon")) {
        (Some(lat), Some(lon)) => {
            if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
                return Err(format!("{lat}, {lon} não é uma latitude e longitude"));
            }
            return Ok(Lugar { lat, lon, nome: None, codigo: None });
        }
        (None, None) => {}
        _ => return Err("dê lat e lon juntas (ou nenhuma)".into()),
    }
    if let Some(texto) = req.str("municipio").map(str::trim).filter(|t| !t.is_empty()) {
        let m = municipios::buscar(texto)?;
        return Ok(Lugar::de_municipio(&m));
    }
    if let Some(l) = lugar_guardado() {
        return Ok(l);
    }
    let v = host::view()?;
    let c = &v["center"];
    match (c[1].as_f64(), c[0].as_f64()) {
        (Some(lat), Some(lon)) => Ok(Lugar { lat, lon, nome: None, codigo: None }),
        _ => Err("escolha um município ou um lugar no mapa".into()),
    }
}

/// O estado do painel quando ele abre: a lista dos municípios e o lugar escolhido.
fn estado() -> Result<Value, String> {
    let guardado = lugar_guardado();
    let mut r = json!({"panel": {"items": {"municipio": {"nodes": municipios::arvore()}}}});
    if let Some(l) = &guardado {
        r["panel"]["items"]["onde"] = json!({"text": l.texto()});
        if let Some(c) = &l.codigo {
            r["panel"]["values"] = json!({"municipio": c});
        }
    }
    Ok(r)
}

/// `marcar`: o município escolhido na lista do painel, ou o lugar clicado no mapa (com o nome que
/// o Photon dá a ele). Fica guardado para os botões e os formulários.
fn marcar(req: &Request) -> Result<Value, String> {
    let escolhido = req.panel_str("municipio").and_then(municipios::por_codigo);
    let clicado = req.panel.get("lugar").and_then(|p| Some((p["lat"].as_f64()?, p["lon"].as_f64()?)));
    let l = match (escolhido, clicado) {
        (Some(m), _) if req.changed("municipio") || clicado.is_none() => Lugar::de_municipio(&m),
        (_, Some((lat, lon))) => {
            let photon = match buscar(&format!("https://photon.komoot.io/reverse?lon={lon:.5}&lat={lat:.5}&radius=20&limit=1")) {
                Ok(None) => return Ok(json!({"pending": true})),
                Ok(Some(r)) => Some(r),
                Err(_) => None,
            };
            Lugar { lat, lon, nome: nome_do_lugar(photon.as_ref()), codigo: None }
        }
        _ => {
            return Ok(
                json!({"panel": {"items": {"onde": {"text": "Nenhum lugar escolhido: busque o município na lista, ou clique em Ou clicar no mapa e depois no mapa."}}}}),
            );
        }
    };
    guardar_lugar(&l);
    let mut r = json!({"panel": {"items": {
        "onde": {"text": l.texto()},
        "resultado": {"text": "Escolha o que ver: os botões rodam para este lugar."},
    }}});
    // O clique no mapa desfaz a escolha da lista.
    if l.codigo.is_none() {
        r["panel"]["values"] = json!({"municipio": null});
    }
    Ok(r)
}

/// A resposta de um comando com o relatório também no painel (que mostra até 4.000 caracteres).
///
/// Um erro também vai para o painel, e por isso o comando responde como se desse certo: o RustGIS
/// não muda o painel quando um comando falha, e o painel ficaria com o resultado anterior. Para
/// o erro não passar por sucesso, ele vai também para a barra de status, como erro.
fn no_painel(r: Result<Value, String>) -> Result<Value, String> {
    let mut r = match r {
        Ok(r) => r,
        Err(e) => {
            // O RustGIS já põe o nome do plugin na frente ("Passagens de satélites: …").
            let _ = host::call("host.notify", json!({"text": e, "error": true}));
            return Ok(json!({"report": format!("Erro: {e}"), "panel": {"items": {"resultado": {"text": format!("Erro: {e}")}}}}));
        }
    };
    if let Some(texto) = r.get("report").and_then(Value::as_str) {
        let mut curto: String = texto.chars().take(3_900).collect();
        if curto.len() < texto.len() {
            curto += "\n…";
        }
        r["panel"] = json!({"items": {"resultado": {"text": curto}}});
    }
    Ok(r)
}

/// O nome do lugar (a cidade), da busca reversa do Photon (Komoot).
fn nome_do_lugar(resposta: Option<&Value>) -> Option<String> {
    let p = &resposta?["json"]["features"][0]["properties"];
    p["city"].as_str().or(p["county"].as_str()).or(p["name"].as_str()).map(str::to_string)
}

/// `passagens`: os satélites que passaram sobre o lugar nos últimos dias.
fn passagens(req: &Request) -> Result<Value, String> {
    let local = lugar(req)?;
    let (lat, lon) = (local.lat, local.lon);
    let dias = req.f64("dias").unwrap_or(7.0).round().clamp(1.0, 14.0);
    let trazer_imagem = req.bool("imagem").unwrap_or(true);
    let nome_dado = req.str("nome").map(str::trim).filter(|n| !n.is_empty()).or(local.nome.as_deref());

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

    let Some(orbitas) = elementos_orbitais(agora, &mut avisos)? else {
        return Ok(json!({"pending": true}));
    };
    // Só os satélites com imagens que qualquer um baixa.
    let elementos: Vec<Value> =
        orbitas.elementos.iter().filter(|e| e["OBJECT_NAME"].as_str().and_then(gratuitos::de).is_some()).cloned().collect();

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

    // A imagem óptica mais recente com pouca nuvem: toda imagem do lugar no período veio de uma
    // passagem sobre ele. A nuvem aqui é a da cena inteira (o que o STAC diz); Imagens limpas
    // confere o pixel do lugar, e uma cena nublada pode ter o lugar limpo.
    //
    // Ela vem antes do cálculo: clicado, o comando não espera a imagem, e roda de novo quando ela
    // chega. Com o `gp.run` antes do `stac.addAsset`, o RustGIS recusa a imagem nessa segunda
    // rodada ("the live source or account changed since this request started"). Assim, a primeira
    // rodada só pede a imagem e para, e a segunda pega a imagem e calcula.
    const PIXEL: &str = "o lugar pode estar limpo mesmo assim: Imagens limpas confere o pixel do lugar";
    let mut linha_imagem = String::new();
    if trazer_imagem {
        let mut opticas: Vec<&Imagem> =
            imgs.iter().filter(|i| i.colecao != "sentinel-1-grd" && i.nuvem.is_some_and(|n| n <= 30.0)).collect();
        opticas.sort_by(|a, b| b.t.total_cmp(&a.t));
        let (escolhida, recente) = match opticas.first() {
            Some(i) => (Some((*i).clone()), true),
            None => (ultima_vista_limpa(caixa, t1), false),
        };
        match escolhida {
            Some(i) => match adicionar_imagem(&i, &nome, &calculo::satelite_legivel(&i.id), &tempo::local(i.t, fuso), &json!(caixa)) {
                Ok(None) => return Ok(json!({"pending": true})),
                Ok(Some(n)) if recente => {
                    linha_imagem = format!("\nA imagem mais recente com pouca nuvem na cena inteira já está no mapa: {n}.")
                }
                Ok(Some(n)) => {
                    linha_imagem = format!(
                        "\nNenhuma imagem óptica do período tem 30% de nuvem ou menos na cena inteira ({PIXEL}). A última cena limpa ({}) já está no mapa: {n}.",
                        tempo::dia(i.t)
                    )
                }
                Err(e) => avisos.push(format!("imagem {}: {e}", i.id)),
            },
            None => {
                linha_imagem = format!(
                    "\nNenhuma imagem óptica do período tem 30% de nuvem ou menos na cena inteira, nem dos 120 dias antes ({PIXEL})."
                )
            }
        }
    }

    // O cálculo, na ferramenta.
    let camada = format!("{PREFIXO}{nome}");
    let camada_lugar = format!("{PREFIXO}{nome} (lugar)");
    remover(&[&camada, &camada_lugar])?;
    let imgs_json: Vec<Value> =
        imgs.iter().map(|i| json!({"id": i.id, "colecao": i.colecao, "t": i.t, "nuvem": i.nuvem, "url": i.url})).collect();
    rodar_ferramenta(
        lon,
        lat,
        json!({"tool": "plugin:passagens/calcular", "params": {
            "gp": Value::Array(elementos).to_string(),
            "imagens": Value::Array(imgs_json).to_string(),
            "lat": lat, "lon": lon, "inicio": t0, "fim": t1, "fuso": fuso,
            "lugar": nome, "camada": camada, "camadaLugar": camada_lugar, "nuvemAgora": nuvem_agora,
            "orbitas": orbitas.fonte,
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
    // passagem sobre ele. A nuvem aqui é a da cena inteira (o que o STAC diz); Imagens limpas
    // confere o pixel do lugar, e uma cena nublada pode ter o lugar limpo.
    relatorio += &format!(
        "\n\nA camada {camada} tem uma linha por passagem (o traço no chão), colorida pela categoria, com a hora, a distância, o Sol e a imagem. Selecione uma passagem que fotografou e rode Imagem da passagem para vê-la."
    );
    relatorio += &linha_imagem;
    if !avisos.is_empty() {
        relatorio += &format!("\n\nAvisos: {}", avisos.join("; "));
    }
    orbitas.guardar();
    Ok(json!({"report": relatorio}))
}

/// Os elementos orbitais, nesta ordem: os do cache, se têm menos de [`cache::VALIDADE`]; os do
/// arquivo do GitHub, se ele tem menos de [`cache::FRESCO`]; os do CelesTrak. Os que vieram de
/// fora vão para o cache. Se nada responde, servem as órbitas guardadas mais novas, de até
/// [`cache::LIMITE`], com um aviso. `None`: um download ainda está chegando. Com os elementos vem
/// de onde eles vieram, para o relatório ("do CelesTrak", "do arquivo diário do plugin"...).
///
/// O cache novo não é gravado aqui: quem chama grava com [`Orbitas::guardar`] no fim, depois das
/// chamadas que baixam. Um comando clicado que grava as configurações e depois espera um download
/// é recusado quando roda de novo ("the live source or account changed since this request
/// started").
fn elementos_orbitais(agora: f64, avisos: &mut Vec<String>) -> Result<Option<Orbitas>, String> {
    let guardado = host::call("host.settings", json!({})).ok().and_then(|c| cache::expandir(&c["gp"]));
    if let Some((els, quando)) = &guardado
        && agora - quando < cache::VALIDADE
        && !els.is_empty()
    {
        return Ok(Some(Orbitas::new(els.clone(), format!("guardadas no plugin, de {}", tempo::idade(agora - quando)), None)));
    }
    // O mesmo arquivo de antes (o do GitHub muda uma vez por dia) não é gravado de novo.
    let quando_guardado = guardado.as_ref().map(|g| g.1);
    let novo = |c: &Value, quando: f64| (quando_guardado != Some(quando)).then(|| c.clone());
    // O arquivo do GitHub.
    let mut falhas = Vec::new();
    let mut reservas: Vec<(Vec<Value>, f64)> = Vec::new();
    match buscar(ORBITAS) {
        Ok(None) => return Ok(None),
        Ok(Some(r)) => match cache::expandir(&r["json"]) {
            Some((els, quando)) if !els.is_empty() => {
                if agora - quando < cache::FRESCO {
                    return Ok(Some(Orbitas::new(
                        els,
                        format!("do arquivo diário do plugin (copiadas do CelesTrak), de {}", tempo::idade(agora - quando)),
                        novo(&r["json"], quando),
                    )));
                }
                falhas.push(format!("o arquivo de órbitas do GitHub tem {:.0} dias", (agora - quando) / 86_400.0));
                reservas.push((els, quando));
            }
            _ => falhas.push("o arquivo de órbitas do GitHub não é deste formato".to_string()),
        },
        Err(e) => falhas.push(format!("GitHub: {e}")),
    }
    if let Some(g) = guardado {
        reservas.push(g);
    }
    // O CelesTrak.
    let gp: Vec<(&str, Result<Option<Value>, String>)> =
        CELESTRAK.iter().map(|q| (*q, buscar(&format!("https://celestrak.org/NORAD/elements/gp.php?{q}&FORMAT=json")))).collect();
    if gp.iter().any(|(_, r)| matches!(r, Ok(None))) {
        return Ok(None);
    }
    // Sem repetir (o Sentinel-3 está em dois grupos) e sem os velhos (mais de 30 dias: satélite
    // reentrado ou abandonado).
    let mut elementos: Vec<Value> = Vec::new();
    let mut vistos: Vec<u64> = Vec::new();
    let falhas_antes = falhas.len();
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
            Err(e) => falhas.push(format!("CelesTrak {q}: {e}")),
        }
    }
    if falhas.len() == falhas_antes && !elementos.is_empty() {
        let c = cache::compactar(&elementos, agora);
        return Ok(Some(Orbitas::new(elementos, "do CelesTrak, baixadas agora".to_string(), Some(c))));
    }
    // Faltou algo: as órbitas inteiras mais novas que houver são melhores que metade delas.
    reservas.sort_by(|a, b| b.1.total_cmp(&a.1));
    if let Some((els, quando)) = reservas.into_iter().find(|(els, _)| !els.is_empty())
        && agora - quando < cache::LIMITE
    {
        avisos.push(format!(
            "as órbitas são de {:.1} dias atrás, porque as fontes novas não responderam ({})",
            (agora - quando) / 86_400.0,
            falhas.join("; ")
        ));
        return Ok(Some(Orbitas::new(
            els,
            format!("guardadas, de {} (as fontes novas não responderam)", tempo::idade(agora - quando)),
            None,
        )));
    }
    if elementos.is_empty() {
        return Err(format!(
            "nenhuma fonte de órbitas respondeu ({}). O CelesTrak bloqueia por algumas horas quem baixa os mesmos dados muitas vezes; tente mais tarde",
            falhas.join("; ")
        ));
    }
    avisos.push(format!("o CelesTrak não deu todos os grupos ({})", falhas.join("; ")));
    Ok(Some(Orbitas::new(elementos, "do CelesTrak, baixadas agora (faltaram grupos)".to_string(), None)))
}

/// Os elementos orbitais, de onde vieram, e o cache a gravar (quando vieram de fora).
struct Orbitas {
    elementos: Vec<Value>,
    fonte: String,
    cache_novo: Option<Value>,
}

impl Orbitas {
    fn new(elementos: Vec<Value>, fonte: String, cache_novo: Option<Value>) -> Orbitas {
        Orbitas { elementos, fonte, cache_novo }
    }

    /// Grava o cache novo, se há um: no fim do comando, depois das chamadas que baixam.
    fn guardar(&self) {
        if let Some(c) = &self.cache_novo
            && c.to_string().len() <= cache::MAXIMO
        {
            let _ = host::call("host.setSettings", json!({"values": {"gp": c}}));
        }
    }
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
        orbitas: req.str("orbitas").unwrap_or("do CelesTrak"),
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

/// `proximas`: as próximas passagens dos satélites de imagem gratuita, com a nuvem prevista.
fn proximas(req: &Request) -> Result<Value, String> {
    let local = lugar(req)?;
    let (lat, lon) = (local.lat, local.lon);
    let dias = req.f64("dias").unwrap_or(10.0).round().clamp(1.0, 16.0);
    let diarios = req.bool("diarios").unwrap_or(false);
    let nome_dado = req.str("nome").map(str::trim).filter(|n| !n.is_empty()).or(local.nome.as_deref());

    let meteo = buscar(&format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat:.4}&longitude={lon:.4}&current=cloud_cover&hourly=cloud_cover&forecast_days=16&timezone=auto"
    ));
    let photon = match nome_dado {
        Some(_) => Ok(Some(Value::Null)),
        None => buscar(&format!("https://photon.komoot.io/reverse?lon={lon:.5}&lat={lat:.5}&radius=20&limit=1")),
    };
    if matches!(meteo, Ok(None)) || matches!(photon, Ok(None)) {
        return Ok(json!({"pending": true}));
    }
    let meteo = meteo?.ok_or("o Open-Meteo não respondeu")?;
    let m = &meteo["json"];
    let fuso = m["utc_offset_seconds"].as_f64().unwrap_or(0.0);
    let agora = m["current"]["time"].as_str().and_then(tempo::ler).map(|t| t - fuso).ok_or("o Open-Meteo não deu a hora de agora")?;
    // A previsão hora a hora, na hora UTC.
    let nuvens: Vec<[f64; 2]> = m["hourly"]["time"]
        .as_array()
        .into_iter()
        .flatten()
        .zip(m["hourly"]["cloud_cover"].as_array().into_iter().flatten())
        .filter_map(|(h, n)| Some([tempo::ler(h.as_str()?)? - fuso, n.as_f64()?]))
        .collect();
    let nome = match nome_dado {
        Some(n) => n.to_string(),
        None => nome_do_lugar(photon.ok().flatten().as_ref()).unwrap_or_else(|| format!("{lat:.4}, {lon:.4}")),
    };
    let mut avisos: Vec<String> = Vec::new();
    let Some(orbitas) = elementos_orbitais(agora, &mut avisos)? else {
        return Ok(json!({"pending": true}));
    };
    let elementos: Vec<Value> =
        orbitas.elementos.iter().filter(|e| e["OBJECT_NAME"].as_str().and_then(gratuitos::de).is_some()).cloned().collect();
    if elementos.is_empty() {
        return Err("as órbitas não têm nenhum dos satélites de imagem gratuita".into());
    }

    let camada = format!("{PREFIXO_PROXIMAS}{nome}");
    let camada_lugar = format!("{PREFIXO_PROXIMAS}{nome} (lugar)");
    remover(&[&camada, &camada_lugar])?;
    rodar_ferramenta(
        lon,
        lat,
        json!({"tool": "plugin:passagens/proximas", "params": {
            "gp": Value::Array(elementos).to_string(),
            "nuvens": json!(nuvens).to_string(),
            "lat": lat, "lon": lon, "inicio": agora, "fim": agora + dias * 86_400.0, "fuso": fuso,
            "lugar": nome, "camada": camada, "camadaLugar": camada_lugar, "diarios": diarios,
        }}),
    )?;
    let mut relatorio = host::features(&camada_lugar, false)
        .ok()
        .and_then(|fc| fc["features"][0]["properties"]["relatorio"].as_str().map(str::to_string))
        .unwrap_or_else(|| "As passagens estão na camada.".to_string());
    if host::layers()?.as_array().into_iter().flatten().any(|l| l["name"] == camada.as_str()) {
        estilo_proximas(&camada);
        relatorio += &format!(
            "\n\nA camada {camada} tem uma linha por passagem (o traço no chão), colorida pela nuvem prevista, com a hora, a resolução e onde baixar."
        );
    }
    let _ = host::run("map.goToXY", json!({"lon": lon, "lat": lat, "scale": 15_000_000}));
    if !avisos.is_empty() {
        relatorio += &format!("\n\nAvisos: {}", avisos.join("; "));
    }
    orbitas.guardar();
    Ok(json!({"report": relatorio}))
}

/// A ferramenta `proximas`: as passagens futuras, do que o comando `proximas` buscou.
fn calcular_proximas(req: &Request) -> Result<Value, String> {
    let texto = |n: &str| req.str(n).ok_or_else(|| format!("falta '{n}': rode o comando Próximas imagens"));
    let numero = |n: &str| req.f64(n).ok_or_else(|| format!("falta '{n}': rode o comando Próximas imagens"));
    let gp: Value = serde_json::from_str(texto("gp")?).map_err(|e| format!("gp: {e}"))?;
    let nuvens: Vec<(f64, f64)> = req
        .str("nuvens")
        .and_then(|n| serde_json::from_str::<Vec<[f64; 2]>>(n).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|[t, n]| (t, n))
        .collect();
    let (lat, lon) = (numero("lat")?, numero("lon")?);
    let lugar = texto("lugar")?;
    let saida = proximas::calcular(&proximas::Entrada {
        gp: gp.as_array().map(Vec::as_slice).unwrap_or_default(),
        lon,
        lat,
        t0: numero("inicio")?,
        t1: numero("fim")?,
        fuso: req.f64("fuso").unwrap_or(0.0),
        nome: lugar,
        nuvens: &nuvens,
        diarios: req.bool("diarios").unwrap_or(false),
    });
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

/// As cores pela nuvem prevista.
fn estilo_proximas(camada: &str) {
    if host::run("layer.symbology", json!({"layer": camada, "type": "unique", "field": "ceu"})).is_err() {
        return;
    }
    for c in Ceu::TODOS {
        let _ = host::run("layer.classStyle", json!({"layer": camada, "value": c.texto(), "color": c.cor()}));
        let _ = host::run("layer.lineStrokes", json!({"layer": camada, "value": c.texto(), "strokes": [{"width": c.largura()}]}));
    }
}

/// `imagens`: as imagens do Sentinel-2 do lugar na barra de tempo, só as datas de céu limpo no
/// próprio lugar.
///
/// O cubo vem do Earth Search (arquivos públicos na AWS, que o cubo do RustGIS lê; os do
/// Planetary Computer têm endereços assinados que expiram). O céu de cada data vem do Planetary
/// Computer, que diz a classe da banda SCL num ponto.
fn imagens_do_lugar(req: &Request) -> Result<Value, String> {
    let local = lugar(req)?;
    let (lat, lon) = (local.lat, local.lon);
    let dias = req.f64("dias").unwrap_or(60.0).round().clamp(5.0, 365.0);
    let lado = req.f64("lado").unwrap_or(5.0).clamp(1.0, 20.0);
    let quantas = req.f64("quantas").unwrap_or(6.0).round().clamp(1.0, 10.0) as usize;
    let nome_dado = req.str("nome").map(str::trim).filter(|n| !n.is_empty()).or(local.nome.as_deref());

    let meteo =
        buscar(&format!("https://api.open-meteo.com/v1/forecast?latitude={lat:.4}&longitude={lon:.4}&current=cloud_cover&timezone=auto"));
    let photon = match nome_dado {
        Some(_) => Ok(Some(Value::Null)),
        None => buscar(&format!("https://photon.komoot.io/reverse?lon={lon:.5}&lat={lat:.5}&radius=20&limit=1")),
    };
    if matches!(meteo, Ok(None)) || matches!(photon, Ok(None)) {
        return Ok(json!({"pending": true}));
    }
    let meteo = meteo?.ok_or("o Open-Meteo não respondeu")?;
    let m = &meteo["json"];
    let fuso = m["utc_offset_seconds"].as_f64().unwrap_or(0.0);
    let agora =
        m["current"]["time"].as_str().and_then(tempo::ler).map(|t| t - fuso + 15.0 * 60.0).ok_or("o Open-Meteo não deu a hora de agora")?;
    let nome = match nome_dado {
        Some(n) => n.to_string(),
        None => nome_do_lugar(photon.ok().flatten().as_ref()).unwrap_or_else(|| format!("{lat:.4}, {lon:.4}")),
    };
    let t0 = agora - dias * 86_400.0;

    // As imagens do lugar no Planetary Computer, uma por dia: as `ITENS` cenas mais recentes.
    let r = host::run(
        "stac.search",
        json!({"url": STAC, "collections": ["sentinel-2-l2a"], "bbox": [lon - 0.0005, lat - 0.0005, lon + 0.0005, lat + 0.0005],
               "datetime": format!("{}/{}", tempo::dia(t0), tempo::dia(agora)), "limit": galeria::ITENS, "sortby": "-datetime", "footprints": false}),
    )?;
    if esperando(&r) {
        return Ok(json!({"pending": true}));
    }
    let itens = imagens::ler(&r);
    // Com a busca cheia, o período pedido tem mais cenas: a lista começa na mais velha lida.
    let desde = (itens.len() >= galeria::ITENS).then(|| itens.iter().map(|i| i.t).fold(f64::INFINITY, f64::min));
    let imgs = galeria::uma_por_dia(itens);
    if imgs.is_empty() {
        return Err(format!("o Planetary Computer não tem imagens do Sentinel-2 de {nome} nos últimos {dias:.0} dias"));
    }

    // O céu no pixel do lugar, das mais novas às mais velhas. No app web as consultas chegam
    // depois: o comando roda de novo quando chegam, e as já feitas respondem de pronto.
    let mut datas: Vec<galeria::Data> = Vec::new();
    let mut pendentes = 0;
    let mut consultas = 0;
    let mut falhas = 0;
    for img in imgs {
        let (ceu, classe) = match galeria::pela_cena(img.nuvem) {
            Some(c) => c,
            // O tempo antes do número: quando os dois acabam juntos, foi o tempo que faltou.
            None if falhas >= FALHAS => (galeria::Ceu::NaoConferido, "o tempo das consultas acabou"),
            None if consultas >= CONFERIR => (galeria::Ceu::NaoConferido, "o limite de consultas acabou"),
            // No app web, no máximo 8 consultas esperam por rodada.
            None if pendentes >= 8 => {
                pendentes += 1;
                (galeria::Ceu::NaoConferido, "chegando")
            }
            None => {
                consultas += 1;
                let url = format!(
                    "https://planetarycomputer.microsoft.com/api/data/v1/item/point/{lon:.6},{lat:.6}?collection=sentinel-2-l2a&item={}&assets=SCL",
                    img.id
                );
                match buscar(&url) {
                    Ok(None) => {
                        pendentes += 1;
                        (galeria::Ceu::NaoConferido, "chegando")
                    }
                    Ok(Some(p)) => {
                        falhas = 0;
                        p["json"]["values"][0].as_f64().map(galeria::classe_scl).unwrap_or((galeria::Ceu::SemDado, "sem dado"))
                    }
                    Err(_) => {
                        falhas += 1;
                        (galeria::Ceu::NaoConferido, "a consulta falhou")
                    }
                }
            }
        };
        datas.push(galeria::Data { imagem: img, ceu, classe });
    }
    if pendentes > 0 {
        return Ok(json!({"pending": true}));
    }

    // As datas da barra de tempo: as limpas mais novas.
    let limpas: Vec<String> = datas.iter().filter(|d| d.ceu == galeria::Ceu::Limpo).take(quantas).map(|d| tempo::dia(d.imagem.t)).collect();

    // Os cubos com as imagens do Earth Search, um por data limpa.
    //
    // Clicado no painel ou na faixa, o comando não espera os downloads: o RustGIS desfaz o que ele
    // fez, baixa, e roda o comando de novo, e o cubo que o download pôs fica no mapa. Por isso nada
    // sai do mapa antes dos `stac.addCube`: um cubo de antes tirado aqui seria, na segunda rodada,
    // o que o download acabou de pôr ("the layer its download added was removed"). A data que já
    // tem cubo (de uma rodada anterior, ou do download) fica com ele; as que não são mais limpas
    // saem no fim.
    let camada = format!("{PREFIXO_IMAGENS}{nome}");
    let cubos_no_mapa = || -> Result<Vec<String>, String> {
        let projeto = host::run("info.project", json!({}))?;
        Ok(projeto["tileLayers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t["name"].as_str())
            .filter(|n| nomes::data_do_cubo(n, &camada).is_some())
            .map(str::to_string)
            .collect())
    };
    let do_dia = |n: &str, dia: &str| nomes::data_do_cubo(n, &camada) == Some(dia);
    let mut relatorio = galeria::relatorio(&nome, dias, desde, &datas, fuso, calculo::satelite_legivel);
    if falhas >= FALHAS {
        relatorio += "\nO tempo das consultas desta rodada acabou antes de conferir todas as datas: rode de novo para conferir as que faltaram (as já conferidas ficam guardadas por 10 minutos e voltam na hora).\n";
    }
    if limpas.is_empty() {
        relatorio += "\nNenhuma das datas conferidas tem o céu limpo no lugar: rode de novo com mais dias.";
        return Ok(json!({"report": relatorio}));
    }
    // Um cubo por data, com uma imagem só: a janela da barra de tempo vai de um passo ao
    // seguinte, e um cubo com todas as datas mostraria a última dentro dela, nublada ou não. Os
    // mais velhos entram por último, por cima, e cada passo mostra a sua data.
    let meio = lado / 2.0 / 111.32;
    let meio_lon = meio / lat.to_radians().cos().max(0.01);
    let ja_no_mapa = cubos_no_mapa()?;
    let mut passos: Vec<String> = Vec::new();
    let mut falhas: Vec<String> = Vec::new();
    let mut baixando = false;
    for dia in &limpas {
        if ja_no_mapa.iter().any(|n| do_dia(n, dia)) {
            passos.push(dia.clone());
            continue;
        }
        let cubo = host::run(
            "stac.addCube",
            json!({"collection": "sentinel-2-l2a", "asset": "visual",
                   "bbox": [lon - meio_lon, lat - meio, lon + meio_lon, lat + meio],
                   "start": dia, "end": dia, "name": format!("{camada} {dia}"),
                   "attribution": "Sentinel-2 (Copernicus), Earth Search (Element 84)"}),
        );
        match cubo {
            // Os downloads começam juntos: o comando volta quando todos chegarem.
            Ok(r) if esperando(&r) => baixando = true,
            Ok(r) if r["dates"].as_u64().unwrap_or(0) > 0 => passos.push(dia.clone()),
            Ok(_) => falhas.push(format!("{dia}: o Earth Search não tem a imagem")),
            Err(e) => {
                falhas.push(format!("{dia}: {e}"));
                // Sem cubo aqui (o app web), nenhum outro entra.
                if passos.is_empty() && !baixando {
                    break;
                }
            }
        }
    }
    if baixando {
        return Ok(json!({"pending": true}));
    }
    // Agora, depois de todos os `stac.addCube`, saem os cubos das datas que não são mais limpas.
    for n in cubos_no_mapa()?.into_iter().filter(|n| !limpas.iter().any(|dia| do_dia(n, dia))) {
        let _ = host::run("map.removeTiles", json!({"name": n}));
    }
    if passos.is_empty() {
        relatorio += &format!(
            "\nAs imagens não entraram no mapa ({}). Elas entram como cubos na barra de tempo, que só o app desktop e o rustgis-cli fazem.",
            falhas.join("; ")
        );
        return Ok(json!({"report": relatorio}));
    }
    passos.reverse();
    let ultima = passos.last().cloned().unwrap_or_default();
    host::run("time.set", json!({"dates": passos, "date": ultima, "speed": 1500, "loop": true, "format": "DD/MM/YYYY"}))?;
    let _ = host::run("map.goToXY", json!({"lon": lon, "lat": lat, "scale": (lado * 5000.0).round()}));
    imagens_por_baixo(&nome);
    relatorio += &if passos.len() == 1 {
        format!(
            "\nA data de céu limpo mais recente está na barra de tempo (View › Time), na camada {camada}, e o mapa a mostra agora ({ultima})."
        )
    } else {
        format!(
            "\nAs {} datas de céu limpo mais recentes estão na barra de tempo (View › Time), uma camada {camada} por data, e o mapa mostra agora a de {ultima}. O ▶ da barra passa as datas uma a uma; Exportar animação (time.export) faz um GIF.",
            passos.len()
        )
    };
    if !falhas.is_empty() {
        relatorio += &format!("\nFicaram de fora: {}.", falhas.join("; "));
    }
    Ok(json!({"report": relatorio}))
}

/// A camada provisória que o mapa vazio ganha enquanto uma ferramenta roda.
const PROVISORIA: &str = "Passagens: calculando";

/// Roda uma ferramenta do plugin pelo `gp.run`. O RustGIS não roda ferramentas num mapa sem
/// camadas (um projeto novo): ele ganha, enquanto isso, uma camada com o ponto do lugar.
fn rodar_ferramenta(lon: f64, lat: f64, pedido: Value) -> Result<Value, String> {
    let vazio = host::layers()?.as_array().is_none_or(|a| a.is_empty());
    if vazio {
        host::call(
            "host.addLayer",
            json!({"name": PROVISORIA, "geojson": geojson::collection(vec![geojson::feature(geojson::point(lon, lat), json!({}))])}),
        )?;
    }
    let r = host::run("gp.run", pedido);
    if vazio {
        let _ = remover(&[PROVISORIA]);
    }
    r
}

/// Remove as camadas `nomes` que estiverem no mapa (de uma rodada anterior), as repetidas também.
///
/// Pelo nome, uma por vez, e sem erro quando ela não está: o `layer.remove` com uma lista de ids
/// falha inteiro se um id não existe mais ("select a layer in the Layers pane first"), e um
/// comando clicado roda de novo depois de desfeito, quando os ids lidos antes já não valem.
fn remover(nomes: &[&str]) -> Result<(), String> {
    for nome in nomes {
        // Até 20 camadas com o mesmo nome; o `layer.remove` falha quando não há mais nenhuma.
        for _ in 0..20 {
            if host::run("layer.remove", json!({"layer": nome})).is_err() {
                break;
            }
        }
    }
    Ok(())
}

/// Põe as imagens do lugar `nome` por baixo das linhas e do ponto dele, que sem isso ficavam
/// escondidos: uma imagem nova (raster ou cubo) entra por cima de tudo.
///
/// As camadas vetoriais do lugar sobem para o topo, e os cubos da barra de tempo do lugar descem
/// para logo abaixo delas: um cubo é desenhado sobre a camada que estava no topo quando ele entrou,
/// e pode ser uma dessas. Os cubos ficam na ordem em que estavam (com a mesma posição pedida, o
/// que vai depois fica por baixo). Sem efeito para o resultado do comando: a ordem é só a vista.
///
/// O `to` do `map.setTiles` conta a posição entre as outras camadas, os outros cubos também: com
/// os cubos do lugar no topo (clicado, o RustGIS põe no topo os cubos que o download trouxe), o
/// primeiro parava acima das linhas, por causa dos outros ainda por cima. Por isso todos descem
/// primeiro para o fundo, e só então sobem, um a um, para logo abaixo das camadas do lugar.
fn imagens_por_baixo(nome: &str) {
    let Ok(camadas) = host::layers() else { return };
    let no_mapa: Vec<&str> = camadas.as_array().into_iter().flatten().filter_map(|l| l["name"].as_str()).collect();
    // Toda camada do lugar no mapa conta, movida ou não: o `layer.move` falha com a camada que já
    // está no topo (as linhas que o comando acabou de pôr), e ela continua lá em cima.
    let mut no_topo = 0;
    for camada in nomes::camadas_do_lugar(nome) {
        if no_mapa.contains(&camada.as_str()) {
            let _ = host::run("layer.move", json!({"layer": camada, "to": 0}));
            no_topo += 1;
        }
    }
    let Ok(projeto) = host::run("info.project", json!({})) else { return };
    let cubo = format!("{PREFIXO_IMAGENS}{nome}");
    // Na ordem do mapa, de cima para baixo.
    let cubos: Vec<String> = projeto["tileLayers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["name"].as_str())
        .filter(|n| nomes::data_do_cubo(n, &cubo).is_some())
        .map(str::to_string)
        .collect();
    for n in &cubos {
        let _ = host::run("map.setTiles", json!({"name": n, "move": "bottom"}));
    }
    for n in &cubos {
        let _ = host::run("map.setTiles", json!({"name": n, "to": no_topo}));
    }
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

/// Põe a imagem do item no mapa; o nome da camada. `area` é onde buscar a mesma imagem no Earth
/// Search: uma caixa `[oeste, sul, leste, norte]` ou uma geometria GeoJSON.
///
/// O Sentinel-2 vem do Earth Search quando o mesmo item está lá: os arquivos dele são públicos, e
/// os endereços do Planetary Computer são assinados e expiram (a camada de um projeto salvo
/// ficaria sem imagem). Os outros satélites vêm do Planetary Computer.
///
/// `None`: a imagem está chegando. Clicado, o comando não espera a imagem: o RustGIS a lê depois
/// e roda o comando de novo, e esta chamada responde então o que a leitura pôs no mapa.
///
/// O nome leva o lugar ("Passagens: Viçosa, Sentinel-2C 2026-10-09 10:02, 2% de nuvem"): a mesma
/// data de outro lugar é outra imagem, e não sai do mapa quando esta entra.
fn adicionar_imagem(i: &Imagem, lugar: &str, satelite: &str, quando: &str, area: &Value) -> Result<Option<String>, String> {
    let nuvem = i.nuvem.map(|n| format!(", {n:.0}% de nuvem")).unwrap_or_default();
    let nome = format!("{PREFIXO}{lugar}, {satelite} {quando}{nuvem}");
    let p = match item_publico(i, area) {
        Some(item) => json!({"item": item, "asset": "visual", "name": nome}),
        None => {
            let mut p = json!({"url": i.url, "name": nome});
            match i.colecao.as_str() {
                "sentinel-2-l2a" => p["asset"] = json!("visual"),
                // O GRD não é georreferenciado: a mesma cena no RTC.
                "sentinel-1-grd" => {
                    p["url"] = json!(
                        imagens::endereco_rtc(&i.url).ok_or_else(|| format!("o endereço {} não é de um item do Sentinel-1 GRD", i.url))?
                    );
                    p["asset"] = json!("vv");
                }
                _ => {}
            }
            p
        }
    };
    let radar = i.colecao == "sentinel-1-grd";
    let r = host::run("stac.addAsset", p).map_err(|e| {
        if radar {
            format!("não deu para abrir a cena de radar corrigida do terreno (sentinel-1-rtc) do Planetary Computer, que pode ainda não ter sido publicada ({e})")
        } else {
            e
        }
    })?;
    // Chegando: o comando roda de novo quando a imagem estiver no mapa.
    if esperando(&r) {
        return Ok(None);
    }
    // A mesma imagem de uma rodada anterior sai só agora, depois do download: tirada antes, a
    // rodada de novo de um comando clicado acharia a sua camada nova tirada. Com o nome ainda em
    // uso, a nova entrou como "Nome (2)", e volta ao nome certo.
    let entrou = r["name"].as_str().unwrap_or(&nome).to_string();
    let velhas: Vec<String> = host::layers()?
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| l["name"].as_str())
        .filter(|n| *n != entrou && (*n == nome || n.strip_prefix(&nome).is_some_and(nomes::copia)))
        .map(str::to_string)
        .collect();
    let velhas: Vec<&str> = velhas.iter().map(String::as_str).collect();
    remover(&velhas)?;
    if entrou != nome {
        host::run("layer.rename", json!({"layer": entrou, "name": nome}))?;
    }
    imagens_por_baixo(lugar);
    Ok(Some(nome))
}

/// O mesmo item do Sentinel-2 no Earth Search (arquivos públicos na AWS): o do mesmo satélite, na
/// mesma grade MGRS e no mesmo dia. Entre os reprocessados (`_0_L2A`, `_1_L2A`), o mais novo.
fn item_publico(i: &Imagem, area: &Value) -> Option<Value> {
    if i.colecao != "sentinel-2-l2a" {
        return None;
    }
    let (sat, dia, grade) = imagens::partes_s2(&i.id)?;
    let mut p = json!({"collections": ["sentinel-2-l2a"], "datetime": dia, "limit": 50, "footprints": false});
    if area.is_array() {
        p["bbox"] = area.clone();
    } else {
        p["intersects"] = area.clone();
    }
    let r = host::run("stac.search", p).ok()?;
    if esperando(&r) {
        return None;
    }
    let comeco = format!("{sat}_{grade}_");
    r["items"]
        .as_array()?
        .iter()
        .filter(|it| it["id"].as_str().is_some_and(|id| id.starts_with(&comeco)))
        .max_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()))
        .cloned()
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
        let camada = l["name"].as_str().unwrap_or_default();
        let fc = host::features(camada, true)?;
        // Onde buscar a imagem: o ponto do lugar, da camada "(lugar)" ao lado; sem ela, o traço.
        let lugar = host::features(&format!("{camada} (lugar)"), false)
            .ok()
            .and_then(|p| geojson::positions(&p["features"][0]["geometry"]).first().copied())
            .map(|[x, y]| json!([x - 0.0005, y - 0.0005, x + 0.0005, y + 0.0005]));
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
            let area = lugar.clone().unwrap_or_else(|| f["geometry"].clone());
            let lugar = camada.strip_prefix(PREFIXO).unwrap_or(camada);
            let Some(nome) = adicionar_imagem(
                &img,
                lugar,
                p["satelite"].as_str().unwrap_or_default(),
                p["hora_local"].as_str().unwrap_or_default(),
                &area,
            )?
            else {
                return Ok(json!({"pending": true}));
            };
            return Ok(json!({"report": format!("No mapa: {nome}.")}));
        }
    }
    Err("selecione uma passagem que fotografou (categoria \"fotografou\") numa camada do Passagens".into())
}
