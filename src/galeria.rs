//! As imagens do Sentinel-2 de um lugar na barra de tempo: o céu no próprio lugar, em cada data,
//! pela classificação da cena (a banda SCL) no pixel do lugar.
//!
//! A nuvem que o catálogo dá é a da cena inteira (110 km): uma cena com 93% de nuvem pode estar
//! limpa sobre o lugar, e uma com 5% coberta ali.

use crate::imagens::Imagem;
use crate::tempo;

/// O céu no pixel do lugar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ceu {
    Limpo,
    Nuvem,
    Cirro,
    Sombra,
    SemDado,
    /// Não conferido (o limite de consultas acabou).
    NaoConferido,
}

impl Ceu {
    pub fn texto(self) -> &'static str {
        match self {
            Ceu::Limpo => "limpo",
            Ceu::Nuvem => "nuvem",
            Ceu::Cirro => "cirro",
            Ceu::Sombra => "sombra de nuvem",
            Ceu::SemDado => "sem dado",
            Ceu::NaoConferido => "não conferido",
        }
    }
}

/// O céu e o nome da classe de um valor da banda SCL do Sentinel-2 L2A.
pub fn classe_scl(v: f64) -> (Ceu, &'static str) {
    match v.round() as i64 {
        2 => (Ceu::Limpo, "área escura ou sombra do relevo"),
        3 => (Ceu::Sombra, "sombra de nuvem"),
        4 => (Ceu::Limpo, "vegetação"),
        5 => (Ceu::Limpo, "solo ou área construída"),
        6 => (Ceu::Limpo, "água"),
        7 => (Ceu::Limpo, "não classificado"),
        8 => (Ceu::Nuvem, "nuvem, probabilidade média"),
        9 => (Ceu::Nuvem, "nuvem, probabilidade alta"),
        10 => (Ceu::Cirro, "nuvem alta e fina"),
        11 => (Ceu::Limpo, "neve ou gelo"),
        _ => (Ceu::SemDado, "sem dado"),
    }
}

/// O céu de uma cena sem consultar o pixel, quando a nuvem da cena já decide: quase toda coberta
/// ou quase toda limpa.
pub fn pela_cena(nuvem: Option<f64>) -> Option<(Ceu, &'static str)> {
    match nuvem? {
        n if n >= 99.0 => Some((Ceu::Nuvem, "a cena inteira está coberta")),
        n if n <= 1.0 => Some((Ceu::Limpo, "a cena inteira está limpa")),
        _ => None,
    }
}

/// Uma imagem por dia (as cenas vizinhas da mesma passagem têm a mesma data), as mais novas
/// primeiro.
pub fn uma_por_dia(mut imgs: Vec<Imagem>) -> Vec<Imagem> {
    imgs.sort_by(|a, b| b.t.total_cmp(&a.t));
    let mut dias: Vec<String> = Vec::new();
    imgs.retain(|i| {
        let d = tempo::dia(i.t);
        if dias.contains(&d) {
            return false;
        }
        dias.push(d);
        true
    });
    imgs
}

/// Uma data conferida.
pub struct Data {
    pub imagem: Imagem,
    pub ceu: Ceu,
    pub classe: &'static str,
}

/// O relatório: a linha do tempo, da mais nova à mais velha.
pub fn relatorio(nome: &str, dias: f64, datas: &[Data], fuso: f64, satelite: impl Fn(&str) -> String) -> String {
    let limpas = datas.iter().filter(|d| d.ceu == Ceu::Limpo).count();
    let mut t = format!(
        "{} imagens do Sentinel-2 de {nome} nos últimos {dias:.0} dias; {limpas} com o céu limpo no lugar (conferido no pixel do lugar, não na cena inteira).\n\n",
        datas.len()
    );
    for d in datas {
        let cena = d.imagem.nuvem.map(|n| format!(" · cena com {n:.0}% de nuvem")).unwrap_or_default();
        t += &format!("- {} · {} · {} ({}){cena}\n", tempo::curto(d.imagem.t, fuso), satelite(&d.imagem.id), d.ceu.texto(), d.classe);
    }
    t
}

#[cfg(test)]
mod testes {
    use super::*;

    fn img(id: &str, quando: &str, nuvem: f64) -> Imagem {
        Imagem { id: id.into(), colecao: "sentinel-2-l2a".into(), t: tempo::ler(quando).unwrap(), nuvem: Some(nuvem), url: String::new() }
    }

    #[test]
    fn classes_e_dias() {
        assert_eq!(classe_scl(4.0).0, Ceu::Limpo);
        assert_eq!(classe_scl(10.0).0, Ceu::Cirro);
        assert_eq!(classe_scl(9.0).0, Ceu::Nuvem);
        assert_eq!(classe_scl(3.0).0, Ceu::Sombra);
        assert_eq!(classe_scl(0.0).0, Ceu::SemDado);
        assert_eq!(pela_cena(Some(99.99)).unwrap().0, Ceu::Nuvem);
        assert_eq!(pela_cena(Some(0.5)).unwrap().0, Ceu::Limpo);
        assert!(pela_cena(Some(93.0)).is_none());
        assert!(pela_cena(None).is_none());
        let v = uma_por_dia(vec![
            img("S2C_A", "2026-10-09T13:02:51Z", 2.0),
            img("S2B_A", "2026-11-13T13:02:49Z", 93.0),
            img("S2C_B", "2026-10-09T13:02:55Z", 3.0),
        ]);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, "S2B_A");
        let datas = vec![Data { imagem: v[1].clone(), ceu: Ceu::Limpo, classe: "vegetação" }];
        let r = relatorio("Viçosa", 60.0, &datas, -3.0 * 3600.0, |id| id.to_string());
        assert!(r.contains("1 com o céu limpo"), "{r}");
        assert!(r.contains("sex 09/10 10:02"), "{r}");
    }
}
