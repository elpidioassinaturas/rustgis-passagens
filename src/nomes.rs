//! Os nomes das camadas.

/// O prefixo das camadas do comando `passagens`.
pub const PREFIXO: &str = "Passagens: ";

/// O prefixo das camadas do comando `proximas`.
pub const PREFIXO_PROXIMAS: &str = "Próximas imagens: ";

/// O prefixo do cubo do comando `imagens`.
pub const PREFIXO_IMAGENS: &str = "Imagens: ";

/// As camadas de linhas e de ponto do lugar `nome`, na ordem em que sobem para o topo quando uma
/// imagem entra: cada uma sobe por cima da anterior, e assim os pontos ficam por cima das linhas,
/// e as Passagens por cima das Próximas imagens.
pub fn camadas_do_lugar(nome: &str) -> [String; 4] {
    [
        format!("{PREFIXO_PROXIMAS}{nome}"),
        format!("{PREFIXO_PROXIMAS}{nome} (lugar)"),
        format!("{PREFIXO}{nome}"),
        format!("{PREFIXO}{nome} (lugar)"),
    ]
}

/// Se o fim de um nome é o número que o RustGIS põe numa camada de nome repetido: " (2)", " (3)"...
pub fn copia(fim: &str) -> bool {
    fim.strip_prefix(" (").and_then(|r| r.strip_suffix(')')).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// A data (`AAAA-MM-DD`) de um cubo do comando `imagens` chamado `nome`, se ele é da camada
/// `camada` ("Imagens: Viçosa 2026-11-13", ou com a grade: "Imagens: Viçosa 2026-11-13
/// MGRS-23KQT"). O nome tem de seguir com um espaço e a data: "Imagens: Viçosa do Ceará
/// 2026-11-13" não é de "Imagens: Viçosa".
pub fn data_do_cubo<'a>(nome: &'a str, camada: &str) -> Option<&'a str> {
    let data = nome.strip_prefix(camada)?.strip_prefix(' ')?.get(..10)?;
    let b = data.as_bytes();
    let digitos = [0, 1, 2, 3, 5, 6, 8, 9].iter().all(|&i| b[i].is_ascii_digit());
    (digitos && b[4] == b'-' && b[7] == b'-').then_some(data)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn copias_de_um_nome() {
        assert!(copia(" (2)"));
        assert!(copia(" (13)"));
        assert!(!copia(""));
        assert!(!copia(" ()"));
        assert!(!copia(" (a)"));
        assert!(!copia(" (lugar)"));
        assert!(!copia("2)"));
    }

    #[test]
    fn camadas_que_sobem_por_cima_das_imagens() {
        let c = camadas_do_lugar("Viçosa");
        // O último a subir fica no topo: o ponto das Passagens.
        assert_eq!(c[3], "Passagens: Viçosa (lugar)");
        assert_eq!(c[2], "Passagens: Viçosa");
        assert_eq!(c[1], "Próximas imagens: Viçosa (lugar)");
        assert_eq!(c[0], "Próximas imagens: Viçosa");
        // A imagem raster do lugar não é uma delas: ela fica por baixo.
        assert!(!c.iter().any(|n| n.starts_with("Passagens: Viçosa,")));
    }

    #[test]
    fn cubos_de_um_lugar() {
        let c = "Imagens: Viçosa";
        assert_eq!(data_do_cubo("Imagens: Viçosa 2026-11-13", c), Some("2026-11-13"));
        assert_eq!(data_do_cubo("Imagens: Viçosa 2026-10-01 MGRS-23KRU", c), Some("2026-10-01"));
        assert_eq!(data_do_cubo("Imagens: Viçosa do Ceará 2026-11-13", c), None);
        assert_eq!(data_do_cubo("Imagens: Viçosa", c), None);
        assert_eq!(data_do_cubo("Imagens: Viçosa 2026-1", c), None);
        assert_eq!(data_do_cubo("Imagens: Ubaporanga 2026-11-13", c), None);
    }
}
