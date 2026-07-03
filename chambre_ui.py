"""
Interface de contrôle — Chambre à brouillard (Pico W)

Affiche les mesures en temps réel et permet d'envoyer des commandes
au firmware principal (src/bin/main.rs).

Usage :
    python chambre_ui.py           # détection automatique du Pico
    python chambre_ui.py COM3      # port explicite

Commandes envoyées :
    TARGET <°C>   température cible chambre
    COMP <0|1>    autoriser / bloquer le compresseur
    HV <0|1>      haut voltage on / off

Dépendances :
    pip install pyserial matplotlib
"""

import sys, os, re, csv, queue, threading, collections, math
from datetime import datetime

import tkinter as tk
from tkinter import ttk
import matplotlib
matplotlib.use("TkAgg")
from matplotlib.figure import Figure
from matplotlib.backends.backend_tkagg import FigureCanvasTkAgg
from matplotlib.gridspec import GridSpec
import serial
import serial.tools.list_ports

# ── Palette ───────────────────────────────────────────────────────────────────
# Zone données (claire)
C_SURFACE  = "#f9f9f7"
C_PAGE     = "#f0efec"
C_GRID     = "#e4e3db"
C_INK      = "#0b0b0b"
C_SECONDARY= "#52514e"
C_MUTED    = "#9a9791"
C_DS       = "#2a78d6"   # slot 1 — bleu
C_BME_T    = "#e34948"   # slot 6 — rouge
C_PRESSURE = "#2a78d6"
C_HUMIDITY = "#1baf7a"   # slot 2 — aqua
C_GOOD     = "#0ca30c"
C_WARN     = "#d03b3b"
C_ORANGE   = "#eda100"

# Panneau de contrôle (sombre)
C_DARK     = "#13151c"   # fond sidebar
C_DARK2    = "#1c1f2b"   # fond cartes / entrées
C_DARK_DIV = "#252836"   # séparateurs
C_DARK_TXT = "#dde1f0"   # texte principal
C_DARK_SUB = "#7b80a0"   # texte secondaire
C_DARK_LBL = "#454a65"   # étiquettes section

# ── Capteurs DS18B20 — noms et couleurs ──────────────────────────────────────
DS_LABELS = [
    "Sortie compresseur",   # ds0 — GP11
    "Sortie condenseur",    # ds1 — GP12
    "Entrée évaporateur",   # ds2 — GP13
    "Sortie évaporateur",   # ds3 — GP14
    "Base chambre",         # ds4 — GP15 (capteur de contrôle)
]
DS_COLORS = ["#eda100", "#4a3aa7", "#1baf7a", "#e87ba4", C_DS]

# ── Parsing de la ligne STATE ─────────────────────────────────────────────────
RE_STATE = re.compile(r"^STATE\s+(.+)$")

def parse_state(line: str) -> dict | None:
    m = RE_STATE.match(line)
    if not m:
        return None
    out = {}
    for token in m.group(1).split():
        if "=" not in token:
            continue
        k, v = token.split("=", 1)
        out[k] = None if v == "--" else _try_float(v)
    return out

def _try_float(s: str) -> float | str:
    try:
        return float(s)
    except ValueError:
        return s

def find_pico() -> str | None:
    for p in serial.tools.list_ports.comports():
        if p.vid == 0x2E8A:
            return p.device
    return None

# ── Thread série (bidirectionnel) ─────────────────────────────────────────────
class SerialThread(threading.Thread):
    def __init__(self, port: str, baud: int,
                 data_q: queue.Queue, cmd_q: queue.Queue):
        super().__init__(daemon=True)
        self.port   = port
        self.baud   = baud
        self.data_q = data_q
        self.cmd_q  = cmd_q
        self._stop  = threading.Event()

    def stop(self):
        self._stop.set()

    def run(self):
        try:
            with serial.Serial(self.port, self.baud, timeout=1) as ser:
                while not self._stop.is_set():
                    # Envoyer les commandes en attente
                    try:
                        while True:
                            cmd = self.cmd_q.get_nowait()
                            ser.write((cmd + "\n").encode())
                    except queue.Empty:
                        pass

                    raw = ser.readline()
                    if not raw:
                        continue
                    line = raw.decode("utf-8", errors="replace").strip()
                    ts   = datetime.now()

                    state = parse_state(line)
                    if state is not None:
                        self.data_q.put(("state", ts, state))
                    elif line:
                        self.data_q.put(("msg", ts, line))

        except Exception as e:
            self.data_q.put(("error", datetime.now(), str(e)))


# ── Application ───────────────────────────────────────────────────────────────
WINDOW = 300

class App:
    def __init__(self, root: tk.Tk, port: str, baud: int = 115200):
        self.root  = root
        self.port  = port
        self.baud  = baud
        self.data_q: queue.Queue = queue.Queue()
        self.cmd_q:  queue.Queue = queue.Queue()
        self.thread: SerialThread | None = None
        self.csv_file = None
        self.writer   = None

        # Données rolling
        self.t_rel  = collections.deque(maxlen=WINDOW)
        self.d_ds   = [collections.deque(maxlen=WINDOW) for _ in range(5)]  # ds0..ds4
        self.d_bmt  = collections.deque(maxlen=WINDOW)
        self.d_prs  = collections.deque(maxlen=WINDOW)
        self.d_hum  = collections.deque(maxlen=WINDOW)
        self.t0: datetime | None = None

        self.last: dict = {}          # dernier STATE reçu

        # État des boutons (cohérent avec le firmware au démarrage)
        self.comp_allowed = True
        self.hv_enabled   = False

        root.title("Chambre à brouillard — Contrôle")
        root.configure(bg=C_DARK)
        root.geometry("1440x840")
        root.minsize(1100, 680)

        self._build_ui()
        self._start()

    # ── Construction de l'interface ───────────────────────────────────────────

    def _build_ui(self):
        self._build_header()

        content = tk.Frame(self.root, bg=C_SURFACE)
        content.pack(fill="both", expand=True)

        left = tk.Frame(content, bg=C_SURFACE)
        left.pack(side="left", fill="both", expand=True)

        # Sidebar sombre
        right = tk.Frame(content, bg=C_DARK, width=380)
        right.pack_propagate(False)
        right.pack(side="right", fill="y")

        self._build_tiles(left)
        self._build_notebook(left)
        self._build_controls(right)

    def _build_header(self):
        bar = tk.Frame(self.root, bg=C_DARK2, pady=10, padx=18)
        bar.pack(fill="x")

        # Titre projet
        tk.Label(bar, text="CHAMBRE À BROUILLARD", bg=C_DARK2,
                 fg=C_DARK_TXT, font=("Segoe UI", 11, "bold")).pack(side="left")

        tk.Label(bar, text="  ·  ", bg=C_DARK2,
                 fg=C_DARK_DIV, font=("Segoe UI", 11)).pack(side="left")

        tk.Label(bar, text=self.port, bg=C_DARK2,
                 fg=C_DARK_SUB, font=("Segoe UI", 10)).pack(side="left")

        self.lbl_file = tk.Label(bar, text="", bg=C_DARK2,
                                 fg=C_DARK_LBL, font=("Segoe UI", 9))
        self.lbl_file.pack(side="left", padx=14)

        tk.Button(bar, text="  ARRÊTER  ", command=self._stop,
                  bg="#3a1a1a", fg="#e87070", relief="flat",
                  font=("Segoe UI", 9, "bold"), pady=4,
                  cursor="hand2", activebackground="#4a2020",
                  activeforeground="#ff9090", bd=0).pack(side="right")

        self.lbl_status = tk.Label(bar, text="●  Connexion…", bg=C_DARK2,
                                   fg=C_ORANGE, font=("Segoe UI", 10, "bold"))
        self.lbl_status.pack(side="right", padx=16)

    def _tile(self, parent, label: str, color: str) -> tk.Label:
        # Bordure simulée par un frame englobant
        border = tk.Frame(parent, bg=C_GRID, padx=1, pady=1)
        border.pack(side="left", fill="both", expand=True, padx=5)

        body = tk.Frame(border, bg="#ffffff", padx=18, pady=14)
        body.pack(fill="both", expand=True)

        tk.Label(body, text=label.upper(), bg="#ffffff", fg=C_MUTED,
                 font=("Segoe UI", 7, "bold")).pack(anchor="w")

        v = tk.Label(body, text="—", bg="#ffffff", fg=color,
                     font=("Segoe UI", 26, "bold"))
        v.pack(anchor="w", pady=(2, 0))

        # Barre de couleur en bas
        tk.Frame(border, bg=color, height=3).pack(fill="x")
        return v

    def _build_tiles(self, parent):
        row = tk.Frame(parent, bg=C_SURFACE, pady=12, padx=10)
        row.pack(fill="x")
        self.v_ds  = self._tile(row, "Base chambre", C_DS)
        self.v_bmt = self._tile(row, "BME280 T",     C_BME_T)
        self.v_prs = self._tile(row, "Pression",     C_PRESSURE)
        self.v_hum = self._tile(row, "Humidité",     C_HUMIDITY)

    def _build_notebook(self, parent):
        style = ttk.Style()
        style.theme_use('default')
        style.configure('Chambre.TNotebook', background=C_SURFACE, borderwidth=0,
                        tabmargins=[0, 4, 0, 0])
        style.configure('Chambre.TNotebook.Tab', background=C_PAGE, foreground=C_SECONDARY,
                        font=('Segoe UI', 9), padding=[16, 7])
        style.map('Chambre.TNotebook.Tab',
                  background=[('selected', '#ffffff'), ('active', '#f5f4f0')],
                  foreground=[('selected', C_INK), ('active', C_INK)])

        nb = ttk.Notebook(parent, style='Chambre.TNotebook')
        nb.pack(fill="both", expand=True, padx=10, pady=(0, 10))

        tab_overview = tk.Frame(nb, bg=C_SURFACE)
        nb.add(tab_overview, text="  VUE GLOBALE  ")

        tab_ds = tk.Frame(nb, bg=C_SURFACE)
        nb.add(tab_ds,  text="  TEMPÉRATURES DS  ")

        tab_bme = tk.Frame(nb, bg=C_SURFACE)
        nb.add(tab_bme, text="  AMBIANCE BME280  ")

        self._build_plots_overview(tab_overview)
        self._build_plots_ds(tab_ds)
        self._build_plots_bme(tab_bme)

    def _build_plots_overview(self, parent):
        self.fig = Figure(facecolor=C_SURFACE)
        gs = GridSpec(2, 2, figure=self.fig,
                      hspace=0.52, wspace=0.30,
                      left=0.07, right=0.97, top=0.93, bottom=0.09)

        self.ax_t = self.fig.add_subplot(gs[0, :])
        self.ln_ds = []
        for i, (lbl, col) in enumerate(zip(DS_LABELS, DS_COLORS)):
            ln, = self.ax_t.plot([], [], color=col, lw=1.8,
                                 label=lbl, solid_capstyle="round")
            self.ln_ds.append(ln)
        self.ln_bmt, = self.ax_t.plot([], [], color=C_BME_T, lw=1.8, label="BME280 (amb.)",
                                      solid_capstyle="round", linestyle="--", alpha=0.8)
        self._style(self.ax_t, "Température (°C)")
        self.ax_t.legend(loc="upper right", frameon=False,
                         labelcolor=C_INK, fontsize=7.5, ncol=2)

        self.ax_p = self.fig.add_subplot(gs[1, 0])
        self.ln_prs, = self.ax_p.plot([], [], color=C_PRESSURE, lw=2.0,
                                      solid_capstyle="round")
        self._style(self.ax_p, "Pression (hPa)")

        self.ax_h = self.fig.add_subplot(gs[1, 1])
        self.ln_hum, = self.ax_h.plot([], [], color=C_HUMIDITY, lw=2.0,
                                      solid_capstyle="round")
        self._style(self.ax_h, "Humidité (%)")

        canvas = FigureCanvasTkAgg(self.fig, master=parent)
        canvas.get_tk_widget().pack(fill="both", expand=True)
        self.canvas = canvas

    def _build_plots_ds(self, parent):
        """5 graphiques individuels DS18B20, ds4 (base chambre) en pleine largeur."""
        self.fig_ds = Figure(facecolor=C_SURFACE)
        gs = GridSpec(3, 2, figure=self.fig_ds,
                      hspace=0.65, wspace=0.30,
                      left=0.07, right=0.97, top=0.95, bottom=0.07)

        self.ax_ds     = []
        self.ln_ds_ind = []
        for i, (r, c) in enumerate([(0, 0), (0, 1), (1, 0), (1, 1)]):
            ax = self.fig_ds.add_subplot(gs[r, c])
            ln, = ax.plot([], [], color=DS_COLORS[i], lw=2.0, solid_capstyle="round")
            self._style(ax, f"{DS_LABELS[i]}  (ds{i})")
            self.ax_ds.append(ax)
            self.ln_ds_ind.append(ln)

        # ds4 — capteur de contrôle, pleine largeur
        ax4 = self.fig_ds.add_subplot(gs[2, :])
        ln4, = ax4.plot([], [], color=DS_COLORS[4], lw=2.5, solid_capstyle="round")
        self._style(ax4, "Base chambre  (ds4)  ·  capteur de contrôle")
        self.ax_ds.append(ax4)
        self.ln_ds_ind.append(ln4)

        canvas_ds = FigureCanvasTkAgg(self.fig_ds, master=parent)
        canvas_ds.get_tk_widget().pack(fill="both", expand=True)
        self.canvas_ds = canvas_ds

    def _build_plots_bme(self, parent):
        """3 graphiques BME280 : température, pression, humidité."""
        self.fig_bme = Figure(facecolor=C_SURFACE)
        gs = GridSpec(3, 1, figure=self.fig_bme,
                      hspace=0.55,
                      left=0.08, right=0.97, top=0.95, bottom=0.06)

        self.ax_bme_t = self.fig_bme.add_subplot(gs[0])
        self.ln_bmt_bme, = self.ax_bme_t.plot([], [], color=C_BME_T, lw=2.0,
                                               solid_capstyle="round")
        self._style(self.ax_bme_t, "Température ambiante (°C)")

        self.ax_bme_p = self.fig_bme.add_subplot(gs[1])
        self.ln_prs_bme, = self.ax_bme_p.plot([], [], color=C_PRESSURE, lw=2.0,
                                               solid_capstyle="round")
        self._style(self.ax_bme_p, "Pression atmosphérique (hPa)")

        self.ax_bme_h = self.fig_bme.add_subplot(gs[2])
        self.ln_hum_bme, = self.ax_bme_h.plot([], [], color=C_HUMIDITY, lw=2.0,
                                               solid_capstyle="round")
        self._style(self.ax_bme_h, "Humidité relative (%)")

        canvas_bme = FigureCanvasTkAgg(self.fig_bme, master=parent)
        canvas_bme.get_tk_widget().pack(fill="both", expand=True)
        self.canvas_bme = canvas_bme

    def _style(self, ax, title: str):
        ax.set_facecolor("#ffffff")
        ax.set_title(title, fontsize=9.5, color=C_INK, loc="left", pad=6,
                     fontweight="semibold")
        ax.tick_params(labelsize=8, colors=C_MUTED, length=0)
        ax.spines[["top", "right", "left", "bottom"]].set_visible(False)
        ax.yaxis.grid(True, color=C_GRID, linewidth=0.8, linestyle="-")
        ax.xaxis.grid(True, color=C_GRID, linewidth=0.4, linestyle=":")
        ax.set_xlabel("t (s)", fontsize=7.5, color=C_MUTED, labelpad=3)
        ax.set_axisbelow(True)

    def _build_controls(self, parent):
        p = tk.Frame(parent, bg=C_DARK, padx=22, pady=20)
        p.pack(fill="both", expand=True)

        def section(text):
            tk.Label(p, text=text, bg=C_DARK, fg=C_DARK_LBL,
                     font=("Segoe UI", 7, "bold")).pack(anchor="w", pady=(0, 6))

        def divider(top=6, bottom=14):
            tk.Frame(p, bg=C_DARK_DIV, height=1).pack(fill="x",
                                                        pady=(top, bottom))

        # ── Titre ────────────────────────────────────────────────────────────
        tk.Label(p, text="CONTRÔLE", bg=C_DARK, fg=C_DARK_TXT,
                 font=("Segoe UI", 13, "bold")).pack(anchor="w")
        divider(top=10, bottom=16)

        # ── Température cible ─────────────────────────────────────────────────
        section("TEMPÉRATURE CIBLE")
        tf = tk.Frame(p, bg=C_DARK)
        tf.pack(fill="x", pady=(0, 16))
        self.target_var = tk.StringVar(value="-40.0")
        ent = tk.Entry(tf, textvariable=self.target_var, width=8,
                       font=("Segoe UI", 12), bg=C_DARK2, fg=C_DARK_TXT,
                       insertbackground=C_DARK_TXT,
                       bd=0, relief="flat", highlightthickness=1,
                       highlightbackground=C_DARK_DIV,
                       highlightcolor=C_DS)
        ent.pack(side="left", ipady=5, ipadx=6)
        tk.Label(tf, text="°C", bg=C_DARK, fg=C_DARK_SUB,
                 font=("Segoe UI", 10)).pack(side="left", padx=(6, 10))
        tk.Button(tf, text="SET", command=self._cmd_target,
                  bg=C_DS, fg="white", font=("Segoe UI", 9, "bold"),
                  padx=14, pady=5, relief="flat", cursor="hand2",
                  activebackground="#1f67c5",
                  activeforeground="white", bd=0).pack(side="left")

        divider()

        # ── Compresseur ────────────────────────────────────────────────────────
        section("COMPRESSEUR")
        self.btn_comp = tk.Button(p, command=self._toggle_comp,
                                  font=("Segoe UI", 10, "bold"),
                                  relief="flat", cursor="hand2",
                                  pady=9, anchor="w", padx=14, bd=0)
        self.btn_comp.pack(fill="x", pady=(0, 16))
        self._refresh_comp()

        divider()

        # ── Haut Voltage ───────────────────────────────────────────────────────
        section("HAUT VOLTAGE")
        self.btn_hv = tk.Button(p, command=self._toggle_hv,
                                font=("Segoe UI", 10, "bold"),
                                relief="flat", cursor="hand2",
                                pady=9, anchor="w", padx=14, bd=0)
        self.btn_hv.pack(fill="x", pady=(0, 16))
        self._refresh_hv()

        divider()

        # ── Indicateurs d'état ────────────────────────────────────────────────
        section("ÉTAT SYSTÈME")

        def stat_row(label):
            f = tk.Frame(p, bg=C_DARK)
            f.pack(fill="x", pady=4)
            tk.Label(f, text=label, bg=C_DARK, fg=C_DARK_SUB,
                     font=("Segoe UI", 9), width=14, anchor="w").pack(side="left")
            v = tk.Label(f, text="—", bg=C_DARK, fg=C_DARK_TXT,
                         font=("Segoe UI", 9, "bold"))
            v.pack(side="left")
            return v

        self.v_target_act = stat_row("Cible active")
        self.v_comp_act   = stat_row("Compresseur")
        self.v_iso_duty   = stat_row("ISO duty")
        self.v_safety     = stat_row("Sécurité")
        self.v_uptime     = stat_row("Durée")

        divider(top=12, bottom=10)

        # ── Journal ───────────────────────────────────────────────────────────
        section("JOURNAL")
        log_frame = tk.Frame(p, bg=C_DARK2, padx=1, pady=1)
        log_frame.pack(fill="both", expand=True)
        self.log = tk.Text(log_frame, bg=C_DARK2, fg="#7090c0",
                           font=("Consolas", 8), bd=0, relief="flat",
                           state="disabled", wrap="word",
                           insertbackground=C_DARK_TXT)
        self.log.pack(fill="both", expand=True, padx=6, pady=6)

    # ── Commandes ─────────────────────────────────────────────────────────────

    def _send(self, cmd: str):
        self.cmd_q.put(cmd)
        self._log(f"→ {cmd}")

    def _cmd_target(self):
        v = self.target_var.get().strip()
        self._send(f"TARGET {v}")

    def _toggle_comp(self):
        self.comp_allowed = not self.comp_allowed
        self._send(f"COMP {'1' if self.comp_allowed else '0'}")
        self._refresh_comp()

    def _toggle_hv(self):
        self.hv_enabled = not self.hv_enabled
        self._send(f"HV {'1' if self.hv_enabled else '0'}")
        self._refresh_hv()

    def _refresh_comp(self):
        if self.comp_allowed:
            self.btn_comp.config(text="  ●  AUTORISÉ", bg="#0f3d14",
                                 fg="#4cde58", activebackground="#144d1a",
                                 activeforeground="#4cde58")
        else:
            self.btn_comp.config(text="  ○  BLOQUÉ", bg="#3d0f0f",
                                 fg="#e87070", activebackground="#4d1414",
                                 activeforeground="#e87070")

    def _refresh_hv(self):
        if self.hv_enabled:
            self.btn_hv.config(text="  ⚡  ACTIVÉ", bg="#3d2a00",
                               fg="#f0b429", activebackground="#4d3500",
                               activeforeground="#f0b429")
        else:
            self.btn_hv.config(text="  ○  DÉSACTIVÉ", bg=C_DARK2,
                               fg=C_DARK_SUB, activebackground="#252836",
                               activeforeground=C_DARK_TXT)

    def _log(self, msg: str):
        ts = datetime.now().strftime("%H:%M:%S")
        self.log.config(state="normal")
        self.log.insert("end", f"[{ts}] {msg}\n")
        self.log.see("end")
        self.log.config(state="disabled")

    # ── Démarrage / arrêt ─────────────────────────────────────────────────────

    def _start(self):
        os.makedirs("data", exist_ok=True)
        ts   = datetime.now().strftime("%Y%m%d_%H%M%S")
        path = os.path.join("data", f"controle_{ts}.csv")
        self.csv_file = open(path, "w", newline="", encoding="utf-8")
        self.writer   = csv.DictWriter(self.csv_file, fieldnames=[
            "timestamp",
            "ds0_c", "ds1_c", "ds2_c", "ds3_c", "ds4_c",
            "bme_t_c", "bme_p_hpa", "bme_h_pct",
            "target_c", "comp", "hv", "iso_duty", "safe", "up_s"])
        self.writer.writeheader()
        self.lbl_file.config(text=f"→ {os.path.basename(path)}")

        self.thread = SerialThread(self.port, self.baud, self.data_q, self.cmd_q)
        self.thread.start()
        self.lbl_status.config(text="● Connecté", fg=C_GOOD)
        self._poll()

    def _stop(self):
        if self.thread:
            self.thread.stop()
        if self.csv_file:
            self.csv_file.close()
        self.lbl_status.config(text="● Arrêté", fg=C_WARN)
        self.root.after(300, self.root.destroy)

    # ── Boucle de mise à jour ─────────────────────────────────────────────────

    def _poll(self):
        updated = False
        try:
            while True:
                kind, ts, payload = self.data_q.get_nowait()
                if kind == "state":
                    self._ingest(ts, payload)
                    updated = True
                elif kind == "msg":
                    self._log(payload)
                    if "error" in kind:
                        self.lbl_status.config(text=f"● Erreur", fg=C_WARN)
        except queue.Empty:
            pass

        if updated:
            self._update_tiles()
            self._update_controls()
            self._update_plots()

        self.root.after(100, self._poll)

    def _ingest(self, ts: datetime, s: dict):
        if self.t0 is None:
            self.t0 = ts
        t = (ts - self.t0).total_seconds()
        self.last = s

        self.t_rel.append(t)
        for i in range(5):
            self.d_ds[i].append(s.get(f"ds{i}"))
        self.d_bmt.append(s.get("bme_t"))
        self.d_prs.append(s.get("bme_p"))
        self.d_hum.append(s.get("bme_h"))

        if self.writer:
            self.writer.writerow({
                "timestamp": ts.strftime("%Y-%m-%d %H:%M:%S.%f")[:-3],
                "ds0_c":     s.get("ds0"), "ds1_c": s.get("ds1"),
                "ds2_c":     s.get("ds2"), "ds3_c": s.get("ds3"),
                "ds4_c":     s.get("ds4"),
                "bme_t_c":   s.get("bme_t"),
                "bme_p_hpa": s.get("bme_p"),
                "bme_h_pct": s.get("bme_h"),
                "target_c":  s.get("target"),
                "comp":      s.get("comp"),
                "hv":        s.get("hv"),
                "iso_duty":  s.get("iso"),
                "safe":      s.get("safe"),
                "up_s":      s.get("up"),
            })
            self.csv_file.flush()

    def _fmt(self, key: str, unit: str, decimals: int = 1) -> str:
        v = self.last.get(key)
        if v is None:
            return "—"
        return f"{v:.{decimals}f} {unit}".strip()

    def _update_tiles(self):
        self.v_ds.config( text=self._fmt("ds4",   "°C",  1))  # base chambre
        self.v_bmt.config(text=self._fmt("bme_t", "°C",  1))
        self.v_prs.config(text=self._fmt("bme_p", "hPa", 0))
        self.v_hum.config(text=self._fmt("bme_h", "%",   0))

    def _update_controls(self):
        s = self.last
        self.v_target_act.config(text=self._fmt("target", "°C", 1))
        comp = s.get("comp")
        if comp is not None:
            txt = "ON" if comp else "OFF"
            col = C_GOOD if comp else C_MUTED
            self.v_comp_act.config(text=txt, fg=col)
        iso = s.get("iso")
        if iso is not None:
            self.v_iso_duty.config(text=f"{iso*100:.0f} %")
        safe = s.get("safe")
        if safe is not None:
            self.v_safety.config(
                text="ARRÊT D'URGENCE" if safe else "OK",
                fg=C_WARN if safe else C_GOOD)
        up = s.get("up")
        if up is not None:
            h, rem = divmod(int(up), 3600)
            m, sec = divmod(rem, 60)
            self.v_uptime.config(text=f"{h:02d}:{m:02d}:{sec:02d}")

    def _update_plots(self):
        t = list(self.t_rel)
        if len(t) < 2:
            return

        def to_plot(src):
            return [v if v is not None else math.nan for v in src]

        ds_data = [to_plot(self.d_ds[i]) for i in range(5)]
        bmt = to_plot(self.d_bmt)
        prs = to_plot(self.d_prs)
        hum = to_plot(self.d_hum)

        # Onglet Vue globale
        for i, ln in enumerate(self.ln_ds):
            ln.set_data(t, ds_data[i])
        self.ln_bmt.set_data(t, bmt)
        self.ln_prs.set_data(t, prs)
        self.ln_hum.set_data(t, hum)
        for ax in (self.ax_t, self.ax_p, self.ax_h):
            ax.relim(); ax.autoscale_view()
        self.canvas.draw_idle()

        # Onglet Températures DS
        for i, (ln, ax) in enumerate(zip(self.ln_ds_ind, self.ax_ds)):
            ln.set_data(t, ds_data[i])
            ax.relim(); ax.autoscale_view()
        self.canvas_ds.draw_idle()

        # Onglet Ambiance BME280
        self.ln_bmt_bme.set_data(t, bmt)
        self.ln_prs_bme.set_data(t, prs)
        self.ln_hum_bme.set_data(t, hum)
        for ax in (self.ax_bme_t, self.ax_bme_p, self.ax_bme_h):
            ax.relim(); ax.autoscale_view()
        self.canvas_bme.draw_idle()


# ── Point d'entrée ────────────────────────────────────────────────────────────

def main():
    port = sys.argv[1] if len(sys.argv) > 1 else None
    baud = int(sys.argv[2]) if len(sys.argv) > 2 else 115200

    if port is None:
        port = find_pico()
        if port is None:
            print("Aucun Pico W détecté. Branchez la carte ou spécifiez le port.")
            print("  Usage : python chambre_ui.py COM3")
            sys.exit(1)
        print(f"Pico W détecté : {port}")

    root = tk.Tk()
    app  = App(root, port, baud)
    root.protocol("WM_DELETE_WINDOW", app._stop)
    root.mainloop()


if __name__ == "__main__":
    main()
