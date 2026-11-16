#!/usr/bin/env python3
"""Gera src/municipios.csv, a lista dos municípios que o plugin leva dentro de si.

    python scripts/municipios.py

A fonte é o projeto kelvins/municipios-brasileiros (licença MIT): o código do IBGE, o nome e as
coordenadas da sede de cada município. O arquivo gerado tem uma linha por município,
`código;nome;UF;latitude;longitude`, em ordem de UF e de nome.
"""

from __future__ import annotations

import csv
import io
import urllib.request
from pathlib import Path

FONTE = "https://raw.githubusercontent.com/kelvins/municipios-brasileiros/main/csv/municipios.csv"
SAIDA = Path(__file__).resolve().parent.parent / "src" / "municipios.csv"

# O código do IBGE de cada UF.
UF = {
    11: "RO", 12: "AC", 13: "AM", 14: "RR", 15: "PA", 16: "AP", 17: "TO",
    21: "MA", 22: "PI", 23: "CE", 24: "RN", 25: "PB", 26: "PE", 27: "AL", 28: "SE", 29: "BA",
    31: "MG", 32: "ES", 33: "RJ", 35: "SP",
    41: "PR", 42: "SC", 43: "RS",
    50: "MS", 51: "MT", 52: "GO", 53: "DF",
}


def main() -> None:
    with urllib.request.urlopen(FONTE, timeout=60) as r:  # noqa: S310 - um endereço https fixo
        texto = r.read().decode("utf-8")
    linhas = []
    for m in csv.DictReader(io.StringIO(texto)):
        uf = UF[int(m["codigo_uf"])]
        linhas.append((uf, m["nome"], m["codigo_ibge"], float(m["latitude"]), float(m["longitude"])))
    linhas.sort(key=lambda l: (l[0], l[1]))
    corpo = "".join(f"{c};{n};{uf};{lat:.4f};{lon:.4f}\n" for uf, n, c, lat, lon in linhas)
    SAIDA.write_text(corpo, encoding="utf-8", newline="\n")
    print(f"{len(linhas)} municípios, {len(corpo.encode())} bytes em {SAIDA}")


if __name__ == "__main__":
    main()
