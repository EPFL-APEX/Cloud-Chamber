"""
Interface graphique d'acquisition — Chambre à brouillard (Pico W)

Usage :
    python acquisition_gui.py           # détection automatique du Pico
    python acquisition_gui.py COM3      # port explicite

Dépendances :
    pip install pyserial matplotlib
"""

import sys
import os
import re
import csv
import math
import queue
import threading
import collections
from datetime import datetime

import tkinter as tk
import matplotlib
matplotlib.use("TkAgg")
from matplotlib.figure import Figure
from matplotlib.backends.backend_tkagg import FigureCanvasTkAgg
from matplotlib.gridspec import GridSpec
import serial
import serial.tools.list_ports

# ── Palette (reference palette.md) ───────────────────────────────────────────
C_SURFACE  = "#fcfcfb"
C_PAGE     = "#f0efec"
C_GRID     = "#e1e0d9"
C_INK      = "#0b0b0b"
C_SECONDARY= "#52514e"
C_MUTED    = "#898781"
C_DS       = "#2a78d6"   # slot 1 — blue
C_BME_T    = "#e34948"   # slot 6 — red
C_PRESSURE = "#2a78d6"   # slot 1 — single series
C_HUMIDITY = "#1baf7a"   # slot 2 — aqua

# ── Parsing (identique à acquisition.py) ─────────────────────────────────────
RE_DS   = re.compile(r"DS18B20:\s*([-\d.]+|--)\s*(?:C)?")
RE_TEMP = re.compile(r"BME280:\s*([-\d.]+)\s*C")
RE_PRES = re.compile(r"([\d.]+)\s*hPa")
RE_HUMI = re.compile(r"([\d.]+)\s*%")
RE_SKIP = re.compile(r"BME280:\s*\(skip")

def parse_line(line: str) -> dict | None:
    ds_m = RE_DS.search(line)
    if not ds_m:
        return None
    ds_val = ds_m.group(1)
    row = {"ds_temp_c": float(ds_val) if ds_val != "--" else None}
    if RE_SKIP.search(line):
        row.update(bme_temp_c=None, bme_pres_hpa=None, bme_humi_pct=None)
    else:
        t_m = RE_TEMP.search(line)
        p_m = RE_PRES.search(line)
        h_m = RE_HUMI.search(line)
        row["bme_temp_c"]   = float(t_m.group(1)) if t_m else None
        row["bme_pres_hpa"] = float(p_m.group(1)) if p_m else None
        row["bme_humi_pct"] = float(h_m.group(1)) if h_m else None
    return row

def find_pico_port() -> str | None:
    for p in serial.tools.list_ports.comports():
        if p.vid == 0x2E8A:
            return p.device
    return None


# ── Thread série (daemon — s'arrête avec le processus) ───────────────────────
class SerialReader(threading.Thread):
    def __init__(self, port: str, baud: int, q: queue.Queue):
        super().__init__(daemon=True)
        self.port = port
        self.baud = baud
        self.q    = q
        self._stop_evt = threading.Event()

    def stop(self):
        self._stop_evt.set()

    def run(self):
        try:
            with serial.Serial(self.port, self.baud, timeout=2) as ser:
                while not self._stop_evt.is_set():
                    raw = ser.readline()
                    if not raw:
                        continue
                    line = raw.decode("utf-8", errors="replace").strip()
                    row  = parse_line(line)
                    ts   = datetime.now()
                    self.q.put(("data" if row is not None else "status", ts,
                                row if row is not None else line))
        except Exception as e:
            self.q.put(("error", datetime.now(), str(e)))


# ── Application ───────────────────────────────────────────────────────────────
WINDOW = 300   # nombre de points affichés (fenêtre glissante)

class App:
    def __init__(self, root: tk.Tk, port: str, baud: int = 115200):
        self.root    = root
        self.port    = port
        self.baud    = baud
        self.q       = queue.Queue()
        self.reader  : SerialReader | None = None
        self.csv_file = None
        self.writer   = None

        # Données rolling
        self.t_rel = collections.deque(maxlen=WINDOW)
        self.d_ds  = collections.deque(maxlen=WINDOW)
        self.d_bmt = collections.deque(maxlen=WINDOW)
        self.d_prs = collections.deque(maxlen=WINDOW)
        self.d_hum = collections.deque(maxlen=WINDOW)
        self.t0: datetime | None = None

        self.last = {"ds": None, "bmt": None, "prs": None, "hum": None}

        root.title("Chambre à brouillard — Acquisition")
        root.configure(bg=C_SURFACE)
        root.geometry("1200x800")
        root.minsize(900, 620)

        self._build_header()
        self._build_tiles()
        self._build_plots()
        self._start()

    # ── UI ────────────────────────────────────────────────────────────────────

    def _build_header(self):
        bar = tk.Frame(self.root, bg=C_PAGE, pady=7, padx=14)
        bar.pack(fill="x")

        tk.Label(bar, text=f"Port : {self.port}", bg=C_PAGE,
                 fg=C_INK, font=("Segoe UI", 10)).pack(side="left")

        self.lbl_file = tk.Label(bar, text="", bg=C_PAGE,
                                 fg=C_SECONDARY, font=("Segoe UI", 9))
        self.lbl_file.pack(side="left", padx=18)

        self.lbl_status = tk.Label(bar, text="● Connexion…", bg=C_PAGE,
                                   fg="#eda100", font=("Segoe UI", 10, "bold"))
        self.lbl_status.pack(side="left")

        tk.Button(bar, text="Arrêter", command=self._stop,
                  bg="#e34948", fg="white", relief="flat",
                  font=("Segoe UI", 9, "bold"), padx=12, pady=2,
                  cursor="hand2", activebackground="#c03030",
                  activeforeground="white").pack(side="right")

    def _tile(self, parent, label: str, color: str) -> tk.Label:
        f = tk.Frame(parent, bg=C_SURFACE, padx=18, pady=10)
        f.pack(side="left", fill="both", expand=True, padx=6)
        tk.Label(f, text=label, bg=C_SURFACE, fg=C_MUTED,
                 font=("Segoe UI", 8)).pack(anchor="w")
        v = tk.Label(f, text="—", bg=C_SURFACE, fg=color,
                     font=("Segoe UI", 22, "bold"))
        v.pack(anchor="w")
        return v

    def _build_tiles(self):
        row = tk.Frame(self.root, bg=C_SURFACE, pady=8, padx=12)
        row.pack(fill="x")

        self.v_ds  = self._tile(row, "DS18B20",  C_DS)
        tk.Frame(row, bg=C_GRID, width=1).pack(side="left", fill="y", pady=6)
        self.v_bmt = self._tile(row, "BME280 T", C_BME_T)
        tk.Frame(row, bg=C_GRID, width=1).pack(side="left", fill="y", pady=6)
        self.v_prs = self._tile(row, "Pression", C_PRESSURE)
        tk.Frame(row, bg=C_GRID, width=1).pack(side="left", fill="y", pady=6)
        self.v_hum = self._tile(row, "Humidité", C_HUMIDITY)

    def _build_plots(self):
        self.fig = Figure(facecolor=C_SURFACE)
        gs = GridSpec(2, 2, figure=self.fig,
                      hspace=0.5, wspace=0.32,
                      left=0.07, right=0.97, top=0.93, bottom=0.09)

        # Températures (DS18B20 + BME — même unité, même axe)
        self.ax_t = self.fig.add_subplot(gs[0, :])
        self.ln_ds,  = self.ax_t.plot([], [], color=C_DS,    lw=1.5, label="DS18B20")
        self.ln_bmt, = self.ax_t.plot([], [], color=C_BME_T, lw=1.5, label="BME280")
        self._style(self.ax_t, "Température (°C)")
        self.ax_t.legend(loc="upper right", frameon=False,
                         labelcolor=C_INK, fontsize=8)

        # Pression
        self.ax_p = self.fig.add_subplot(gs[1, 0])
        self.ln_prs, = self.ax_p.plot([], [], color=C_PRESSURE, lw=1.5)
        self._style(self.ax_p, "Pression (hPa)")

        # Humidité
        self.ax_h = self.fig.add_subplot(gs[1, 1])
        self.ln_hum, = self.ax_h.plot([], [], color=C_HUMIDITY, lw=1.5)
        self._style(self.ax_h, "Humidité (%)")

        canvas = FigureCanvasTkAgg(self.fig, master=self.root)
        canvas.get_tk_widget().pack(fill="both", expand=True, padx=12, pady=(0, 10))
        self.canvas = canvas

    def _style(self, ax, title: str):
        ax.set_facecolor(C_SURFACE)
        ax.set_title(title, fontsize=9, color=C_INK, loc="left", pad=4)
        ax.tick_params(labelsize=8, colors=C_MUTED, length=3)
        ax.spines[["top", "right"]].set_visible(False)
        ax.spines[["left", "bottom"]].set_color(C_GRID)
        ax.yaxis.grid(True, color=C_GRID, linewidth=0.7, linestyle="-")
        ax.set_xlabel("t (s)", fontsize=7, color=C_MUTED, labelpad=2)

    # ── Démarrage / arrêt ─────────────────────────────────────────────────────

    def _start(self):
        os.makedirs("data", exist_ok=True)
        ts = datetime.now().strftime("%Y%m%d_%H%M%S")
        path = os.path.join("data", f"mesures_{ts}.csv")
        self.csv_file = open(path, "w", newline="", encoding="utf-8")
        fields = ["timestamp", "ds_temp_c", "bme_temp_c", "bme_pres_hpa", "bme_humi_pct"]
        self.writer = csv.DictWriter(self.csv_file, fieldnames=fields)
        self.writer.writeheader()
        self.lbl_file.config(text=f"→ {os.path.basename(path)}")

        self.reader = SerialReader(self.port, self.baud, self.q)
        self.reader.start()
        self.lbl_status.config(text="● Connecté", fg="#0ca30c")
        self._poll()

    def _stop(self):
        if self.reader:
            self.reader.stop()
        if self.csv_file:
            self.csv_file.close()
        self.lbl_status.config(text="● Arrêté", fg="#e34948")
        self.root.after(400, self.root.destroy)

    # ── Boucle de mise à jour ─────────────────────────────────────────────────

    def _poll(self):
        updated = False
        try:
            while True:
                kind, ts, payload = self.q.get_nowait()
                if kind == "data":
                    self._ingest(ts, payload)
                    updated = True
                elif kind == "error":
                    self.lbl_status.config(
                        text=f"● Erreur : {payload}", fg="#e34948")
        except queue.Empty:
            pass

        if updated:
            self._update_tiles()
            self._update_plots()

        self.root.after(100, self._poll)

    def _ingest(self, ts: datetime, row: dict):
        if self.t0 is None:
            self.t0 = ts
        t = (ts - self.t0).total_seconds()

        self.t_rel.append(t)
        self.d_ds.append(row.get("ds_temp_c"))
        self.d_bmt.append(row.get("bme_temp_c"))
        self.d_prs.append(row.get("bme_pres_hpa"))
        self.d_hum.append(row.get("bme_humi_pct"))

        for k, v in [("ds", "ds_temp_c"), ("bmt", "bme_temp_c"),
                     ("prs", "bme_pres_hpa"), ("hum", "bme_humi_pct")]:
            if row.get(v) is not None:
                self.last[k] = row[v]

        self.writer.writerow({
            "timestamp":    ts.strftime("%Y-%m-%d %H:%M:%S.%f")[:-3],
            "ds_temp_c":    row.get("ds_temp_c"),
            "bme_temp_c":   row.get("bme_temp_c"),
            "bme_pres_hpa": row.get("bme_pres_hpa"),
            "bme_humi_pct": row.get("bme_humi_pct"),
        })
        self.csv_file.flush()

    def _fmt(self, val, unit: str) -> str:
        return f"{val:.2f}{unit}" if val is not None else "—"

    def _update_tiles(self):
        self.v_ds.config( text=self._fmt(self.last["ds"],  " °C"))
        self.v_bmt.config(text=self._fmt(self.last["bmt"], " °C"))
        self.v_prs.config(text=self._fmt(self.last["prs"], " hPa"))
        self.v_hum.config(text=self._fmt(self.last["hum"], " %"))

    def _update_plots(self):
        t = list(self.t_rel)
        if len(t) < 2:
            return

        def to_plot(src):
            return [v if v is not None else math.nan for v in src]

        self.ln_ds.set_data(t, to_plot(self.d_ds))
        self.ln_bmt.set_data(t, to_plot(self.d_bmt))
        self.ln_prs.set_data(t, to_plot(self.d_prs))
        self.ln_hum.set_data(t, to_plot(self.d_hum))

        for ax in (self.ax_t, self.ax_p, self.ax_h):
            ax.relim()
            ax.autoscale_view()

        self.canvas.draw_idle()


# ── Point d'entrée ────────────────────────────────────────────────────────────

def main():
    port = sys.argv[1] if len(sys.argv) > 1 else None
    baud = int(sys.argv[2]) if len(sys.argv) > 2 else 115200

    if port is None:
        port = find_pico_port()
        if port is None:
            print("Aucun Pico W détecté. Brancher la carte ou spécifier le port.")
            print("  Usage : python acquisition_gui.py COM3")
            sys.exit(1)

    root = tk.Tk()
    app  = App(root, port, baud)
    root.protocol("WM_DELETE_WINDOW", app._stop)
    root.mainloop()


if __name__ == "__main__":
    main()
