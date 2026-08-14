<div align="center">
  <img src="docs/hivemind-chat.png" alt="HiveMind Chat" width="440"/>
  <h1>HiveMind Chat</h1>
</div>

[🇬🇧 English Version](README.md)

**Ein Chatraum für mehrere Modelle. Rust, Axum, React und SQLite.**

HiveMind Chat setzt mehrere Sprachmodelle in ein Gespräch und gibt diesem Gespräch eine Form: Sie antworten reihum, beziehen gegensätzliche Positionen, werden von einem aus ihrer Mitte moderiert, oder diskutieren und stimmen danach ab. Lokale und gehostete Modelle sitzen im selben Raum, und jeder Beitrag wird live gestreamt.

[![CI](https://github.com/9t29zhmwdh-coder/HiveMind_Chat/actions/workflows/ci.yml/badge.svg)](https://github.com/9t29zhmwdh-coder/HiveMind_Chat/actions/workflows/ci.yml) [![CodeQL](https://github.com/9t29zhmwdh-coder/HiveMind_Chat/actions/workflows/codeql.yml/badge.svg)](https://github.com/9t29zhmwdh-coder/HiveMind_Chat/actions/workflows/codeql.yml) [![OpenSSF Scorecard](https://api.securityscorecards.dev/projects/github.com/9t29zhmwdh-coder/HiveMind_Chat/badge)](https://securityscorecards.dev/viewer/?uri=github.com/9t29zhmwdh-coder/HiveMind_Chat)
![Platform](https://img.shields.io/badge/Platform-Linux%20%7C%20macOS%20%7C%20Docker-lightgrey) ![Rust](https://img.shields.io/badge/Rust-CE422B?logo=rust&logoColor=white) ![React](https://img.shields.io/badge/React-20232a?logo=react&logoColor=61dafb) ![SQLite](https://img.shields.io/badge/SQLite-003B57?logo=sqlite&logoColor=white) ![AI | Claude Code](https://img.shields.io/badge/AI-Claude_Code-black?logo=anthropic&logoColor=white) ![AI | Ollama](https://img.shields.io/badge/AI-Ollama-black?logo=ollama&logoColor=white)

> **So läuft es:** `hivemind-server` ist eine einzelne Binärdatei, die JSON-API und Web-UI auf demselben Port ausliefert, standardmässig gebunden an `127.0.0.1:8750`, und ihren Zustand in einer einzigen SQLite-Datei hält. Es gibt keinen Installer und keinen Hintergrunddienst. `hive` ist dasselbe ohne Browser: Es steuert Räume direkt aus dem Terminal gegen dieselbe Datenbank.

![HiveMind Chat](docs/screenshot.png)

---

**In der Praxis:** Du legst einen Raum an, setzt zwei oder drei Modelle hinein, wählst, wie sie miteinander reden sollen, und stellst eine Frage. Du siehst ihnen live beim Antworten zu, sie widersprechen einander namentlich, und das Transkript bleibt bei dir. Nichts verlässt deinen Rechner, solange du kein gehostetes Modell hinzunimmst.

## Funktionen

- **Fünf Gesprächsabläufe.** *Parallel* stellt allen Modellen dieselbe Frage isoliert, für einen sauberen Direktvergleich. *Round Robin* lässt sie aufeinander aufbauen. *Debate* verteilt gegensätzliche Positionen. *Moderated* übergibt einem Agenten die Entscheidung, wer als Nächstes spricht. *Consensus* führt eine Diskussion und sammelt danach von allen eine ausdrückliche Stimme ein.
- **Lokale und gehostete Modelle im selben Raum.** Ollama braucht überhaupt keine Zugangsdaten. Daneben lassen sich bis zu 16 Endpunkte eintragen, sodass mehrere Konten und mehrere gehostete Modelle am selben Gespräch teilnehmen können.
- **Eine Implementierung deckt viele Endpunkte ab.** Ollama, die Anthropic Messages API und alles, was den OpenAI-Chat-Completions-Dialekt spricht, also LM Studio, vLLM, llama.cpp, Groq und Together.
- **Zugangsdaten werden nie gespeichert.** Ein Provider-Eintrag enthält den *Namen* einer Umgebungsvariable, nie einen Schlüssel. Nichts Sensibles landet in der Datenbank, in der Konfigurationsdatei oder in einem Backup davon.
- **Modelle nebeneinander lesen.** Im Parallel-Ablauf stehen die Antworten nebeneinander statt untereinander, was einen Vergleich erst wirklich lesbar macht. Jeder Raum lässt sich duplizieren, um dieselbe Besetzung für eine neue Frage zu nutzen, und der Verlauf ist durchsuchbar.
- **Live-Streaming.** Token-Deltas kommen über eine WebSocket-Verbindung, du siehst also jeden Agenten denken, statt auf eine Textwand zu warten.
- **Terminal-Client inklusive.** `hive` legt Räume an, fügt Agenten hinzu, führt Runden aus und exportiert Transkripte, ohne Browser und ohne laufenden Server.
- **Transkripte bleiben bei dir.** Alles landet in einer SQLite-Datei und lässt sich als Markdown exportieren.

## Voraussetzungen

- Rust 1.82 oder neuer und Node.js 22 oder neuer, um aus dem Quelltext zu bauen. Ein Container-Build braucht beides nicht.
- [Ollama](https://ollama.com) für lokale Modelle, oder Zugangsdaten für einen gehosteten Endpunkt.
- Linux, macOS oder Docker. Windows ist ungetestet.

## Schnellstart

### Aus dem Quelltext

```bash
git clone https://github.com/9t29zhmwdh-coder/HiveMind_Chat.git
cd HiveMind_Chat

cp hivemind.example.toml hivemind.toml
(cd frontend && npm ci && npm run build)
cargo build --release

./target/release/hivemind-server
```

Öffne <http://127.0.0.1:8750>, lege einen Raum an, füge zwei Agenten hinzu und frag etwas.

### Mit Docker

```bash
cp .env.example .env      # HIVEMIND_ACCESS_TOKEN und allfällige API-Keys eintragen
docker compose up -d
```

Die Compose-Datei veröffentlicht nur auf Loopback. Bevor du den Port ins Netzwerk freigibst, setze `HIVEMIND_ACCESS_TOKEN`; der Server warnt im Log, wenn er ohne Token an eine Nicht-Loopback-Adresse gebunden wird.

### Aus dem Terminal

```bash
hive providers                                   # was konfiguriert ist, und ob der Schlüssel auflösbar ist
hive models local                                # was der Endpunkt tatsächlich anbietet

ROOM=$(hive new-room "Design Review" --policy debate --rounds 2)
hive add-agent "$ROOM" Scout local llama3:8b --persona "Du bevorzugst Einfachheit."
hive add-agent "$ROOM" Vera  local gemma4    --persona "Du bevorzugst Betriebsreife."
hive chat "$ROOM" "Sollen wir das als eine Binärdatei ausliefern oder als zwei?"
```

### Ein gehostetes Modell hinzufügen

Trag das Konto in `hivemind.toml` ein und benenne dabei die Variable, die den Schlüssel trägt:

```toml
[[providers]]
id = "anthropic-main"
label = "Anthropic"
kind = "anthropic"
api_key_env = "HIVEMIND_KEY_ANTHROPIC"
```

Exportiere den Schlüssel, bevor du den Server startest. Der Wert wird pro Anfrage gelesen und nirgends geschrieben:

```bash
export HIVEMIND_KEY_ANTHROPIC="..."
```

`kind = "openai"` funktioniert genauso für jeden OpenAI-kompatiblen Endpunkt; setze `base_url` entsprechend.

## Dokumentation

- [GETTING_STARTED.md](GETTING_STARTED.md) führt Schritt für Schritt durch den ersten Raum.
- [ARCHITECTURE.md](ARCHITECTURE.md) erklärt, wie aus einer Frage ein Gespräch wird.
- [SECURITY.md](SECURITY.md) behandelt den Umgang mit Zugangsdaten und die Meldung von Schwachstellen.
- [ROADMAP.md](ROADMAP.md) listet Geplantes und bewusst Ausgeschlossenes.

## Deinstallation und Aufräumen

Die Anwendung schreibt an genau zwei Stellen: in die SQLite-Datenbank (standardmässig `hivemind.db`, im Container `/data`) und in `hivemind.toml`. Keine davon enthält Zugangsdaten.

```bash
# Aus dem Quelltext
rm -rf HiveMind_Chat            # enthält hivemind.db und hivemind.toml

# Docker
docker compose down -v          # -v entfernt auch das Daten-Volume
```

Zugangsdaten liegen nur in deiner Umgebung oder in der `.env`-Datei; die entfernst du separat. Ausserhalb des Projektverzeichnisses wird nichts geschrieben, und es wird kein Dienst und kein Launch Agent registriert.

---

**Autor:** [Rafael Yilmaz](https://github.com/9t29zhmwdh-coder) · **Status:** Active · ![version](https://img.shields.io/github/v/release/9t29zhmwdh-coder/HiveMind_Chat?color=6b7280&style=flat-square) · **Lizenz:** MIT
