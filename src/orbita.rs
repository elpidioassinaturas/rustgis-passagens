//! As passagens de um satélite sobre um lugar.
//!
//! Para cada órbita, o momento em que o satélite está mais perto do lugar é o zero de
//! `f(t) = (r_sat − r_alvo) · v_sat`, no sistema fixo na Terra, onde o lugar não se move. O método
//! de Newton com `f'(t) ≈ |v|²` chega lá em poucas iterações a partir de qualquer ponto da órbita,
//! e a aproximação seguinte fica uma órbita depois. São uns cinco cálculos do SGP4 por órbita, em
//! vez de milhares de passos de tempo: o plugin roda com um orçamento de instruções curto.

use serde_json::Value;

use crate::perfis::{Perfil, Sensor};
use crate::tempo;

/// O raio equatorial e o achatamento do WGS 84, em km.
const A: f64 = 6378.137;
const F: f64 = 1.0 / 298.257_223_563;
/// O raio médio, para distâncias no chão.
pub const R: f64 = 6371.0;
/// A rotação da Terra, em rad/s.
const OMEGA: f64 = 7.292_115e-5;
/// Quantas vezes o SGP4 rodou (para medir, fora do wasm).
#[cfg(not(target_arch = "wasm32"))]
pub static CHAMADAS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A distância no chão (km) do lugar ao traço até onde um satélite sem câmera conhecida conta: a
/// 1.500 km, um satélite a 1.200 km de altitude já está abaixo de 45° de elevação.
pub const LONGE: f64 = 1500.0;

/// Até onde (km no chão) a passagem de um satélite com `perfil` pode contar, a `altitude` km.
pub fn alcance_do_perfil(perfil: Option<&Perfil>, altitude: f64) -> f64 {
    let visao = match perfil.map(|p| p.sensor) {
        Some(Sensor::Nadir { meia_faixa }) => meia_faixa,
        Some(Sensor::Radar { longe, .. }) => longe,
        Some(Sensor::Apontavel { angulo, .. }) => alcance(altitude, angulo),
        None => 0.0,
    };
    visao.max(LONGE) + 100.0
}

type V3 = [f64; 3];

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn norma(a: V3) -> f64 {
    dot(a, a).sqrt()
}

/// Um lugar no chão.
#[derive(Clone, Copy, Debug)]
pub struct Lugar {
    pub lon: f64,
    pub lat: f64,
    /// A posição no sistema fixo na Terra, em km.
    pub ecef: V3,
    /// A vertical do lugar (normal ao elipsoide).
    pub cima: V3,
}

impl Lugar {
    pub fn new(lon: f64, lat: f64) -> Lugar {
        let (fi, la) = (lat.to_radians(), lon.to_radians());
        let e2 = F * (2.0 - F);
        let n = A / (1.0 - e2 * fi.sin().powi(2)).sqrt();
        let ecef = [n * fi.cos() * la.cos(), n * fi.cos() * la.sin(), n * (1.0 - e2) * fi.sin()];
        Lugar { lon, lat, ecef, cima: [fi.cos() * la.cos(), fi.cos() * la.sin(), fi.sin()] }
    }

    /// A elevação, em graus, de um ponto `p` (km, fixo na Terra) vista daqui.
    pub fn elevacao(&self, p: V3) -> f64 {
        let d = sub(p, self.ecef);
        (dot(d, self.cima) / norma(d)).clamp(-1.0, 1.0).asin().to_degrees()
    }
}

/// Longitude e latitude (geodésica, aproximada) do ponto abaixo de `p`.
pub fn lon_lat(p: V3) -> [f64; 2] {
    let e2 = F * (2.0 - F);
    let lon = p[1].atan2(p[0]).to_degrees();
    let lat = p[2].atan2(p[0].hypot(p[1]) * (1.0 - e2)).to_degrees();
    [lon, lat]
}

/// A distância no chão, em km, entre dois pontos em graus.
pub fn distancia(a: [f64; 2], b: [f64; 2]) -> f64 {
    let (f1, f2) = (a[1].to_radians(), b[1].to_radians());
    let (df, dl) = (f2 - f1, (b[0] - a[0]).to_radians());
    let h = (df / 2.0).sin().powi(2) + f1.cos() * f2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * R * h.sqrt().min(1.0).asin()
}

/// A direção do Sol no sistema fixo na Terra (unitária), pela fórmula de baixa precisão do
/// Astronomical Almanac (uns 0,01°).
pub fn sol(t: f64) -> V3 {
    let n = tempo::juliano(t) - 2_451_545.0;
    let l = (280.460 + 0.985_647_4 * n).to_radians();
    let g = (357.528 + 0.985_600_3 * n).to_radians();
    let lambda = l + (1.915f64.to_radians()) * g.sin() + (0.020f64.to_radians()) * (2.0 * g).sin();
    let eps = (23.439 - 0.000_000_4 * n).to_radians();
    let eci = [lambda.cos(), eps.cos() * lambda.sin(), eps.sin() * lambda.sin()];
    para_terra(eci, tempo::gmst(t))
}

/// A elevação do Sol, em graus, no lugar.
pub fn elevacao_do_sol(lugar: &Lugar, t: f64) -> f64 {
    dot(sol(t), lugar.cima).clamp(-1.0, 1.0).asin().to_degrees()
}

/// Gira um vetor inercial (TEME) para o sistema fixo na Terra.
fn para_terra(v: V3, gmst: f64) -> V3 {
    let (s, c) = gmst.sin_cos();
    [c * v[0] + s * v[1], -s * v[0] + c * v[1], v[2]]
}

/// O tempo (s) até a maior aproximação, pela posição `r` e velocidade `v` (fixas na Terra) do
/// satélite, e a distância transversal ao traço, em km no chão. É o ângulo, no plano da órbita,
/// de `r` até a projeção do lugar `a` nesse plano, sobre a velocidade angular: exato numa órbita
/// circular sobre um lugar parado, de qualquer ponto da órbita.
fn passo_angular(r: V3, v: V3, a: V3) -> (f64, f64) {
    let h = cross(r, v);
    let hn = norma(h);
    let u = [h[0] / hn, h[1] / hn, h[2] / hn];
    let fora = dot(a, u);
    let ap = sub(a, [u[0] * fora, u[1] * fora, u[2] * fora]);
    let angulo = dot(cross(r, ap), u).atan2(dot(r, ap));
    let w = hn / dot(r, r);
    (angulo / w, R * (fora / norma(a)).clamp(-1.0, 1.0).asin().abs())
}

/// Um satélite, dos elementos GP (OMM em JSON) do CelesTrak.
pub struct Satelite {
    pub nome: String,
    pub norad: u64,
    /// A época dos elementos.
    pub epoca: f64,
    /// O período, em segundos.
    pub periodo: f64,
    constantes: sgp4::Constants,
}

impl Satelite {
    pub fn de_gp(v: &Value) -> Option<Satelite> {
        let el: sgp4::Elements = serde_json::from_value(v.clone()).ok()?;
        let constantes = sgp4::Constants::from_elements(&el).ok()?;
        Some(Satelite {
            nome: el.object_name.clone().unwrap_or_else(|| el.norad_id.to_string()),
            norad: el.norad_id,
            epoca: el.datetime.and_utc().timestamp_millis() as f64 / 1000.0,
            periodo: 86_400.0 / el.mean_motion,
            constantes,
        })
    }

    /// Se é geoestacionário (ou quase): dá uma volta por dia.
    pub fn geoestacionario(&self) -> bool {
        self.periodo > 80_000.0
    }

    /// Posição (km) e velocidade (km/s) no sistema fixo na Terra.
    pub fn estado(&self, t: f64) -> Option<(V3, V3)> {
        #[cfg(not(target_arch = "wasm32"))]
        CHAMADAS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = self.constantes.propagate(sgp4::MinutesSinceEpoch((t - self.epoca) / 60.0)).ok()?;
        let g = tempo::gmst(t);
        let r = para_terra(p.position, g);
        let v = para_terra(p.velocity, g);
        // A velocidade vista da Terra, que gira: v − ω × r.
        Some((r, [v[0] + OMEGA * r[1], v[1] - OMEGA * r[0], v[2]]))
    }

    /// Os instantes de maior aproximação ao lugar entre `t0` e `t1`, uma por órbita, das órbitas
    /// cujo traço passa a menos de `longe` km do lugar.
    pub fn aproximacoes(&self, lugar: &Lugar, t0: f64, t1: f64, longe: f64) -> Vec<f64> {
        let p = self.periodo;
        // O quanto o traço pode chegar mais perto do lugar de uma órbita para a outra: a Terra
        // gira por baixo da órbita (o plano dela quase não se move num dia).
        let por_orbita = R * std::f64::consts::TAU * p / 86_164.0;
        let mut saida: Vec<f64> = Vec::new();
        let mut t = t0;
        let mut voltas = 0;
        while t < t1 + p * 0.5 && voltas < 2000 {
            voltas += 1;
            let Some((r, v)) = self.estado(t) else {
                t += p * 0.5;
                continue;
            };
            let (passo, transversal) = passo_angular(r, v, lugar.ecef);
            // Longe do mínimo (no começo): vai até ele antes de decidir, porque o lugar gira com a
            // Terra (até 12° em meia órbita) e a distância transversal muda.
            if passo.abs() > 300.0 {
                t += passo;
                continue;
            }
            // Um só cálculo por órbita que passa longe, e as órbitas seguintes que não podem
            // chegar perto também ficam de fora.
            if transversal > longe {
                let pular = 1.0 + ((transversal - longe) / por_orbita).floor();
                t += passo + pular * p;
                continue;
            }
            let mut tm = t + passo;
            for _ in 0..4 {
                let Some((r, v)) = self.estado(tm) else { break };
                let (passo, _) = passo_angular(r, v, lugar.ecef);
                tm += passo;
                if passo.abs() < 0.5 {
                    break;
                }
            }
            if tm >= t0 && tm <= t1 && saida.last().is_none_or(|u| tm - u > p * 0.25) {
                saida.push(tm);
            }
            t = tm + p;
        }
        saida
    }

    /// O traço no chão de `t − meio` a `t + meio`, em `n` pontos.
    pub fn traco(&self, t: f64, meio: f64, n: usize) -> Vec<[f64; 2]> {
        (0..n)
            .filter_map(|i| {
                let ti = t - meio + 2.0 * meio * i as f64 / (n - 1).max(1) as f64;
                self.estado(ti).map(|(r, _)| lon_lat(r))
            })
            .collect()
    }
}

/// O que a passagem significa para o lugar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Categoria {
    /// Há uma imagem pública desta passagem.
    Fotografou,
    /// A faixa fixa cobriu o lugar, com luz (ou um radar), mas não há imagem publicada.
    Provavel,
    /// Uma câmera apontável estava ao alcance.
    PoderiaApontar,
    /// Só cruzou o céu do lugar, alto (acima de [`ELEVACAO_MINIMA`]).
    PassouPorCima,
}

impl Categoria {
    pub fn texto(self) -> &'static str {
        match self {
            Categoria::Fotografou => "fotografou",
            Categoria::Provavel => "provavelmente fotografou",
            Categoria::PoderiaApontar => "poderia ter apontado",
            Categoria::PassouPorCima => "passou por cima",
        }
    }

    pub fn cor(self) -> &'static str {
        match self {
            Categoria::Fotografou => "#E8590C",
            Categoria::Provavel => "#F2A541",
            Categoria::PoderiaApontar => "#7B61FF",
            Categoria::PassouPorCima => "#8A8F9850",
        }
    }

    /// A largura da linha, em pontos.
    pub fn largura(self) -> f64 {
        match self {
            Categoria::Fotografou => 3.0,
            Categoria::Provavel => 1.6,
            Categoria::PoderiaApontar => 0.9,
            Categoria::PassouPorCima => 0.5,
        }
    }

    pub const TODAS: [Categoria; 4] = [Categoria::Fotografou, Categoria::Provavel, Categoria::PoderiaApontar, Categoria::PassouPorCima];
}

/// A elevação mínima para "passou por cima".
pub const ELEVACAO_MINIMA: f64 = 45.0;

/// Uma passagem sobre o lugar.
#[derive(Clone, Debug)]
pub struct Passagem {
    pub satelite: String,
    pub norad: u64,
    /// O instante de maior aproximação.
    pub t: f64,
    /// A distância no chão do lugar ao traço, em km.
    pub distancia: f64,
    /// A elevação do satélite vista do lugar, em graus.
    pub elevacao: f64,
    /// A elevação do Sol no lugar, em graus.
    pub sol: f64,
    /// A altitude do satélite, em km.
    pub altitude: f64,
    /// Se o lugar ficou à direita da órbita.
    pub direita: bool,
    pub categoria: Categoria,
    /// O começo do id dos itens do STAC deste satélite.
    pub stac: Option<&'static str>,
}

/// O alcance no chão, em km, de uma câmera a `altitude` km inclinada `angulo` graus.
pub fn alcance(altitude: f64, angulo: f64) -> f64 {
    let s = (R + altitude) / R * angulo.to_radians().sin();
    if s >= 1.0 {
        // Além do horizonte: até onde o satélite vê.
        return R * (R / (R + altitude)).acos();
    }
    R * (s.asin() - angulo.to_radians())
}

/// A categoria de uma passagem, ou `None` se ela não conta.
pub fn classificar(perfil: Option<&Perfil>, distancia: f64, direita: bool, elevacao: f64, sol: f64, altitude: f64) -> Option<Categoria> {
    let dia = sol > 0.0;
    let por_cima = (elevacao >= ELEVACAO_MINIMA).then_some(Categoria::PassouPorCima);
    let Some(perfil) = perfil else { return por_cima };
    let viu = match perfil.sensor {
        Sensor::Nadir { meia_faixa } => (distancia <= meia_faixa && dia).then_some(Categoria::Provavel),
        Sensor::Radar { perto, longe } => (direita && distancia >= perto && distancia <= longe).then_some(Categoria::Provavel),
        Sensor::Apontavel { angulo, radar } => {
            (distancia <= alcance(altitude, angulo) && (dia || radar)).then_some(Categoria::PoderiaApontar)
        }
    };
    viu.or(por_cima)
}

/// As passagens de `sat` sobre `lugar` entre `t0` e `t1`.
pub fn passagens(sat: &Satelite, perfil: Option<&Perfil>, lugar: &Lugar, t0: f64, t1: f64) -> Vec<Passagem> {
    let alvo = [lugar.lon, lugar.lat];
    let altitude = sat.estado(t0).map(|(r, _)| norma(r) - R).unwrap_or(700.0);
    sat.aproximacoes(lugar, t0, t1, alcance_do_perfil(perfil, altitude))
        .into_iter()
        .filter_map(|t| {
            let (r, v) = sat.estado(t)?;
            let chao = lon_lat(r);
            let distancia = distancia(alvo, chao);
            let elevacao = lugar.elevacao(r);
            let sol = elevacao_do_sol(lugar, t);
            let altitude = norma(r) - R;
            // À direita: do lado de v × r (com r para cima e v para a frente).
            let direita = dot(cross(v, r), sub(lugar.ecef, r)) > 0.0;
            let categoria = classificar(perfil, distancia, direita, elevacao, sol, altitude)?;
            Some(Passagem {
                satelite: sat.nome.clone(),
                norad: sat.norad,
                t,
                distancia,
                elevacao,
                sol,
                altitude,
                direita,
                categoria,
                stac: perfil.and_then(|p| p.stac),
            })
        })
        .collect()
}

#[cfg(test)]
pub mod testes {
    use super::*;
    use serde_json::json;

    /// O Sentinel-2B do CelesTrak de 15/11/2026.
    pub fn s2b() -> Satelite {
        Satelite::de_gp(&json!({"OBJECT_NAME":"SENTINEL-2B","OBJECT_ID":"2017-013A","EPOCH":"2026-11-15T19:34:54.704352","MEAN_MOTION":14.30813686,"ECCENTRICITY":0.0001114,"INCLINATION":98.5709,"RA_OF_ASC_NODE":32.4267,"ARG_OF_PERICENTER":90.3469,"MEAN_ANOMALY":269.7841,"EPHEMERIS_TYPE":0,"CLASSIFICATION_TYPE":"U","NORAD_CAT_ID":42063,"ELEMENT_SET_NO":999,"REV_AT_EPOCH":50632,"BSTAR":9.0686e-05,"MEAN_MOTION_DOT":1.94e-06,"MEAN_MOTION_DDOT":0}))
            .unwrap()
    }

    #[test]
    fn uma_aproximacao_por_orbita() {
        let sat = s2b();
        let lugar = Lugar::new(-42.887, -20.695);
        let t0 = tempo::ler("2026-11-08T00:00:00Z").unwrap();
        let t1 = t0 + 7.0 * 86_400.0;
        let a = sat.aproximacoes(&lugar, t0, t1, 2100.0);
        // Confere com uma varredura de 10 em 10 s: cada mínimo local da distância a menos de
        // 2.000 km tem de estar na lista.
        let dist = |t: f64| norma(sub(sat.estado(t).unwrap().0, lugar.ecef));
        let mut minimos = Vec::new();
        let mut t = t0 + 10.0;
        while t < t1 - 10.0 {
            if dist(t) < 2000.0 && dist(t) < dist(t - 10.0) && dist(t) <= dist(t + 10.0) {
                minimos.push(t);
            }
            t += 10.0;
        }
        for m in &minimos {
            assert!(a.iter().any(|x| (x - m).abs() < 12.0), "faltou {}", tempo::iso(*m));
        }
        // As que a busca achou e a varredura não ficam a mais de 2.000 km.
        for x in &a {
            assert!(minimos.iter().any(|m| (x - m).abs() < 12.0) || dist(*x) > 2000.0, "sobrou {}", tempo::iso(*x));
        }
        assert!(minimos.len() >= 10, "{}", minimos.len());
    }

    #[test]
    fn o_sentinel_2b_viu_vicosa_em_13_de_novembro() {
        // O item S2B_MSIL2A_20261113T130249_R095_T23KQT cobre Viçosa: a passagem tem de cair
        // perto das 13:03 UTC, dentro da meia-faixa, de dia.
        let sat = s2b();
        let lugar = Lugar::new(-42.887, -20.695);
        let t0 = tempo::ler("2026-11-13T00:00:00Z").unwrap();
        let p = passagens(&sat, crate::perfis::perfil("SENTINEL-2B").as_ref(), &lugar, t0, t0 + 86_400.0);
        let vista = p.iter().find(|p| p.categoria == Categoria::Provavel).expect("uma passagem com a faixa");
        let item = tempo::ler("2026-11-13T13:02:49Z").unwrap();
        assert!((vista.t - item).abs() < 15.0 * 60.0, "{}", tempo::iso(vista.t));
        assert!(vista.distancia < 145.0 && vista.sol > 30.0, "{vista:?}");
        assert!((780.0..810.0).contains(&vista.altitude), "{}", vista.altitude);
    }

    #[test]
    fn sol_ao_meio_dia() {
        // Viçosa, 15/11, perto do meio-dia solar (≈ 14:55 UTC): o Sol quase a pino.
        let lugar = Lugar::new(-42.887, -20.695);
        let e = elevacao_do_sol(&lugar, tempo::ler("2026-11-15T14:55:00Z").unwrap());
        assert!(e > 80.0, "{e}");
        let noite = elevacao_do_sol(&lugar, tempo::ler("2026-11-15T03:00:00Z").unwrap());
        assert!(noite < -30.0, "{noite}");
    }

    #[test]
    fn alcance_de_uma_camera_inclinada() {
        // 617 km de altitude, 45°: um pouco mais que h·tan(45°) por causa da curvatura.
        let a = alcance(617.0, 45.0);
        assert!((650.0..720.0).contains(&a), "{a}");
        assert_eq!(alcance(500.0, 0.0), 0.0);
    }

    #[test]
    fn classificacao() {
        let s2 = crate::perfis::perfil("SENTINEL-2A");
        assert_eq!(classificar(s2.as_ref(), 100.0, true, 70.0, 40.0, 786.0), Some(Categoria::Provavel));
        // De noite, a câmera não conta; alto no céu, passou por cima.
        assert_eq!(classificar(s2.as_ref(), 100.0, true, 70.0, -20.0, 786.0), Some(Categoria::PassouPorCima));
        assert_eq!(classificar(s2.as_ref(), 2000.0, true, 5.0, 40.0, 786.0), None);
        let s1 = crate::perfis::perfil("SENTINEL-1C");
        assert_eq!(classificar(s1.as_ref(), 300.0, true, 50.0, -20.0, 693.0), Some(Categoria::Provavel));
        assert_eq!(classificar(s1.as_ref(), 300.0, false, 50.0, -20.0, 693.0), Some(Categoria::PassouPorCima));
        let wv = crate::perfis::perfil("WORLDVIEW-3 (WV-3)");
        assert_eq!(classificar(wv.as_ref(), 500.0, false, 40.0, 30.0, 617.0), Some(Categoria::PoderiaApontar));
        assert_eq!(classificar(None, 100.0, true, 60.0, 30.0, 420.0), Some(Categoria::PassouPorCima));
    }
}
