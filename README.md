# Heim-AI Windows-App (Mikro-Phase 2.5)

Windows-11-Client des Heim-AI-Projekts — bewusst als **dünne native Shell**
um die zentrale Web-UI des Voice-Orchestrators gebaut (Architektur-
Entscheidung „voll zentral", Juli 2026):

- Die komplette User-Oberfläche (Chat, Verlauf, Einstellungen, Karten)
  kommt **live vom Orchestrator** (`https://<server>/app`, Repo
  [`Local-AI-Voice-Orchastrator`](https://github.com/daswoody/Local-AI-Voice-Orchastrator),
  Verzeichnis `frontend/`). Ein Server-Deploy aktualisiert damit alle
  Windows-Clients — analog zu „Karte deployen = Skill deployen" (Spez 4.12).
- Auch die **Chat-Historie liegt zentral** auf dem Server
  (`GET /v1/conversations`): Windows, Android und Browser sehen dieselben
  Gespräche.
- Diese Shell (Tauri 2, Rust) liefert nur, was ein Browser nicht kann.

## Was die Shell macht

| Funktion | Umsetzung |
|---|---|
| System-Tray, Close-to-Tray, Autostart | Tray-Icon mit Menü (Öffnen / Realtime Voice / Screenshot / Beenden) |
| Globale Hotkeys | Chat öffnen, Realtime Voice, Screenshot — konfigurierbar in den App-Einstellungen (UI) |
| Antwort-Popups über allen Anwendungen, **anpinnbar** | eigene rahmenlose Topmost-Fenster (keine Windows-Toasts — die wären nicht anpinnbar), rendern die Karten-UI unter `#/popup` |
| Schwebender Voice-/Record-Indikator oben mittig | Topmost-Fenster unter `#/indicator` |
| Desktop-Screenshot für die AI | `capture_screenshot`-Befehl (xcap → PNG → Base64); als Geräte-Tool im WebSocket-`hello` angemeldet, das LLM kann ihn selbst anfordern („Hey AI, hilf mir hier") |
| Wake Word | openWakeWord-Pipeline (ONNX Runtime + cpal), Port der Android-Engine, mit Energie-Gate |

Der Befehls-/Event-Vertrag zwischen Shell und Web-UI ist in
`docs/protocol-additions-2.5.md` des Orchestrator-Repos dokumentiert und
über `shell_api_version` versioniert: verlangt die Server-UI eine neuere
Shell, bleibt die App auf der Bootstrap-Seite und bittet um ein Update.

## Erststart

1. Orchestrator deployen (Branch `claude/windows-app-central-ui` des
   Orchestrator-Repos — enthält `/app`-UI, zentrale Historie, `image_input`).
2. App starten → Bootstrap-Seite → Server-Adresse eingeben
   (z. B. `https://ai.preuss.app`). Die Shell prüft `/v1/health` und
   `/app/version.json`, speichert die Adresse und lädt die UI.
3. Anmelden wie in der Android-App (`/v1/auth/login`).

## Wake Word

**Kein Setup nötig** — die openWakeWord-Modelle (Apache-2.0, von
<https://github.com/dscripka/openWakeWord>, v0.5.1) sind im Installer
enthalten: die Basis-Pipeline (`melspectrogram.onnx`,
`embedding_model.onnx`) plus drei Wake Words zur Auswahl in den
App-Einstellungen:

- **„Hey Jarvis"** (Standard)
- **„Alexa"**
- **„Hey Mycroft"**

Einfach in den Einstellungen „Wake Word aktiv" einschalten und das Wort
wählen. Erkennung → Assist-Modus: einmal zuhören, antworten
(Sprachantwort + ggf. Karte als Popup), fertig.

**Eigene Modelle** (z. B. eine selbst trainierte Phrase): `.onnx`-Datei
nach `%APPDATA%\de.heimai.windows\openwakeword\` legen und in den
Einstellungen „Eigenes Modell" wählen. Der Nutzer-Ordner gewinnt auch bei
Namensgleichheit mit den mitgelieferten Modellen (Update-Möglichkeit).

## Build

Voraussetzungen: Rust (stable), Node ≥ 20 (nur für die Tauri-CLI).

```powershell
npm install -g @tauri-apps/cli@^2
tauri build        # NSIS-Installer in src-tauri/target/release/bundle/nsis/
tauri dev          # Entwicklung (lädt die Bootstrap-Seite)
```

CI: `.github/workflows/windows-build.yml` baut auf jedem Push den
Installer und hängt ihn als Artifact `heimai-windows-installer` an
(analog zum `heimai-debug-apk` der Android-App).

## Projektstruktur

```
bootstrap/            gebündelte Mini-UI: Server-Auswahl + Versions-Check
                      (die einzige UI, die NICHT vom Server kommt)
src-tauri/
  src/lib.rs          App-Setup, Tray, Close-to-Tray
  src/commands.rs     IPC-Vertrag zur Web-UI (shell_api_version)
  src/windows.rs      Hauptfenster-Navigation + Voice-Indikator
  src/popups.rs       anpinnbare Antwort-Popups (Topmost-Fenster)
  src/hotkeys.rs      globale Hotkeys → "hotkey"-Events an die UI
  src/screenshot.rs   Desktop-Capture (PNG/Base64, max. 1600 px breit)
  src/wakeword.rs     openWakeWord-Pipeline (cpal + ONNX Runtime)
  capabilities/       Tauri-ACL inkl. Remote-IPC für die Server-UI
```

## Bekannte Punkte / bewusste Entscheidungen

- **Remote-IPC:** Die Capability erlaubt der vom eigenen Server geladenen
  UI den Zugriff auf die Shell-Befehle (`remote.urls`). Das ist im
  Heim-Setup gewollt — die Shell exponiert ausschließlich ihre eigenen,
  schmalen Befehle. Wer das enger ziehen will, trägt dort seine konkrete
  Server-Domain ein.
- **Mikrofon:** Wake Word (nativ/cpal) und Chat-Audio (getUserMedia in
  WebView2) können unter Windows parallel laufen — anders als auf Android
  ist keine Exklusiv-Logik nötig. WebView2 fragt beim ersten Mal pro
  Origin nach der Mikrofon-Freigabe.
- **Eigen-Trigger:** Spricht die TTS-Antwort das Wake Word selbst, könnte
  die Erkennung erneut auslösen; der 2-s-Cooldown dämpft das. Echte
  Echo-Unterdrückung ist ein späterer Punkt.
- Ohne erreichbaren Server zeigt die App nur die Bootstrap-Seite — das
  ist der bewusste Trade-off der zentralen Architektur (ohne Server kann
  die App ohnehin nichts).
