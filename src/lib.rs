//! Passagens: quais satélites passaram sobre um lugar nos últimos dias, e as imagens públicas
//! que eles fizeram dele.
//!
//! 1. Os elementos orbitais vêm do CelesTrak (grupos `resource` e `weather`, e alguns satélites
//!    por nome), e o SGP4 acha a maior aproximação de cada satélite ao lugar em cada órbita
//!    ([`orbita`]).
//! 2. Cada passagem é classificada pelo que o satélite consegue ver ([`perfis`]) e pela luz.
//! 3. O `stac.search` do RustGIS procura no Planetary Computer as imagens do Sentinel-2, do
//!    Landsat e do Sentinel-1 do lugar no período, e cada uma é ligada à passagem que a fez
//!    ([`imagens`]).
//!
//! Inspirado no watched.earth.

pub mod cache;
pub mod calculo;
pub mod galeria;
pub mod gratuitos;
pub mod imagens;
pub mod orbita;
pub mod perfis;
pub mod proximas;
pub mod tempo;

// O que fala com o RustGIS só compila no wasm: fora dele (nos testes), as importações do
// RustGIS ficariam sem resolver no linker do Windows.
#[cfg(target_arch = "wasm32")]
mod comandos;
