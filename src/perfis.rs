//! O que cada satélite consegue ver, pelo nome que o CelesTrak dá a ele.
//!
//! - **Nadir:** a câmera olha para baixo e grava uma faixa fixa (Sentinel-2, Landsat, MODIS…).
//!   O lugar é visto quando está dentro da meia-faixa e é dia.
//! - **Radar lateral:** o Sentinel-1 olha para a direita da órbita, numa faixa entre duas
//!   distâncias do traço; não precisa de sol.
//! - **Apontável:** a câmera (ou o radar) inclina até um ângulo. O lugar pode ter sido visto
//!   quando está ao alcance; se foi, o operador não publica.
//!
//! As larguras são as nominais dos instrumentos principais, arredondadas.

/// O tipo de visão.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sensor {
    /// Meia-faixa em km.
    Nadir { meia_faixa: f64 },
    /// Distâncias do traço, em km, à direita da órbita.
    Radar { perto: f64, longe: f64 },
    /// O maior ângulo fora do nadir, em graus; `radar` não precisa de sol.
    Apontavel { angulo: f64, radar: bool },
}

/// Um satélite conhecido.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Perfil {
    pub sensor: Sensor,
    /// O começo do id dos itens dele no STAC do Planetary Computer (`S2B`, `LC09`, `S1C`).
    pub stac: Option<&'static str>,
}

const fn nadir(meia_faixa: f64) -> Perfil {
    Perfil { sensor: Sensor::Nadir { meia_faixa }, stac: None }
}

const fn apontavel(angulo: f64) -> Perfil {
    Perfil { sensor: Sensor::Apontavel { angulo, radar: false }, stac: None }
}

const fn sar(angulo: f64) -> Perfil {
    Perfil { sensor: Sensor::Apontavel { angulo, radar: true }, stac: None }
}

/// O perfil do satélite chamado `nome` no CelesTrak, ou `None` para os que não fotografam o chão
/// (ou que este plugin não conhece): esses só "passam por cima".
pub fn perfil(nome: &str) -> Option<Perfil> {
    let n = nome.to_uppercase();
    let comeca = |p: &str| n.starts_with(p);
    // Os que têm imagens no Planetary Computer.
    if let Some(letra) = n.strip_prefix("SENTINEL-2") {
        return Some(Perfil { sensor: Sensor::Nadir { meia_faixa: 145.0 }, stac: stac_sentinel("S2", letra) });
    }
    // O Sentinel-1B falhou em dezembro de 2021: só passa por cima.
    if n == "SENTINEL-1B" {
        return None;
    }
    if let Some(letra) = n.strip_prefix("SENTINEL-1") {
        return Some(Perfil { sensor: Sensor::Radar { perto: 240.0, longe: 490.0 }, stac: stac_sentinel("S1", letra) });
    }
    if comeca("LANDSAT 8") {
        return Some(Perfil { sensor: Sensor::Nadir { meia_faixa: 92.5 }, stac: Some("LC08") });
    }
    if comeca("LANDSAT 9") {
        return Some(Perfil { sensor: Sensor::Nadir { meia_faixa: 92.5 }, stac: Some("LC09") });
    }
    // MODIS, 2.330 km (pelo nome inteiro: TERRASAR-X também começa com TERRA).
    if n == "TERRA" || n == "AQUA" {
        return Some(nadir(1165.0));
    }
    // Faixa fixa, sem imagens no Planetary Computer.
    let tabela: &[(&str, Perfil)] = &[
        ("SENTINEL-3", nadir(635.0)),  // OLCI, 1.270 km
        ("SUOMI NPP", nadir(1520.0)),  // VIIRS, 3.040 km
        ("NOAA 20", nadir(1520.0)),    // VIIRS
        ("NOAA 21", nadir(1520.0)),    // VIIRS
        ("METOP", nadir(1450.0)),      // AVHRR, 2.900 km
        ("FENGYUN 3", nadir(1400.0)),  // MERSI
        ("METEOR-M", nadir(1400.0)),   // MSU-MR
        ("CBERS 4A", nadir(342.0)),    // WFI, 684 km
        ("CBERS 4", nadir(433.0)),     // WFI, 866 km
        ("AMAZONIA 1", nadir(425.0)),  // WFI, 850 km
        ("RESOURCESAT", nadir(370.0)), // AWiFS, 740 km
        ("GAOFEN-1", nadir(400.0)),    // WFV, 800 km
        ("GAOFEN-6", nadir(400.0)),    // WFV
        ("HUANJING", nadir(360.0)),    // CCD, 720 km
        ("PLANET", nadir(12.0)),       // Doves
        ("FLOCK", nadir(12.0)),        // Doves (PlanetScope), 24 km
        // Apontáveis ópticos (sol), em graus fora do nadir.
        ("WORLDVIEW", apontavel(45.0)),
        ("GEOEYE", apontavel(45.0)),
        ("LEGION", apontavel(45.0)),
        ("PLEIADES", apontavel(47.0)),
        ("SPOT", apontavel(45.0)),
        ("SKYSAT", apontavel(30.0)),
        ("PELICAN", apontavel(30.0)),
        ("ARIRANG-3", apontavel(45.0)),
        ("KOMPSAT-3", apontavel(45.0)),
        ("CARTOSAT", apontavel(26.0)),
        ("GAOFEN-2", apontavel(35.0)),
        ("DEIMOS-2", apontavel(45.0)),
        ("CO3D", apontavel(30.0)),
        ("NEMO-HD", apontavel(30.0)),
        ("VNREDSAT", apontavel(30.0)),
        ("THEOS", apontavel(50.0)),
        ("KAZEOSAT 1", apontavel(35.0)),
        ("DUBAISAT", apontavel(30.0)),
        ("RESURS-P", apontavel(45.0)),
        ("ZIYUAN 3", apontavel(30.0)),
        // Apontáveis de radar (sem sol).
        ("SENTINEL-1", sar(46.0)),
        ("RADARSAT", sar(49.0)),
        ("TERRASAR-X", sar(45.0)),
        ("TANDEM-X", sar(45.0)),
        ("COSMO-SKYMED", sar(50.0)),
        ("CSG-", sar(50.0)),
        ("ALOS-2", sar(60.0)),
        ("ALOS-4", sar(60.0)),
        ("ARIRANG-5", sar(45.0)),
        ("GAOFEN-3", sar(50.0)),
        ("ICEYE", sar(35.0)),
        ("CAPELLA", sar(45.0)),
        ("UMBRA", sar(45.0)),
    ];
    tabela.iter().find(|(p, _)| comeca(p)).map(|(_, perfil)| *perfil)
}

/// `S2` + a letra do satélite (`SENTINEL-2B` → `S2B`).
fn stac_sentinel(missao: &'static str, resto: &str) -> Option<&'static str> {
    let letra = resto.trim().chars().next()?;
    Some(match (missao, letra) {
        ("S2", 'A') => "S2A",
        ("S2", 'B') => "S2B",
        ("S2", 'C') => "S2C",
        ("S2", 'D') => "S2D",
        ("S1", 'A') => "S1A",
        ("S1", 'B') => "S1B",
        ("S1", 'C') => "S1C",
        ("S1", 'D') => "S1D",
        _ => return None,
    })
}

/// As coleções do Planetary Computer que este plugin procura.
pub const COLECOES: [&str; 3] = ["sentinel-2-l2a", "landsat-c2-l2", "sentinel-1-grd"];

/// O começo do id de um item do STAC que diz o satélite (`S2B_MSIL2A_…` → `S2B`,
/// `LC09_L2SP_…` → `LC09`).
pub fn satelite_do_item(id: &str) -> &str {
    let fim = id.find('_').unwrap_or(id.len());
    &id[..fim]
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn nomes_do_celestrak() {
        assert_eq!(perfil("SENTINEL-2C").unwrap().stac, Some("S2C"));
        assert_eq!(perfil("SENTINEL-1D").unwrap().stac, Some("S1D"));
        assert!(matches!(perfil("SENTINEL-1A").unwrap().sensor, Sensor::Radar { .. }));
        assert_eq!(perfil("LANDSAT 9").unwrap().stac, Some("LC09"));
        assert_eq!(perfil("CBERS 4A").unwrap().sensor, Sensor::Nadir { meia_faixa: 342.0 });
        assert_eq!(perfil("CBERS 4").unwrap().sensor, Sensor::Nadir { meia_faixa: 433.0 });
        assert_eq!(perfil("WORLDVIEW-3 (WV-3)").unwrap().sensor, Sensor::Apontavel { angulo: 45.0, radar: false });
        assert!(perfil("ISS (ZARYA)").is_none());
        assert_eq!(perfil("TERRA").unwrap().sensor, Sensor::Nadir { meia_faixa: 1165.0 });
        assert_eq!(perfil("TERRASAR-X").unwrap().sensor, Sensor::Apontavel { angulo: 45.0, radar: true });
        assert_eq!(satelite_do_item("S2B_MSIL2A_20261113T130249_R095_T23KQT_20261113T152136"), "S2B");
        assert_eq!(satelite_do_item("LC09_L2SP_217074_20261110_02_T1"), "LC09");
    }
}
