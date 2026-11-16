#!/usr/bin/env python3
"""Baixa as órbitas do CelesTrak e grava o arquivo que o plugin lê do GitHub.

    python scripts/orbitas.py saida/orbitas.json

São as mesmas consultas que o plugin faria (comandos.rs, CELESTRAK), no formato compacto do
cache (cache.rs): {"v": 1, "t": hora, "s": [[nome, época, NORAD, MEAN_MOTION, ...], ...]}.
O workflow orbitas.yml roda isto uma vez por dia, dentro do que o CelesTrak permite.
"""

from __future__ import annotations

import json
import sys
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

CELESTRAK = ["GROUP=resource", "GROUP=weather", "NAME=SENTINEL-1", "NAME=CBERS", "NAME=AMAZONIA", "NAME=ICEYE", "NAME=CAPELLA"]
CAMPOS = [
    "NORAD_CAT_ID",
    "MEAN_MOTION",
    "ECCENTRICITY",
    "INCLINATION",
    "RA_OF_ASC_NODE",
    "ARG_OF_PERICENTER",
    "MEAN_ANOMALY",
    "BSTAR",
    "MEAN_MOTION_DOT",
    "MEAN_MOTION_DDOT",
]
# Mais que isso não cabe nas configurações do plugin (cache.rs, MAXIMO).
MAXIMO = 63_000


def epoca(texto: str) -> float:
    """Segundos desde 1970 de uma época do OMM (UTC, sem fuso)."""
    return datetime.fromisoformat(texto.rstrip("Z")).replace(tzinfo=timezone.utc).timestamp()


def baixar(consulta: str) -> list[dict]:
    url = f"https://celestrak.org/NORAD/elements/gp.php?{consulta}&FORMAT=json"
    pedido = urllib.request.Request(url, headers={"User-Agent": "rustgis-plugin"})
    with urllib.request.urlopen(pedido, timeout=60) as r:
        return json.load(r)


def main() -> int:
    agora = time.time()
    linhas, vistos = [], set()
    for consulta in CELESTRAK:
        elementos = baixar(consulta)
        print(f"{consulta}: {len(elementos)}")
        for e in elementos:
            norad = e.get("NORAD_CAT_ID")
            if norad in vistos or agora - epoca(e["EPOCH"]) > 30 * 86_400:
                continue
            vistos.add(norad)
            linhas.append([e["OBJECT_NAME"], round(epoca(e["EPOCH"]), 3)] + [e.get(c) for c in CAMPOS])
    if len(linhas) < 100:
        print(f"erro: só {len(linhas)} satélites", file=sys.stderr)
        return 1
    texto = json.dumps({"v": 1, "t": round(agora), "s": linhas}, ensure_ascii=False, separators=(",", ":"))
    if len(texto) > MAXIMO:
        print(f"erro: {len(texto)} bytes, mais que {MAXIMO}", file=sys.stderr)
        return 1
    saida = Path(sys.argv[1])
    saida.parent.mkdir(parents=True, exist_ok=True)
    saida.write_text(texto, encoding="utf-8")
    print(f"{len(linhas)} satélites, {len(texto)} bytes em {saida}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
