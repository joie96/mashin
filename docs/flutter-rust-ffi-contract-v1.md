# Mashin Flutter + Rust Architekturvertrag

Version: 1.0.0
Status: Entwurf (implementierungsbereit)
Datum: 2026-08-11
Gueltig fuer: Migration von mashin-maui auf Flutter + Rust mit sendspin-rs

## 1. Ziel und Scope

Dieses Dokument definiert den verbindlichen Vertrag zwischen Flutter (App/UI/Orchestrierung) und Rust (Sendspin-Engine).

Ziele:
- Funktionsparitaet zur bestehenden MAUI-App.
- Klare Ownership pro Schicht.
- Stabile FFI-Schnittstelle trotz moeglicher sendspin-rs API-Aenderungen.
- Event-getriebene, asynchrone Integration ohne UI-Blockaden.

Nicht-Ziele (V1):
- Vollstaendige Android Auto Umsetzung.
- Vollstaendige Desktop-Audiogeraeteverwaltung in Flutter.

## 2. Architekturgrenzen

### 2.1 Flutter Ownership

Flutter implementiert:
- UI, Layout, Navigation, Overlays, Context Menus.
- Session- und Runtime-Orchestrierung (Login, Startup, Offline/Online).
- Music Assistant REST und Event-Websocket.
- Queue-Projektion und Queue-Diff-Logik.
- Favoriten/Playlists/Userdaten und Persistenz.
- Auswahl und Wechsel zwischen Playback-Modi (Sendspin, MA Remote, Local).

### 2.2 Rust Ownership

Rust implementiert:
- Sendspin-Verbindung und Protokoll-Handshake ueber sendspin-rs.
- Audio-Streaming-Pfad: Empfang, Dekodierung, Pufferung, Ausspielung.
- Clock-Sync, Buffer-Stats, Low-Latency State.
- Codec-Negotiation und Formatwechsel.
- Zustands- und Fehler-Events fuer Flutter.

### 2.3 Integrationsprinzip

- Flutter bleibt Source of Truth fuer App-Status und Business-Entscheidungen.
- Rust ist Source of Truth fuer engine-nahe Audio/Transport-States.
- Recovery-Strategien (Queue wiederherstellen, Seek, Resume) werden in Flutter orchestriert.

## 3. Technologie und Paketstruktur

## 3.1 Flutter

Empfohlen:
- State Management: Riverpod (oder Bloc, falls bereits bevorzugt).
- Serialisierung: freezed + json_serializable.
- Persistenz: shared_preferences + ggf. drift/sqflite fuer Snapshots.

Vorgeschlagene Struktur:
- lib/app
- lib/presentation
- lib/application
- lib/data
- lib/domain
- lib/interop/rust

## 3.2 Rust

Empfohlen:
- Bridge: flutter_rust_bridge.
- Async Runtime: tokio.
- Adapter-Crate vor sendspin-rs, um API-Stabilitaet nach aussen zu sichern.

Vorgeschlagene Struktur:
- rust/mashin_core/src/api
- rust/mashin_core/src/engine
- rust/mashin_core/src/adapter_sendspin
- rust/mashin_core/src/model
- rust/mashin_core/src/error

## 4. FFI API Vertrag (V1)

Alle FFI-Methoden sind asynchron und liefern entweder Erfolg oder strukturierten Fehler.

## 4.1 Datentypen

### Core Enums

- ConnectionState: offline | connecting | online | reconnecting
- PlayerStateType: unknown | uninitialized | idle | buffering | playing | paused | seeking | error
- PlayerStateReason: user_action | reconnect | play_media | seek | system
- Codec: opus | flac | pcm

### Core Strukturen

```json
{
  "RuntimeConfig": {
    "sendspinUrl": "string",
    "clientId": "string",
    "clientName": "string",
    "bufferCapacityBytes": "int",
    "preferredCodec": "opus|flac|pcm",
    "initialVolume": "0..100",
    "initialMuted": "bool",
    "supportedFormats": [
      { "codec": "opus|flac|pcm", "sampleRate": 48000, "channels": 2, "bitDepth": 16 }
    ]
  }
}
```

```json
{
  "PlayerSnapshot": {
    "connectionState": "offline|connecting|online|reconnecting",
    "playbackState": {
      "state": "playing|paused|buffering|idle|seeking|error|unknown",
      "reason": "user_action|reconnect|play_media|seek|system|null",
      "activeSinceUtc": "RFC3339"
    },
    "positionSeconds": "double",
    "durationSeconds": "double",
    "bufferedMilliseconds": "double",
    "volume": "0..100",
    "muted": "bool",
    "activeCodec": "opus|flac|pcm|null",
    "serverName": "string|null"
  }
}
```

## 4.2 Commands (Flutter -> Rust)

1. `engineInit(config: RuntimeConfig) -> void`
2. `engineConnect() -> void`
3. `engineReconnect() -> void`
4. `engineDisconnect(reason: string) -> void`
5. `engineTerminate() -> void`
6. `transportPlay() -> void`
7. `transportPause() -> void`
8. `transportNext() -> void`
9. `transportPrevious() -> void`
10. `transportSeek(seconds: double) -> void`
11. `setVolume(volume: int) -> void`
12. `setMuted(muted: bool) -> void`
13. `requestCodec(codec: Codec) -> void`
14. `getSnapshot() -> PlayerSnapshot`

Optional V1.1:
- `switchOutputDevice(deviceId: string|null) -> void`

## 4.3 Events (Rust -> Flutter)

Rust publiziert einen Event-Stream `engineEvents`.

Eventtypen:
1. `connection_state_changed`
2. `playback_state_changed`
3. `position_updated`
4. `buffer_stats_updated`
5. `volume_changed`
6. `mute_changed`
7. `codec_changed`
8. `server_message_observed` (optional Debug)
9. `engine_error`
10. `engine_log` (optional Debug)

Event-Huelle:

```json
{
  "type": "string",
  "atUtc": "RFC3339",
  "correlationId": "string|null",
  "payload": {}
}
```

Positions-Event (Beispiel):

```json
{
  "type": "position_updated",
  "atUtc": "2026-08-11T10:00:00Z",
  "correlationId": null,
  "payload": {
    "positionSeconds": 123.42,
    "durationSeconds": 245.00,
    "bufferedMilliseconds": 510.0
  }
}
```

## 5. Fehlervertrag

## 5.1 Fehlerstruktur

```json
{
  "code": "string",
  "message": "string",
  "isRetryable": true,
  "details": {
    "source": "engine|protocol|audio|ffi",
    "nativeCode": "string|null"
  }
}
```

## 5.2 Fehlercodes (V1)

- `ENGINE_NOT_INITIALIZED`
- `INVALID_CONFIG`
- `CONNECT_FAILED`
- `DISCONNECTED_UNEXPECTED`
- `HANDSHAKE_FAILED`
- `CODEC_NOT_SUPPORTED`
- `AUDIO_DEVICE_ERROR`
- `AUDIO_PIPELINE_ERROR`
- `COMMAND_REJECTED_INVALID_STATE`
- `INTERNAL_PANIC_GUARDED`

Regeln:
- Rust darf keine ungefangenen Panics bis an Flutter durchreichen.
- Jeder Fehler muss einheitlich als strukturierter Error erscheinen.

## 6. Zustandsmodell

## 6.1 Connection-State-Maschine

- offline -> connecting -> online
- online -> reconnecting -> online
- reconnecting -> offline (wenn max. Retry oder fataler Fehler)

## 6.2 Playback-State-Maschine

- idle -> buffering -> playing
- playing -> paused -> playing
- playing -> seeking -> buffering|playing
- jeder State -> error bei fatalem Enginefehler

Regel:
- Flutter darf UI-States nur ueber Events/Snapshot aus Rust aktualisieren, nicht spekulativ.

## 7. Recovery-Strategie (Flutter orchestriert)

Ablauf bei unerwartetem Disconnect:
1. Flutter markiert Session als reconnecting.
2. Rust reconnectet und meldet `connection_state_changed`.
3. Flutter synchronisiert Queue ueber Music Assistant.
4. Flutter stellt wieder her:
- Queue-Inhalt
- Current Index / QueueItem
- Position (seek)
- Play/Pause Zustand
5. Flutter sendet notwendige Transportcommands an Rust.

Wichtig:
- Rust kennt keine Business-Queue-Wiederherstellung als Domainregel.

## 8. Performance- und Threading-Regeln

- Keine blockierenden Calls im Flutter UI-Isolate.
- Rust Audio-Thread darf nicht auf FFI roundtrips warten.
- Position/Buffer Events sollen gedrosselt werden (z. B. 4 Hz), aber on-demand Snapshot bleibt jederzeit moeglich.
- Event-Reihenfolge muss pro Verbindung monoton sein.

## 9. Kompatibilitaet und Versionierung

- SemVer fuer den FFI-Vertrag.
- `contractVersion` wird von Rust beim Init gemeldet.
- Flutter prueft Major-Version:
- Major mismatch: harte Ablehnung mit klarer Fehlermeldung.
- Minor mismatch: erlaubt, wenn Felder optional rueckwaertskompatibel sind.

## 10. Security und Privacy

- Keine Zugangsdaten im Klartext in Logs.
- Tokens verbleiben in Flutter-Persistenz; Rust erhaelt nur, was fuer den Verbindungsaufbau erforderlich ist.
- Debug-Events (`engine_log`, `server_message_observed`) in Production standardmaessig deaktivieren.

## 11. Teststrategie

## 11.1 Rust

- Unit Tests: State-Machine, Codec-Switch, Error-Mapping.
- Integration Tests: sendspin Verbindung, Reconnect, Stream-Continuity.
- Soak Test: Langzeitstreaming mit Speichergrenzen.

## 11.2 Flutter

- Contract Tests gegen Mock-Rust-Bridge.
- Golden Tests fuer Layout-Paritaet.
- Integration Tests fuer Runtime-Flows (Login, Player-Switch, Recovery).

## 11.3 End-to-End

- Reale Sendspin Instanz.
- Reale Music Assistant Instanz.
- Szenarien:
- kalter Start
- Netzwerkverlust waehrend Playback
- Codec-Wechsel waehrend Playback
- Wechsel Sendspin <-> MA Remote

## 12. Umsetzungsreihenfolge

Phase 1:
- Contract finalisieren
- Rust Adapter-Crate + minimaler Connect/Disconnect Pfad
- Flutter interop layer + Snapshot/Events anzeigen

Phase 2:
- Transportcommands + Volume/Mute + Position
- Reconnect mit Flutter-Orchestrierung

Phase 3:
- Queue- und Moduswechsel-Ende-zu-Ende
- Local Mode Strategie finalisieren

Phase 4:
- Android Background/Notification
- Android Auto Paritaet

## 13. Offene Entscheidungen

1. Local Playback: in Flutter belassen oder in Rust vereinheitlichen?
2. Device-Switch API schon in V1 oder erst V1.1?
3. Event-Takt fuer Position/Buffer: 4 Hz vs 10 Hz.
4. Android Auto als eigener Plugin-Track oder spaetere Phase.

## 14. Definition of Done fuer Contract V1

- Flutter und Rust Teams bestaetigen diesen Vertrag.
- Beispielimplementierung von mindestens:
- `engineInit`
- `engineConnect`
- `engineDisconnect`
- `transportPlay`
- `transportPause`
- `setVolume`
- `engineEvents`
- Fehlerformat einheitlich in beiden Richtungen verifiziert.
- Version-Handshake aktiv.
