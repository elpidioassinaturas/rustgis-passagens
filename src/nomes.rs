//! Os nomes das camadas.

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
