# Passagens de satélites (plugin do RustGIS)

Quais satélites passaram sobre um lugar nos últimos dias, e quais imagens públicas eles fizeram
dele. E quando os satélites de imagem gratuita passam de novo, com a nuvem prevista para a hora de
cada passagem. Inspirado no [watched.earth](https://www.watched.earth).

## Comandos

- **Passagens de satélites:** os ~330 satélites de observação e meteorológicos que passaram sobre
  o lugar nos últimos dias, cada passagem como o traço no chão, colorida pelo que o satélite pode
  ter feito, e a imagem óptica mais recente com pouca nuvem.
- **Próximas imagens:** as próximas passagens dos satélites de imagem gratuita (abaixo), cada uma
  com a nuvem prevista pelo [Open-Meteo](https://open-meteo.com) para a hora dela, até 16 dias à
  frente. Para planejar o campo no dia da imagem.
- **Imagem da passagem:** com uma passagem *fotografou* selecionada, põe a imagem dela no mapa.

```sh
rustgis-cli run --sample --yes --cmd 'plugin:passagens/proximas={"lat":-20.695,"lon":-42.887}'
rustgis-cli run --sample --yes --cmd 'plugin:passagens/passagens={"lat":-20.695,"lon":-42.887}'
```

Parâmetros: o lugar é o centro do mapa, ou `lat` e `lon`; `nome` dá o nome das camadas (padrão:
o do Photon). Passagens: `dias` de 1 a 14 (padrão 7) e `imagem` (padrão `true`). Próximas
imagens: `dias` de 1 a 16 (padrão 10) e `diarios` (padrão `false`), que inclui os de resolução
grossa.

### Satélites de imagem gratuita

| Satélite | Instrumento | Onde baixar | Vira imagem? |
|---|---|---|---|
| Sentinel-2A, 2B, 2C | MSI, 10 m | Copernicus Browser, Planetary Computer | toda passagem de dia |
| Landsat 8 e 9 | OLI e TIRS, 30 m | USGS EarthExplorer, Planetary Computer | toda passagem de dia |
| Sentinel-1A, 1C, 1D | radar C, 10 m | Copernicus Browser, Planetary Computer | conforme o plano da ESA |
| CBERS 4, CBERS 4A, Amazonia-1 | 2 m a 64 m | catálogo do INPE | conforme a programação do INPE |
| Sentinel-3A, 3B (`diarios`) | OLCI, 300 m | Copernicus Browser | toda passagem de dia |
| Terra, Aqua (`diarios`) | MODIS, 250 m a 1 km | NASA Earthdata, Worldview | toda passagem de dia |
| Suomi NPP, NOAA 20, NOAA 21 (`diarios`) | VIIRS, 375 e 750 m | NASA Earthdata, Worldview | toda passagem de dia |

O Sentinel-1B, que o CelesTrak ainda rastreia, falhou em 2021 e fica de fora (`src/gratuitos.rs`).

## Como funciona

1. **Órbitas.** Os elementos GP do [CelesTrak](https://celestrak.org) (grupos `resource` e
   `weather`, e Sentinel-1, CBERS, Amazonia-1, ICEYE e Capella por nome). O workflow
   [Órbitas](.github/workflows/orbitas.yml) baixa tudo uma vez por dia e publica um arquivo
   compacto (~37 KB) no branch [`orbitas`](../../tree/orbitas). O plugin lê esse arquivo e o
   guarda nas configurações por 12 horas. O CelesTrak fica de reserva: ele bloqueia quem baixa os
   mesmos dados muitas vezes, e não deixa páginas web lerem os dados. Se nada responder, órbitas
   guardadas de até 30 dias ainda servem, com um aviso: o erro do SGP4 cresce alguns km por dia.
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

   As próximas imagens são as passagens futuras da segunda categoria, só dos satélites gratuitos,
   coloridas pela nuvem prevista: céu limpo (até 30%), parcialmente nublado, nublado (70% ou mais),
   radar (a nuvem não importa) e sem previsão.
4. **Imagens.** O `stac.search` do RustGIS procura Sentinel-2 L2A, Landsat C2 L2 e Sentinel-1 GRD
   no Microsoft Planetary Computer, num retângulo de ~100 m em volta do lugar. A imagem óptica
   mais recente com até 30% de nuvem vai para o mapa. Se não houver, vai a última vista limpa do
   Sentinel-2 (até 10% de nuvem, 120 dias).
5. **Hora e lugar.** O plugin não tem relógio: a hora vem do Open-Meteo, com o fuso do lugar, e o
   nome do lugar vem da busca reversa do Photon (OpenStreetMap).

Os comandos buscam tudo isso e passam para as ferramentas `calcular` e `proximas` pelo `gp.run`.
O SGP4 de centenas de satélites não cabe no orçamento de instruções de um comando (500 milhões),
mas cabe no de uma ferramenta (30 bilhões).

Permissões: `network` (GitHub, CelesTrak, Open-Meteo, Photon, STAC), `edit` (as camadas) e
`files` (o `gp.run` das ferramentas do próprio plugin).

## Limites

- Passar não garante imagem: o Sentinel-1, os CBERS e o Amazonia-1 gravam o que a programação da
  missão manda. A nuvem prevista para mais de 5 a 7 dias é pouco confiável.
- Satélites militares e de reconhecimento não estão nos catálogos públicos, e as empresas não
  dizem para onde apontam as câmeras. *Poderia ter apontado* é só alcance.
- As imagens chegam aos arquivos horas ou dias depois. O Landsat L2 atrasa mais.
- O GitHub desliga os workflows agendados de repositórios sem atividade por 60 dias. Se o arquivo
  de órbitas passar de 3 dias, o plugin volta a perguntar ao CelesTrak.

## Desenvolvimento

```sh
cargo test                                   # tempo, órbitas, perfis, casamento, cache, previsão
cargo test --release --test medida -- --ignored --nocapture   # custo, com $TEMP/g_*.json do CelesTrak
py -3.12 scripts/package.py                  # compila o wasm, valida e empacota em dist/
py -3.12 scripts/orbitas.py saida/orbitas.json   # o arquivo de órbitas (o workflow faz isso)
gh workflow run demonstracao.yml -f lat=-20.695 -f lon=-42.887   # roda tudo no GitHub e guarda os mapas
```
