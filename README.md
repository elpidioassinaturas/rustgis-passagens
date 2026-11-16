# Passagens de satélites (plugin do RustGIS)

Quais satélites passaram sobre um lugar nos últimos dias, e quais imagens públicas eles fizeram
dele. Inspirado no [watched.earth](https://www.watched.earth).

## Como funciona

1. **Órbitas.** Os elementos GP do [CelesTrak](https://celestrak.org) (grupos `resource` e
   `weather`, e Sentinel-1, CBERS, Amazonia-1, ICEYE e Capella por nome), cerca de 330 satélites,
   ficam guardados nas configurações do plugin por 6 horas. O CelesTrak bloqueia quem baixa os
   mesmos dados muitas vezes.
2. **Passagens.** O SGP4 (crate `sgp4`) acha, em cada órbita, o instante de maior aproximação ao
   lugar, por um passo angular no plano da órbita. As órbitas que passam longe custam uma
   chamada só, e as seguintes que não podem chegar perto são puladas.
3. **Categorias**, pelo que cada satélite vê (`src/perfis.rs`) e pela luz do Sol:
   - **fotografou:** há imagem pública da passagem (o item do STAC é do mesmo satélite, a menos de
     40 min);
   - **provavelmente fotografou:** a faixa fixa da câmera cobriu o lugar, de dia (ou o radar do
     Sentinel-1, à direita da órbita), sem imagem publicada;
   - **poderia ter apontado:** uma câmera ou um radar apontável estava ao alcance;
   - **passou por cima:** cruzou o céu a mais de 45° do horizonte.
4. **Imagens.** O `stac.search` do RustGIS procura Sentinel-2 L2A, Landsat C2 L2 e Sentinel-1 GRD
   no Microsoft Planetary Computer, num retângulo de ~100 m em volta do lugar. A imagem óptica
   mais recente com até 30% de nuvem vai para o mapa. Se não houver, vai a última vista limpa do
   Sentinel-2 (até 10% de nuvem, 120 dias).
5. **Hora e lugar.** O plugin não tem relógio: a hora vem do Open-Meteo, com o fuso do lugar, e o
   nome do lugar vem da busca reversa do Photon (OpenStreetMap).

O comando `passagens` busca tudo isso e passa para a ferramenta `calcular` pelo `gp.run`. O SGP4
de centenas de satélites não cabe no orçamento de instruções de um comando (500 milhões), mas
cabe no de uma ferramenta (30 bilhões).

## Uso

- **Passagens de satélites:** o centro do mapa, ou `lat` e `lon`; `dias` (1 a 14, padrão 7);
  `nome`; `imagem` (padrão `true`). O resultado são a camada *Passagens: lugar*, com uma linha por
  passagem (o traço no chão, ±2,5 min) colorida pela categoria, e a camada *Passagens: lugar
  (lugar)*, cujo ponto leva o relatório.
- **Imagem da passagem:** com uma passagem *fotografou* selecionada, põe a imagem dela no mapa.

```sh
rustgis-cli run --sample --yes --cmd 'plugin:passagens/passagens={"lat":-20.695,"lon":-42.887}'
```

Permissões: `network` (CelesTrak, Open-Meteo, Photon, STAC), `edit` (as camadas) e `files`
(o `gp.run` da ferramenta do próprio plugin).

## Limites

- Satélites militares e de reconhecimento não estão nos catálogos públicos, e as empresas não
  dizem para onde apontam as câmeras. *Poderia ter apontado* é só alcance.
- As imagens chegam aos arquivos horas ou dias depois. O Landsat L2 atrasa mais.
- No app web, o CelesTrak não aceita pedidos de páginas (não manda CORS), então o plugin roda no
  desktop e no `rustgis-cli`.

## Desenvolvimento

```sh
cargo test                                   # tempo, órbitas, perfis, casamento, cache
cargo test --release --test medida -- --ignored --nocapture   # custo, com $TEMP/g_*.json do CelesTrak
py -3.12 scripts/package.py                  # compila o wasm, valida e empacota em dist/
```
