# Passagens de satélites (plugin do RustGIS)

Quais satélites passaram sobre um lugar nos últimos dias, e quais imagens públicas eles fizeram
dele. As imagens do Sentinel-2 do lugar na barra de tempo, só as de céu limpo ali. E quando os
satélites de imagem gratuita passam de novo, com a nuvem prevista para a hora de cada passagem.
Inspirado no [watched.earth](https://www.watched.earth).

## Uso: o painel

Na aba **Satélites**, o botão **Satélites sobre um lugar** (grupo **Lugar**) abre o painel:

1. O lugar: o **Município**, digitando parte do nome na busca da lista (as UFs, cada uma com os
   seus municípios) e escolhendo-o; ou **Ou clicar no mapa** e um clique no mapa. O painel mostra
   o nome e as coordenadas, e o lugar fica guardado.
2. Um dos botões, que rodam para esse lugar sem perguntar nada:
   - **Imagens limpas:** as imagens de céu limpo do Sentinel-2 dos últimos 60 dias na barra de
     tempo;
   - **Próximas:** as próximas passagens dos próximos 10 dias, com a nuvem prevista;
   - **Recentes:** as passagens dos últimos 7 dias e as imagens que elas fizeram.
3. O resultado aparece no painel, e as camadas no mapa.

Os botões do painel e os comandos que só o painel usa (escolher o lugar, o estado do painel) e as
ferramentas de cálculo não ficam na aba; a busca de comandos acha todos eles.

Os botões **Passagens de satélites**, **Imagens do lugar** e **Próximas imagens** do grupo **Com
mais opções** são os mesmos comandos com um formulário (município, dias, tamanho do recorte). O **Município** é o nome
(`Ubaporanga`, ou `Viçosa/MG` quando o nome se repete) ou o código do IBGE; vazio, vale o último
lugar escolhido no painel, ou o centro do mapa. O município de um formulário vale só para ele: o
lugar dos botões do painel muda só no painel. Os municípios (com a sede de cada um) vão dentro do
plugin: `src/municipios.csv`, gerado por `scripts/municipios.py` a partir do
[kelvins/municipios-brasileiros](https://github.com/kelvins/municipios-brasileiros) (MIT).

## Comandos

- **Passagens de satélites:** as passagens dos satélites de imagem gratuita que viram o lugar nos
  últimos dias, cada uma como o traço no chão: fotografou (a imagem está no Planetary Computer) ou
  provavelmente fotografou (o relatório diz em que catálogo procurar). Traz para o mapa a imagem
  óptica mais recente com pouca nuvem.
- **Imagens do lugar:** as imagens do Sentinel-2 do lugar na barra de tempo do RustGIS, um
  recorte de 5 km em volta dele em 10 m, só as datas com o céu limpo no próprio lugar. O céu de
  cada data é a classe da banda SCL (a classificação da cena do Sentinel-2 L2A) no pixel do lugar,
  que o Planetary Computer dá num ponto, e não a nuvem da cena inteira (110 km): uma cena com 93%
  de nuvem pode estar limpa sobre o lugar, e uma com 21% coberta ali. Cada data limpa entra como
  um cubo de dados com uma imagem só, do [Earth Search](https://earth-search.aws.element84.com/v1)
  (os arquivos do Planetary Computer têm endereços assinados que expiram, e o cubo do RustGIS não
  os lê). O ▶ da barra passa as datas, e `time.export` faz um GIF. Só no app desktop e no
  `rustgis-cli`, que fazem cubos.
- **Próximas imagens:** as próximas passagens dos satélites de imagem gratuita (abaixo), cada uma
  com a nuvem prevista pelo [Open-Meteo](https://open-meteo.com) para a hora dela, até 16 dias à
  frente. Para planejar o campo no dia da imagem.
- **Imagem da passagem:** com uma passagem *fotografou* selecionada, põe a imagem dela no mapa.

```sh
rustgis-cli run --sample --yes --cmd 'plugin:passagens/imagens={"lat":-20.755,"lon":-42.879,"dias":90}' --cmd 'time.export={"path":"vicosa.gif"}'
rustgis-cli run --sample --yes --cmd 'plugin:passagens/proximas={"lat":-20.695,"lon":-42.887}'
rustgis-cli run --sample --yes --cmd 'plugin:passagens/passagens={"lat":-20.695,"lon":-42.887}'
```

Parâmetros: o lugar é o centro do mapa, ou `lat` e `lon`; `nome` dá o nome das camadas (padrão:
o do Photon). Passagens: `dias` de 1 a 14 (padrão 7) e `imagem` (padrão `true`). Imagens do
lugar: `dias` de 5 a 365 (padrão 60; lê as 100 cenas mais recentes, e o céu é conferido nas 48
datas mais novas, pelo limite de consultas de um comando), `lado` do recorte em km (padrão 5) e `quantas` datas limpas na barra (padrão
6, até 10; cada uma leva uns 8 s). Próximas
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
3. **Só o que vira imagem para baixar.** Só os satélites de imagem gratuita (a tabela acima), e
   só as passagens em que a faixa da câmera cobriu o lugar de dia (ou a do radar do Sentinel-1, à
   direita da órbita), pelo que cada um vê (`src/perfis.rs`):
   - **fotografou:** a imagem está no Planetary Computer (o item do STAC é do mesmo satélite, a
     menos de 40 min);
   - **provavelmente fotografou:** a imagem não está lá; o relatório diz em que catálogo procurar
     (INPE, NASA, Copernicus, USGS).

   As próximas imagens são as mesmas passagens no futuro, coloridas pela nuvem prevista: céu limpo
   (até 30%), parcialmente nublado, nublado (70% ou mais), radar (a nuvem não importa) e sem
   previsão.
4. **Imagens.** O `stac.search` do RustGIS procura Sentinel-2 L2A, Landsat C2 L2 e Sentinel-1 GRD
   no Microsoft Planetary Computer, num retângulo de ~100 m em volta do lugar. A imagem óptica
   mais recente com até 30% de nuvem na cena inteira vai para o mapa. Se não houver, vai a última
   cena limpa do Sentinel-2 (até 10% de nuvem, 120 dias). O lugar pode estar limpo numa cena
   nublada: **Imagens do lugar** confere o pixel. A imagem do Sentinel-2 vem do mesmo item no Earth
   Search, de endereço público (os do Planetary Computer expiram), numa camada com o nome do lugar.
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
- O painel escolhe o lugar com um clique no mapa (o `pick` do RustGIS 0.7.0). As checagens do
  registro de plugins ainda não conhecem o `pick`; `scripts/package.py` o acrescenta a elas.
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
