//! Os satélites cujas imagens qualquer um pode baixar de graça, pelo nome que o CelesTrak dá a
//! eles. O comando `proximas` calcula só as passagens deles.

/// Um satélite (ou uma família) de imagens gratuitas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gratuito {
    /// O começo do nome no CelesTrak (`SENTINEL-2` pega o 2A, o 2B e o 2C).
    pub prefixo: &'static str,
    /// O nome tem de ser só o prefixo (`TERRA`, e não `TERRASAR-X`).
    pub exato: bool,
    /// O instrumento e a resolução.
    pub resolucao: &'static str,
    /// Onde baixar as imagens.
    pub onde: &'static str,
    /// Se a passagem pode não virar imagem: o satélite grava só o que o plano de aquisição
    /// manda. `None`: ele grava toda a terra que vê de dia.
    pub condicao: Option<&'static str>,
    /// Radar: vê através das nuvens, de dia e de noite.
    pub radar: bool,
    /// Resolução grossa (250 m a 1 km), com passagem todo dia.
    pub diario: bool,
}

const fn optico(prefixo: &'static str, resolucao: &'static str, onde: &'static str) -> Gratuito {
    Gratuito { prefixo, exato: false, resolucao, onde, condicao: None, radar: false, diario: false }
}

const fn diario(prefixo: &'static str, exato: bool, resolucao: &'static str, onde: &'static str) -> Gratuito {
    Gratuito { prefixo, exato, resolucao, onde, condicao: None, radar: false, diario: true }
}

const INPE: &str = "catálogo do INPE (www.dgi.inpe.br/catalogo)";
const PROGRAMACAO_INPE: &str = "grava conforme a programação do INPE";

pub const GRATUITOS: [Gratuito; 13] = [
    optico("SENTINEL-2", "MSI, 10 m", "Copernicus Browser e Planetary Computer"),
    optico("LANDSAT 8", "OLI e TIRS, 30 m", "USGS EarthExplorer e Planetary Computer"),
    optico("LANDSAT 9", "OLI-2 e TIRS-2, 30 m", "USGS EarthExplorer e Planetary Computer"),
    Gratuito {
        prefixo: "SENTINEL-1",
        exato: false,
        resolucao: "radar C, 10 m",
        onde: "Copernicus Browser e Planetary Computer",
        condicao: Some("grava conforme o plano de aquisição da ESA"),
        radar: true,
        diario: false,
    },
    Gratuito { condicao: Some(PROGRAMACAO_INPE), ..optico("CBERS 4A", "WPM 2 e 8 m, MUX 16 m, WFI 55 m", INPE) },
    Gratuito { condicao: Some(PROGRAMACAO_INPE), ..optico("CBERS 4", "PAN 5 e 10 m, MUX 20 m, WFI 64 m", INPE) },
    Gratuito { condicao: Some(PROGRAMACAO_INPE), ..optico("AMAZONIA 1", "WFI, 60 m", INPE) },
    diario("SENTINEL-3", false, "OLCI, 300 m", "Copernicus Browser"),
    diario("TERRA", true, "MODIS, 250 m a 1 km", "NASA Earthdata e Worldview"),
    diario("AQUA", true, "MODIS, 250 m a 1 km", "NASA Earthdata e Worldview"),
    diario("SUOMI NPP", false, "VIIRS, 375 e 750 m", "NASA Earthdata e Worldview"),
    diario("NOAA 20", false, "VIIRS, 375 e 750 m", "NASA Earthdata e Worldview"),
    diario("NOAA 21", false, "VIIRS, 375 e 750 m", "NASA Earthdata e Worldview"),
];

/// Os que o CelesTrak ainda rastreia, mas não fazem mais imagens.
const FORA_DE_OPERACAO: [&str; 1] = [
    "SENTINEL-1B", // falhou em dezembro de 2021
];

/// O satélite gratuito chamado `nome` no CelesTrak.
pub fn de(nome: &str) -> Option<&'static Gratuito> {
    let n = nome.trim().to_uppercase();
    if FORA_DE_OPERACAO.contains(&n.as_str()) {
        return None;
    }
    GRATUITOS.iter().find(|g| if g.exato { n == g.prefixo } else { n.starts_with(g.prefixo) })
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn nomes_do_celestrak() {
        assert_eq!(de("SENTINEL-2C").unwrap().resolucao, "MSI, 10 m");
        assert!(de("SENTINEL-1C").unwrap().radar);
        assert!(de("SENTINEL-1B").is_none());
        assert_eq!(de("CBERS 4A").unwrap().prefixo, "CBERS 4A");
        assert_eq!(de("CBERS 4").unwrap().prefixo, "CBERS 4");
        assert!(de("TERRA").unwrap().diario);
        assert!(de("TERRASAR-X").is_none());
        assert!(de("NOAA 20 (JPSS-1)").is_some());
        assert!(de("NOAA 19").is_none());
        assert!(de("WORLDVIEW-3 (WV-3)").is_none());
        // Todo gratuito tem um perfil de sensor, sem o qual não há passagem que "vê".
        for g in GRATUITOS {
            let nome = if g.prefixo == "SENTINEL-2" || g.prefixo == "SENTINEL-1" || g.prefixo == "SENTINEL-3" {
                format!("{}A", g.prefixo)
            } else {
                g.prefixo.to_string()
            };
            assert!(crate::perfis::perfil(&nome).is_some(), "{nome}");
        }
    }
}
