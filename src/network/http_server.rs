/// Serveur HTTP minimaliste pour le Pico W.
///
/// Deux endpoints :
/// - `GET /`          → Dashboard HTML (embarqué dans le firmware)
/// - `GET /api/data`  → JSON avec toutes les données capteurs
///
/// Le serveur tourne sur Core 1 et lit les données depuis
/// `data::SHARED_STATE` (partagé avec Core 0).

use embassy_net::tcp::TcpSocket;
use embassy_net::Stack;
use embassy_time::Duration;
use defmt;
use heapless::String;
use core::fmt::Write;
use embedded_io_async::Write as IoWrite;

use crate::config::{HTTP_PORT, TEMP_LABELS};
use crate::data::SHARED_STATE;

/// Taille du buffer TCP
const TX_BUF_SIZE: usize = 2048;
const RX_BUF_SIZE: usize = 512;

/// Dashboard HTML embarqué (minifié)
const DASHBOARD_HTML: &str = r#"<!DOCTYPE html>
<html lang="fr"><head><meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Cloud Chamber</title>
<style>
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:monospace;background:#0a0e17;color:#c8d6e5;padding:16px}
h1{font-size:1.2em;color:#00d2ff;text-align:center;margin-bottom:16px}
.g{display:grid;grid-template-columns:repeat(auto-fit,minmax(200px,1fr));gap:10px;max-width:800px;margin:0 auto}
.c{background:#111827;border:1px solid #1e3a5f;border-radius:6px;padding:12px}
.c.a{border-color:#ef4444}
.l{font-size:.65em;color:#64748b;text-transform:uppercase;letter-spacing:1px}
.v{font-size:1.6em;font-weight:bold;margin-top:4px}
.v.cold{color:#00d2ff}.v.hot{color:#f87171}.v.warn{color:#f59e0b}.v.ok{color:#10b981}
.s{text-align:center;margin:12px 0;font-size:.7em;color:#64748b}
.d{width:8px;height:8px;border-radius:50%;display:inline-block;margin-right:4px;vertical-align:middle}
.d.on{background:#10b981}.d.off{background:#ef4444}
.al{margin-top:12px;max-width:800px;margin-left:auto;margin-right:auto}
.al .e{background:#1c1117;border:1px solid #7f1d1d;border-radius:4px;padding:8px;margin:4px 0;font-size:.75em;color:#fca5a5}
</style></head><body>
<h1>Cloud Chamber Monitor</h1>
<div class="s" id="st"><span class="d off"></span>Connecting...</div>
<div class="g" id="gr"></div>
<div class="al" id="al"></div>
<script>
function cc(v){if(v===null)return'ok';if(v<=-20)return'cold';if(v<=10)return'cold';if(v<=50)return'warn';return'hot'}
function poll(){fetch('/api/data').then(r=>r.json()).then(d=>{
document.getElementById('st').innerHTML='<span class="d on"></span>Core0 cycles: '+d.cycle_count+' | Uptime: '+d.uptime_s+'s';
let h='';
d.temperatures.forEach(s=>{let v=s.valid?s.value.toFixed(2):'--';
h+='<div class="c'+(s.critical?' a':'')+'"><div class="l">'+s.label+(s.critical?' [CRIT]':'')+'</div><div class="v '+cc(s.value)+'">'+v+'&deg;C</div></div>'});
h+='<div class="c"><div class="l">Pression BP</div><div class="v warn">'+(d.pressure_bp.valid?d.pressure_bp.pressure.toFixed(3):'--')+' bar</div></div>';
h+='<div class="c"><div class="l">Pression HP</div><div class="v warn">'+(d.pressure_hp.valid?d.pressure_hp.pressure.toFixed(2):'--')+' bar</div></div>';
h+='<div class="c"><div class="l">Compresseur</div><div class="v '+(d.compressor_allowed?'ok':'hot')+'">'+
(d.compressor_allowed?'OK':'COUPE')+'</div></div>';
document.getElementById('gr').innerHTML=h;
let a='';d.alarms.forEach(al=>{a+='<div class="e">['+al.level+'] '+al.source+': '+al.message+'</div>'});
document.getElementById('al').innerHTML=a;
}).catch(()=>{document.getElementById('st').innerHTML='<span class="d off"></span>Connection lost'});
}
setInterval(poll,2000);poll();
</script></body></html>"#;

/// Construit la réponse JSON à partir de l'état partagé.
async fn build_json_response() -> String<2048> {
    let mut json: String<2048> = String::new();

    let state = SHARED_STATE.lock().await;

    let _ = write!(json, "{{\"temperatures\":[");
    for (i, reading) in state.temperatures.iter().enumerate() {
        if i > 0 {
            let _ = write!(json, ",");
        }
        let _ = write!(
            json,
            "{{\"label\":\"{}\",\"value\":{:.4},\"valid\":{},\"critical\":{}}}",
            TEMP_LABELS.get(i).unwrap_or(&"unknown"),
            reading.value,
            reading.valid,
            reading.critical,
        );
    }
    let _ = write!(
        json,
        "],\"pressure_bp\":{{\"pressure\":{:.4},\"temperature\":{:.1},\"valid\":{}}},",
        state.pressure_bp.pressure,
        state.pressure_bp.temperature,
        state.pressure_bp.valid,
    );
    let _ = write!(
        json,
        "\"pressure_hp\":{{\"pressure\":{:.4},\"temperature\":{:.1},\"valid\":{}}},",
        state.pressure_hp.pressure,
        state.pressure_hp.temperature,
        state.pressure_hp.valid,
    );
    let _ = write!(
        json,
        "\"compressor_allowed\":{},\"cycle_count\":{},\"uptime_s\":{},",
        state.compressor_allowed,
        state.cycle_count,
        state.uptime_s,
    );

    // Alarmes
    let _ = write!(json, "\"alarms\":[");
    for (i, alarm) in state.alarms.iter().enumerate() {
        if i > 0 {
            let _ = write!(json, ",");
        }
        let level_str = match alarm.level {
            crate::data::AlarmLevel::Info => "info",
            crate::data::AlarmLevel::Warning => "warning",
            crate::data::AlarmLevel::Critical => "critical",
        };
        let _ = write!(
            json,
            "{{\"level\":\"{}\",\"source\":\"{}\",\"message\":\"{}\",\"t\":{}}}",
            level_str, alarm.source, alarm.message, alarm.timestamp_s,
        );
    }
    let _ = write!(json, "]}}");

    json
}

/// Tâche principale du serveur HTTP.
/// Boucle infinie acceptant les connexions TCP.
pub async fn run(stack: &Stack<'_>) {
    let mut rx_buf = [0u8; RX_BUF_SIZE];
    let mut tx_buf = [0u8; TX_BUF_SIZE];

    loop {
        let mut socket = TcpSocket::new(*stack, &mut rx_buf, &mut tx_buf);
        socket.set_timeout(Some(Duration::from_secs(10)));

        defmt::debug!("HTTP: waiting for connection on port {}...", HTTP_PORT);

        if let Err(e) = socket.accept(HTTP_PORT).await {
            defmt::warn!("HTTP: accept error: {}", e);
            continue;
        }

        defmt::debug!("HTTP: client connected");

        // Lire la requête (on cherche juste GET /api/data vs GET /)
        let mut request_buf = [0u8; 256];
        match socket.read(&mut request_buf).await {
            Ok(0) | Err(_) => {
                let _ = socket.close();
                continue;
            }
            Ok(n) => {
                let request = core::str::from_utf8(&request_buf[..n]).unwrap_or("");

                if request.contains("GET /api/data") {
                    // JSON API
                    let json = build_json_response().await;
                    let header: String<256> = {
                        let mut h: String<256> = String::new();
                        let _ = write!(
                            h,
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            json.len()
                        );
                        h
                    };
                    let _ = socket.write_all(header.as_bytes()).await;
                    let _ = socket.write_all(json.as_bytes()).await;
                } else {
                    // Dashboard HTML
                    let header: String<256> = {
                        let mut h: String<256> = String::new();
                        let _ = write!(
                            h,
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            DASHBOARD_HTML.len()
                        );
                        h
                    };
                    let _ = socket.write_all(header.as_bytes()).await;
                    let _ = socket.write_all(DASHBOARD_HTML.as_bytes()).await;
                }
            }
        }

        let _ = socket.close();
    }
}
