# Vertrag: Sendspin Cache und Downloader

Status: Verbindlicher Zielvertrag fuer die vollstaendige Umstellung

Dieser Vertrag beschreibt den einzigen zulaessigen Zustand nach der Umstellung des Sendspin-Downloads und des Encoded-Cache. Er ersetzt die bestehende Fenster-, Retention- und Cache-Eviction-Mechanik vollstaendig. Es gibt keine Kompatibilitaetsadapter, Fallbacks, parallelen Schreibpfade oder weiter unterstuetzten Alt-APIs.

## Ziele und Besitzgrenzen

Der Cache ist ein dummer, synchronisierter Speicher fuer kodierte Audio-Chunks. Er trifft keine fachlichen Entscheidungen ueber Download-Reihenfolge, Prioritaet, Cache-Kapazitaet, Eviction, Wiedergabe-Fenster oder Download-Slots.

Der Downloader besitzt die gesamte Download-Planung. Er erstellt den Plan aus einem Queue-Snapshot, schaetzt Speicherbedarf, verwaltet die Cache-Grenze, bestimmt zu loeschende Eintraege und steuert die beiden Sendspin-Download-Slots.

Die Sendspin-Engine besitzt die Wiedergabe-Queue und den Wiedergabe-Cursor. Sie uebergibt beide als Snapshot an den Downloader und liest Cache-Chunks fuer den Audio-Player. Sie erstellt keine Download-Prioritaeten und verwaltet keine Cache-Retention.

## Cache-Vertrag

Der Cache verwaltet eine Liste von Cache-Schluesseln. Jeder Schluessel verweist auf genau einen `CacheEntry`.

```rust
struct CacheEntry {
    state: CacheStatus,
    format: Option<AudioFormat>,
    chunks: VecDeque<EncodedCacheChunk>,
    first_chunk_index: usize,
    cached_duration_millis: u64,
    byte_len: usize,
}

struct EncodedCacheChunk {
    data: Arc<[u8]>,
    start_position_millis: u64,
    duration_millis: u64,
}
```

Der Cache stellt genau diese fachlichen Mutationen bereit:

```rust
add_key(key, initial_state)
remove_key(key)
append_chunk(key, format, duration_millis, data)
remove_chunk(key, Front | Back)
```

Zulaessige Hilfsmethoden lesen Daten oder fuehren rein mechanische Aenderungen auf einem explizit benannten Eintrag aus:

```rust
contains_key(key)
keys()
entry(key)
total_bytes()
bytes_for_key(key)
set_state(key, state)
read_format(key)
read_chunk(key, index)
local_seek_target(key, position_millis)
trim_front_through_position(key, played_position_millis, target_total_bytes)
```

`append_chunk` legt keinen fehlenden Eintrag an, prueft keine globale Kapazitaet und entfernt keine Eintraege. Der Aufrufer muss den Schluessel vorher mit `add_key` anlegen. Der Cache gibt bei jedem Schreibvorgang die veraenderte Eintrags- und Gesamtnutzung zurueck, damit der Downloader Kapazitaetsdruck feststellen kann.

Der Cache darf weder `CacheRetentionPolicy` noch Download-Fenster, Prioritaeten, LRU-Raenge oder Eviction-Hilfen enthalten. Folgende bestehende Konzepte und ihre Tests werden entfernt:

- `CacheRetentionPolicy`
- `cache_evict_to_fit`
- `cache_evict_before_next_to_fit`
- `cache_discard_interrupted_outside_window`
- `eviction_rank`
- implizites Anlegen oder Kapazitaetsablehnen in `cache_append_encoded_chunk`

Der Integritaetszustand eines Eintrags bleibt Teil der Cache-Daten. `Partial`, `Interrupted` und `Complete` sind Datenzustaende; ihre fachliche Behandlung entscheidet ausschliesslich der Downloader.

## Queue-Snapshot und Downloadplan

Der Downloader stellt als einzige fachliche Planungsschnittstelle bereit:

```rust
apply_download_plan(DownloadPlanRequest) -> Result<(), String>
```

```rust
struct DownloadPlanRequest {
    queue_revision: u64,
    queue_items: Vec<DownloadQueueItem>,
    current_index: Option<usize>,
}
```

Der Request ist ein vollstaendiger Snapshot. Der Downloader speichert daraus seinen autoritativen, intern erzeugten Plan; die Engine liefert keinen vorbereiteten Plan, keine Rollen und keine Prioritaeten.

`create_download_plan(request)` erstellt fuer jedes Queue-Item einen Eintrag mit mindestens Queue-ID, Queue-Index, Cache-Key, prioritaet, geschaetzter Byte-Groesse, erwarteter Dauer und `MediaItem`. Der Cache-Key beschreibt den Provider, das Element und das aktive Codec-Format vollstaendig.

Die maximale Prioritaet ergibt sich fuer $n = max(queue_items.len(), 1)$ aus:

$$
maximum = 10^{\lfloor \log_{10}(n) \rfloor + 2}
$$

Damit gelten beispielsweise $n = 11 \Rightarrow maximum = 1000$ und $n = 567 \Rightarrow maximum = 10000$.

Die Prioritaeten werden eindeutig vergeben:

| Queue-Position relativ zu `current_index` | Prioritaet |
| --- | --- |
| Aktueller Titel | `maximum` |
| Naechster Titel | `0.9 * maximum` |
| Vorheriger Titel | `0.8 * maximum` |
| Weitere zukuenftige Titel | gleichmaessig fallend von `0.7 * maximum` bis `0.5 * maximum + 1` |
| Weiter zurueckliegende Titel | gleichmaessig fallend von `0.5 * maximum - 1` bis `1` |

Die Berechnung verwendet ganzzahlige, deterministische Rundung und darf keine gleiche Prioritaet fuer zwei Queue-Elemente erzeugen. Bei fehlendem aktuellen Index hat kein Element eine Wiedergabe-Sonderrolle; der Plan wird in Queue-Reihenfolge mit eindeutig fallenden Prioritaeten erstellt und startet keine Downloads, bis ein aktueller Titel bestimmt ist.

## Speicherprognose

Vor jeder Slot-Zuweisung berechnet der Downloader `estimate_cache_size(item, format_profile)`.

```rust
struct CacheFormatProfile {
    codec: SendspinAudioCodec,
    sample_rate: u32,
    channels: u8,
    bit_depth: u8,
    encoded_bitrate_bits_per_second: Option<u32>,
}
```

- PCM wird aus Dauer, Abtastrate, Kanaelen und Bittiefe berechnet.
- Opus verwendet eine konfigurierte Bitrate einschliesslich eines konservativen Container-Overheads.
- FLAC verwendet eine konservative obere Schaetzung auf Basis des PCM-Formats.
- Ohne bekannte Dauer ist ein Element nicht vorab zulassbar. Nur der aktuelle Titel darf nach Bereinigung im `Zero`-Modus erneut bewertet werden.

Weicht das beim Stream-Start bestaetigte Format vom Profil ab, verwirft der Downloader den betroffenen Cache-Eintrag, aktualisiert die Prognose und plant erneut. Er behaelt keinen Eintrag mit einer falschen Formatannahme.

## Kapazitaets- und Slot-Vertrag

Der Downloader besitzt genau zwei Sendspin-Slots. Eine Reservation ist nur fuer ihre Lease-ID gueltig. Jeder abgebrochene, geloeschte oder ersetzte Plan-Eintrag invalidiert seine Reservation vor dem Entfernen des Cache-Eintrags.

```rust
enum DownloadCapacityMode {
    Dual,
    Single { protected_key: CacheContentKey },
    Zero,
}
```

- `Dual`: Bis zu zwei hoechstpriorisierte, zulaessige, unvollstaendige Plan-Eintraege duerfen laden.
- `Single`: Genau ein Slot darf starten. Der `protected_key` ist der wichtigere Eintrag, dessen Abschluss die Rueckkehr zu `Dual` ausloest.
- `Zero`: Kein Slot darf starten. Laufende Slots werden beendet, bevor der Modus aktiv wird.

Eine geplante Aufnahme ist nur zulaessig, wenn ihre prognostizierte Groesse zusammen mit der aktuellen Cache-Nutzung die Cache-Grenze nicht ueberschreitet. Der Downloader berechnet seine Auswahl immer aus dem vollstaendigen aktuellen Plan und nicht aus einem separaten Download-Fenster.

Bei einer realen Cache-Nutzung von mindestens $95\%$ beginnt Kapazitaetsdruck. Ziel jeder Bereinigung ist eine Nutzung von hoechstens $90\%$. Nach jedem Entfernen wird die Zielgrenze erneut geprueft.

Die Bereinigung erfolgt zwingend in dieser Reihenfolge:

1. Alle Cache-Eintraege entfernen, deren Schluessel nicht im aktuellen Plan vorkommt.
2. Plan-Eintraege in aufsteigender Prioritaet durchlaufen und vorhandene Cache-Eintraege entfernen.
3. Soll ein aktiver Download entfernt werden, dessen Reservation invalidieren, den Sendspin-Stream stoppen, den Cache-Eintrag entfernen und `Single` fuer den hoechstpriorisierten verbleibenden Plan-Eintrag aktivieren.
4. Besteht im `Single`-Modus weiter Druck und laedt der aktuelle Titel, nur bereits wiedergegebene Chunks am Anfang dieses Eintrags entfernen, bis die Zielgrenze erreicht ist oder keine entfernbaren Chunks mehr bestehen.
5. Besteht im `Single`-Modus weiter Druck und laedt ein nicht aktueller Titel, diesen Download abbrechen, seinen Cache-Eintrag entfernen und `Zero` aktivieren.

`Single` endet nur nach Abschluss des geschuetzten Downloads und anschliessender erneuter Plan-Anwendung. `Zero` endet nur bei einer erneuten `apply_download_plan`-Bewertung, wenn die Nutzung unter der Wiederaufnahmegrenze liegt. Es gibt keinen Timer, keinen Keepalive und keine automatische Wiederaufnahme ausserhalb dieses Ablaufs.

Ueberschreitet ein Chunk trotz Prognose die harte Cache-Grenze, wird dieser Chunk nicht gespeichert. Der Downloader fuehrt unmittelbar dieselbe Bereinigung aus und entscheidet danach aus dem aktuellen Plan, ob der Stream fortgesetzt oder abgebrochen wird.

## Engine-Integration

Die Engine ist fuer Queue-Mutationen, Titelwechsel, Wiedergabe und UI-Snapshots verantwortlich. Nach jeder Queue-Aenderung und nach jedem Titelwechsel erstellt sie einen vollstaendigen `DownloadPlanRequest` und ruft ausschliesslich `downloader.apply_download_plan(request)` auf.

Die Engine meldet die aktuelle Wiedergabeposition nur mit:

```rust
downloader.record_playback_position(content_key, position_millis) -> Result<(), String>
```

Der Downloader verwendet diese Position nur im `Single`-Modus, um ausschliesslich bereits wiedergegebene Chunks des aktuellen Titels zu trimmen. Die Engine bleibt autoritativ fuer die Wiedergabeposition; der Downloader speichert oder verwaltet keinen Playback-Cursor. Die Positionsmeldung ist kein Retention-Fenster und keine zweite Planungs-API.

Die Cache-Zustandsanzeige fuer Queue-Elemente wird vom Downloader aus dem aktuellen Plan und den Cache-Eintraegen abgeleitet. `Prepared`, `Buffering`, `Downloading`, `Complete`, `Interrupted`, `Failed`, `Evicted` und `Discarded` duerfen nur fuer Eintraege des aktuellen Plans veroeffentlicht werden.

## Zu entfernende Altmechanik

Die Umstellung ist erst abgeschlossen, wenn folgende APIs, Datenmodelle, Felder, Commands, Aufrufe und Tests entfernt oder auf die neue Schnittstelle umgestellt sind:

- `SendspinDownloadWindow`
- `SendspinPlaybackProgress`
- `replace_download_window`
- `update_playback_progress`
- `sync_download_window`
- `recently_played_content_keys`
- `SendspinDownloadRole`, sofern es nur zur alten Fensterlogik verwendet wird
- die Engine-Methode `build_queue_download_plan`
- jede Cache-Eviction- oder Retention-Policy
- Cache-Pause-Keepalive-Tasks und deren Resume-Logik
- jede implizite Wiederaufnahme eines durch Kapazitaetsdruck abgebrochenen Downloads

`set_pending_current_seek` und `seek` bleiben nur erhalten, wenn sie ausschliesslich die neue Reservation, den neuen Cache-Eintrag und den neuen Plan aktualisieren. Sie duerfen keine alte Fenster- oder Retention-Information lesen oder schreiben.

## Verifikation

Die Umstellung erfordert mindestens diese fokussierten Tests:

- Cache legt Eintraege nur explizit an, entfernt Schluessel und Chunks korrekt und fuehrt keine globale Policy aus.
- Der Downloadplan berechnet fuer 11 und 567 Eintraege die Maxima 1000 beziehungsweise 10000.
- Die Prioritaetsreihenfolge ist fuer aktuelle, naechste, vorherige, zukuenftige und historische Elemente eindeutig.
- Die Schaetzung ist fuer PCM reproduzierbar und fuer Opus sowie FLAC konservativ.
- Verwaiste Eintraege werden vor Plan-Eintraegen entfernt.
- Kapazitaetsdruck entfernt Plan-Eintraege strikt von der niedrigsten Prioritaet aufwaerts.
- Das Entfernen eines aktiven Downloads aktiviert `Single` und verhindert dessen sofortige Neuaufnahme.
- Anhaltender Druck im `Single`-Modus trimmt nur bereits abgespielte Chunks des aktuellen Titels.
- Anhaltender Druck mit einem nicht aktuellen Slot aktiviert `Zero` und blockiert beide Slots.
- Ein neuer Queue-Snapshot kann `Zero` nur bei eingehaltenem Wiederaufnahme-Limit aufheben.
- Reconnect, Codec-Wechsel und Seek invalidieren alte Leases und koennen keine stale Chunks in einen entfernten Cache-Eintrag schreiben.

Jede neue oder angepasste Sendspin- und Cache-Schnittstelle wird im Rust-Modul sowie in allen FFI-, JNI-, Dart- und Test-Aufrufern innerhalb derselben Aenderung umgestellt. Ein verbliebener Alt-Aufrufer ist ein fehlgeschlagener Migrationsabschluss.