"""
Acquisition USB série — Chambre à brouillard (Pico W)
======================================================
Usage :
    python acquisition.py              # détection automatique du port
    python acquisition.py COM3         # port explicite
    python acquisition.py COM3 115200  # port + baudrate

Sortie :
    - Affichage en temps réel dans le terminal
    - Fichier CSV horodaté dans data/  (créé automatiquement)

Dépendances :
    pip install pyserial
"""

import sys
import os
import re
import csv
import serial
import serial.tools.list_ports
from datetime import datetime


# ── Détection automatique du Pico W ──────────────────────────────────────────

def find_pico_port() -> str | None:
    """Retourne le port COM du premier Pico W détecté (VID=0x2E8A)."""
    for p in serial.tools.list_ports.comports():
        if p.vid == 0x2E8A:
            return p.device
    return None


# ── Parsing des lignes ────────────────────────────────────────────────────────

RE_DS   = re.compile(r"DS18B20:\s*([-\d.]+|--)\s*(?:C)?")
RE_TEMP = re.compile(r"BME280:\s*([-\d.]+)\s*C")
RE_PRES = re.compile(r"([\d.]+)\s*hPa")
RE_HUMI = re.compile(r"([\d.]+)\s*%")
RE_SKIP = re.compile(r"BME280:\s*\(skip")

def parse_line(line: str) -> dict | None:
    """Extrait les valeurs d'une ligne de mesure. Retourne None si non reconnue."""
    ds_m = RE_DS.search(line)
    if not ds_m:
        return None

    ds_val = ds_m.group(1)
    row = {"ds_temp_c": float(ds_val) if ds_val != "--" else None}

    if RE_SKIP.search(line):
        row["bme_temp_c"]    = None
        row["bme_pres_hpa"]  = None
        row["bme_humi_pct"]  = None
    else:
        t_m = RE_TEMP.search(line)
        p_m = RE_PRES.search(line)
        h_m = RE_HUMI.search(line)
        row["bme_temp_c"]   = float(t_m.group(1)) if t_m else None
        row["bme_pres_hpa"] = float(p_m.group(1)) if p_m else None
        row["bme_humi_pct"] = float(h_m.group(1)) if h_m else None

    return row


# ── Affichage terminal ────────────────────────────────────────────────────────

def fmt(val, unit: str, decimals: int = 2) -> str:
    if val is None:
        return f"{'--':>10}"
    return f"{val:>{10}.{decimals}f} {unit}"

def print_row(ts: str, row: dict):
    ds   = fmt(row["ds_temp_c"],   "°C")
    temp = fmt(row["bme_temp_c"],  "°C")
    pres = fmt(row["bme_pres_hpa"],"hPa", 1)
    humi = fmt(row["bme_humi_pct"],"%",   1)
    print(f"[{ts}]  DS18B20:{ds}  T:{temp}  P:{pres}  H:{humi}")


# ── Point d'entrée ────────────────────────────────────────────────────────────

def main():
    # Arguments
    port    = sys.argv[1] if len(sys.argv) > 1 else None
    baud    = int(sys.argv[2]) if len(sys.argv) > 2 else 115200

    if port is None:
        port = find_pico_port()
        if port is None:
            print("Aucun Pico W détecté. Brancher la carte ou spécifier le port.")
            print("  Usage : python acquisition.py COM3")
            sys.exit(1)
        print(f"Pico W détecté sur {port}")

    # Dossier de sortie
    os.makedirs("data", exist_ok=True)
    ts_file = datetime.now().strftime("%Y%m%d_%H%M%S")
    csv_path = os.path.join("data", f"mesures_{ts_file}.csv")

    print(f"Enregistrement → {csv_path}")
    print("Ctrl+C pour arrêter\n")
    print(f"{'Horodatage':<20}  {'DS18B20':>12}  {'BME T':>12}  {'BME P':>12}  {'BME H':>10}")
    print("-" * 75)

    FIELDS = ["timestamp", "ds_temp_c", "bme_temp_c", "bme_pres_hpa", "bme_humi_pct"]

    with serial.Serial(port, baud, timeout=2) as ser, \
         open(csv_path, "w", newline="", encoding="utf-8") as f:

        writer = csv.DictWriter(f, fieldnames=FIELDS)
        writer.writeheader()

        try:
            while True:
                raw = ser.readline()
                if not raw:
                    continue
                line = raw.decode("utf-8", errors="replace").strip()

                row = parse_line(line)
                if row is None:
                    # Lignes de statut (init, erreurs) : affichage brut
                    if line:
                        print(f"  >> {line}")
                    continue

                ts = datetime.now().strftime("%Y-%m-%d %H:%M:%S.%f")[:-3]
                print_row(ts, row)

                writer.writerow({"timestamp": ts, **row})
                f.flush()

        except KeyboardInterrupt:
            print(f"\nArrêt. Données enregistrées dans {csv_path}")


if __name__ == "__main__":
    main()
