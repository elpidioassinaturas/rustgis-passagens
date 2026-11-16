//! Os municípios do Brasil, com a sede de cada um, para escolher o lugar pelo nome em vez de
//! coordenadas. A lista vai dentro do plugin (`municipios.csv`, gerado por
//! `scripts/municipios.py` a partir do kelvins/municipios-brasileiros, licença MIT).

use serde_json::{Value, json};

const CSV: &str = include_str!("municipios.csv");

/// Um município.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Municipio {
    pub codigo: &'static str,
    pub nome: &'static str,
    pub uf: &'static str,
    pub lat: f64,
    pub lon: f64,
}

impl Municipio {
    /// "Ubaporanga/MG".
    pub fn rotulo(&self) -> String {
        format!("{}/{}", self.nome, self.uf)
    }
}

/// Todos, em ordem de UF e de nome.
pub fn todos() -> impl Iterator<Item = Municipio> {
    CSV.lines().filter_map(|l| {
        let mut c = l.split(';');
        Some(Municipio { codigo: c.next()?, nome: c.next()?, uf: c.next()?, lat: c.next()?.parse().ok()?, lon: c.next()?.parse().ok()? })
    })
}

/// O município de código `codigo` (do IBGE, 7 dígitos).
pub fn por_codigo(codigo: &str) -> Option<Municipio> {
    todos().find(|m| m.codigo == codigo.trim())
}

/// Sem acentos, em minúsculas e com um espaço só entre as palavras, para comparar nomes.
pub fn normal(texto: &str) -> String {
    let sem: String = texto
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            'ñ' | 'Ñ' => 'n',
            '\'' | '’' | '`' => ' ',
            c => c.to_ascii_lowercase(),
        })
        .collect();
    sem.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// O município de um texto: "Ubaporanga", "Viçosa/MG", "Viçosa - MG", "viçosa, mg", ou o código
/// do IBGE. Um nome que se repete em várias UFs, sem a UF, é um erro que diz quais são.
pub fn buscar(texto: &str) -> Result<Municipio, String> {
    let t = texto.trim();
    if t.len() == 7 && t.chars().all(|c| c.is_ascii_digit()) {
        return por_codigo(t).ok_or_else(|| format!("nenhum município tem o código {t}"));
    }
    // A UF no fim, depois de /, -, vírgula ou espaço.
    let (nome, uf) = match t.rsplit_once(['/', '-', ',', ' ']) {
        Some((n, u)) if u.trim().len() == 2 && todos().any(|m| m.uf.eq_ignore_ascii_case(u.trim())) => (n, Some(u.trim().to_uppercase())),
        _ => (t, None),
    };
    let alvo = normal(nome.trim_end_matches(['/', '-', ',', ' ']));
    if alvo.is_empty() {
        return Err("dê o nome de um município".into());
    }
    let achados: Vec<Municipio> = todos().filter(|m| normal(m.nome) == alvo && uf.as_deref().is_none_or(|u| m.uf == u)).collect();
    match achados.as_slice() {
        [m] => Ok(*m),
        [] => {
            // Os que começam com o texto, para sugerir.
            let parecidos: Vec<String> = todos().filter(|m| normal(m.nome).starts_with(&alvo)).take(6).map(|m| m.rotulo()).collect();
            if parecidos.is_empty() {
                Err(format!("nenhum município se chama \"{t}\""))
            } else {
                Err(format!("nenhum município se chama \"{t}\"; talvez {}", parecidos.join(", ")))
            }
        }
        varios => {
            let ufs: Vec<String> = varios.iter().map(|m| m.rotulo()).collect();
            Err(format!("há {} municípios com esse nome ({}): diga a UF, como {}", varios.len(), ufs.join(", "), ufs[0]))
        }
    }
}

/// A árvore do painel: as UFs, cada uma com os seus municípios (o id de um município é o código
/// do IBGE).
pub fn arvore() -> Value {
    let mut ufs: Vec<(&str, Vec<Value>)> = Vec::new();
    for m in todos() {
        let no = json!({"id": m.codigo, "label": m.nome});
        match ufs.last_mut() {
            Some((uf, filhos)) if *uf == m.uf => filhos.push(no),
            _ => ufs.push((m.uf, vec![no])),
        }
    }
    Value::Array(
        ufs.into_iter()
            .map(|(uf, filhos)| json!({"id": format!("uf-{uf}"), "label": uf, "count": filhos.len(), "children": filhos}))
            .collect(),
    )
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn busca_pelo_nome() {
        assert_eq!(todos().count(), 5571);
        let u = buscar("Ubaporanga").unwrap();
        assert_eq!((u.codigo, u.uf), ("3170057", "MG"));
        assert!((u.lat + 19.6351).abs() < 1e-4);
        assert_eq!(buscar("viçosa/mg").unwrap().codigo, "3171303");
        assert_eq!(buscar("Vicosa - MG").unwrap().codigo, "3171303");
        assert_eq!(buscar("Viçosa, RN").unwrap().uf, "RN");
        assert_eq!(buscar("3171303").unwrap().nome, "Viçosa");
        assert_eq!(buscar("Sao Paulo/SP").unwrap().codigo, "3550308");
        let e = buscar("Viçosa").unwrap_err();
        assert!(e.contains("Viçosa/MG") && e.contains("Viçosa/AL"), "{e}");
        assert!(buscar("Ubapor").unwrap_err().contains("Ubaporanga/MG"));
        assert!(buscar("Xyzzy").is_err());
        let a = arvore();
        assert_eq!(a.as_array().unwrap().len(), 27);
        let total: usize = a.as_array().unwrap().iter().map(|u| u["children"].as_array().unwrap().len()).sum();
        assert_eq!(total, 5571);
    }
}
