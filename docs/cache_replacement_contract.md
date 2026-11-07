# Vertrag: Serverorientierter Medien- und Bildcache

Status: Verbindliches Zielmodell fuer die Implementierung
Datum: 2026-11-07
Geltungsbereich: Rust, Dart, Kotlin, native Bindings, Tests und Build-Konfiguration von Mashin

## 1. Zweck und Verbindlichkeit

Dieser Vertrag definiert eine lokale SQLite-Medienbibliothek und einen persistenten Bildcache nach den Funktionsprinzipien des Music-Assistant-Servers. Er legt Zielmodell, Verantwortlichkeiten, Sicherheitsanforderungen und Abnahmenachweise fuer die Implementierung fest.

Die Umsetzung erfolgt auf gesonderten Implementierungsauftrag anhand der hier beschriebenen Anforderungen.

## 2. Schutz bestehender Nutzerdaten

- Der lokale Mashin-Cache-Ordner darf fuer die in Abschnitt 4 definierten Cache-Leerungen vollstaendig geloescht werden, einschliesslich Bilddateien, Cache-Datenbanken, SQLite-Sidecars und temporaerer Cache-Dateien. Diese Leerungen sind ausdruecklich autorisiert und benoetigen keine weitere Freigabe.
- Zugangsdaten, Einstellungen, Logs und Serverdaten bleiben geschuetzt und duerfen nicht geloescht werden. Die Freigabe umfasst weder das uebergeordnete App-Datenverzeichnis noch andere Ordner ausserhalb des nachgewiesenen Cache-Ordners.
- Vor einer Bereinigung werden Cache-Worker, Downloads und sonstige Cache-Nutzer beendet und alle Datei-/DB-Handles geschlossen. Der Zielpfad und seine Ownership werden geprueft; Symlinks oder Junctions duerfen keine Loeschung ausserhalb des Cache-Ordners bewirken. Bei unklarer Abgrenzung zu geschuetzten Daten wird nicht geloescht, sondern nachgefragt.
- Tests und Abnahmen verwenden ausschliesslich isolierte Testverzeichnisse und synthetische Daten.
- Cache-Dateien erhalten eine explizite Formatidentitaet und ein cacheeigenes Namensschema.
- Dateien ausserhalb des nachgewiesenen Cache-Ordners brauchen fuer ihre Loeschung einen eigenen ausdruecklichen Auftrag.

## 3. Servervorbild

Massgeblich sind folgende lokal gepruefte Serverprinzipien, nicht dessen interne DB-Version als Clientvertrag:

- [Bibliotheksschema](../../music-assistant-server/music_assistant/controllers/music/database.py): relationale Medientabellen und Beziehungen, JSON fuer zusammengesetzte Metadaten.
- [Mediencontroller](../../music-assistant-server/music_assistant/controllers/music/media/base.py): Library-Aufloesung vor Providerzugriff, getrennte Summary-/Detail-Abfragen, SQL-Filter und Pagination.
- [Playlistcontroller](../../music-assistant-server/music_assistant/controllers/music/media/playlists.py): Playlistidentitaet und Inhaltsquelle sind getrennt; Listen haben eigenen Kontext und Reihenfolge.
- [Builtin-Provider](../../music-assistant-server/music_assistant/providers/builtin/__init__.py): gespeicherte Eintragsmetadaten erlauben Anzeige vor weiterer Detailaufloesung; erzeugte Playlists verwenden gezielte Bibliotheksabfragen.
- [Bibliotheksempfehlungen](../../music-assistant-server/music_assistant/providers/recommendations/__init__.py): einzelne Abschnitte werden unabhaengig geladen.
- [Bildproxy](../../music-assistant-server/music_assistant/controllers/metadata/images.py) und [Bildhelper](../../music-assistant-server/music_assistant/helpers/images.py): Quellenidentitaet, Bildbytes und Darstellungsvarianten sind getrennt; gleiche laufende Abrufe werden geteilt.

Das Serverschema wird nicht unveraendert kopiert. Der lokale Cache enthaelt beobachtete Daten, nicht die vollstaendige Serverbibliothek. Das stabile externe Vorbild sind die unterstuetzten API-Befehle und Modelle der verbundenen Serverversion.

### Verbindliche Quellcodepruefung vor der Umsetzung

- Vor jedem fachlichen Implementierungsschritt muss der dazugehoerige Servercode tatsaechlich eingelesen werden. Die Kurzbeschreibungen dieses Vertrags, fruehere Chatantworten, Erinnerungsnotizen und vermutetes Serververhalten reichen nicht als Implementierungsgrundlage.
- Die Pruefung verfolgt den konkreten Pfad von der API-Registrierung ueber den Controller bis zur entscheidenden SQL-Abfrage, Providerfunktion oder Cache-/Bildfunktion. Relevante aufgerufene Helper, Modelldefinitionen und vorhandene Verhaltenstests werden ebenfalls gelesen; allein Signatur, Docstring oder Dateiliste reichen nicht.
- Fuer DB-Schritte werden die betroffenen Tabellen, Spalten, Indizes und produktiven Lese-/Schreibabfragen gelesen. Fuer Playlists werden Library-Aufloesung und Builtin-Inhaltsbereitstellung geprueft; fuer Bilder Proxy-Aufloesung, Quellen-/Variantencache und Invalidierung.
- Parameterdefaults, Filter, Sortierung, Pagination, Feldabwesenheit, Benutzerkontext, Fehler und Refresh-Verhalten werden aus diesem Code abgeleitet. Abweichungen fuer den lokalen Cache werden ausdruecklich begruendet und nicht als identisches Serververhalten bezeichnet.
- Pro Implementierungsschritt wird ein kurzer Quellennachweis mit Server-Commit bzw. Version, gelesenen Dateien und Symbolen sowie den daraus abgeleiteten Vertrags-/Testfaellen festgehalten. Uncommittete relevante Serveraenderungen werden kenntlich gemacht. Nach Sitzungswechsel darf nicht allein eine alte Zusammenfassung diesen Nachweis ersetzen; der fuer den naechsten Schritt relevante Code wird erneut gelesen.
- Wenn der erforderliche Servercode nicht verfuegbar ist oder sein Verhalten dem Zielvertrag widerspricht, wird der betroffene Schritt angehalten und die offene Entscheidung mit dem Auftraggeber geklaert. Es wird kein Ersatzverhalten aus der knappen Beschreibung erfunden.
- Fehlende Quellcodepruefung oder fehlender Quellennachweis verhindert die Abnahme des betroffenen Implementierungsschritts.

## 4. Ownership und Ausfuehrung

- Eine lokale Leseschicht und ein Remote-Adapter implementieren denselben typisierten fachlichen Lesevertrag.
- Eine Koordination entscheidet ueber lokale Anzeige und serverseitigen Refresh. Die UI greift weder direkt auf SQL noch auf Bilddateien oder Provider zu.
- SQLite besitzt die bestaetigten Medien, Beziehungen und Cache-Zustaende. RAM enthaelt nur begrenzte Arbeitsdaten, laufende Anfragen und Benachrichtigungen, keine zweite bestaetigte Medienbibliothek.
- Ein dedizierter Writer fuehrt atomare Transaktionen aus. Eine begrenzte Zahl eigener Read-Verbindungen liest konsistente WAL-Snapshots unabhaengig von der Writer-Warteschlange.
- Blocking SQL, Datei-I/O, Dekodierung und Bildverarbeitung laufen nicht auf dem UI-Thread oder Async-Executor.
- Dateilesen und Bilddekodierung warten nicht auf umfangreiche Medien-Schreiboperationen. SQL-Indizes fuer Bilder werden nur fuer kurze gezielte Abfragen verwendet.
- Akzeptierte Schreiboperationen und Datei-Publikationen haben einen definierten Abschluss. Shutdown stoppt neue Arbeit, beendet Downloads und wartet auf laufende Arbeit, bevor Handles geschlossen werden.
- Ein spaetes Ergebnis einer alten Sitzung darf die aktive Ansicht einer anderen Sitzung nicht aktualisieren.

Die cachebezogenen Rust-Dateien (`.rs`) sollen hauptsaechlich im Daten-Unterordner `rust/mashin_engine/src/data/` und dort unter `cache/` angelegt werden. Dies ist die bevorzugte Ablage, keine starre Verzeichnisvorgabe; auf Anfrage kann davon abgewichen werden.

### Verantwortlichkeiten des Datenladepfads

| Schicht | Verantwortung | Abgrenzung |
| --- | --- | --- |
| UI und Seitencontroller | Fachliche Daten anfordern, Ergebnisse und Lade-/Fehlerzustaende darstellen, Beobachtung an Sichtbarkeit und Seiten-Lifecycle binden | Kein direkter SQL-, Datei- oder Providerzugriff und keine eigene Online-/Cache-Auswahl |
| Anwendung | Benoetigte Auswahl, Benutzer-/Serverkontext, Prioritaet und Abbruchkontext festlegen; fachliche Berechnungen aus gelieferten Modellen ausfuehren | Kein zweiter bestaetigter Cachebestand und kein eigener persistenter Schreibpfad |
| Lade-Koordination | Lokal zuerst lesen, Kenntnisstand und Frische bewerten, erforderlichen Remote-Refresh starten und gemeinsame laufende Refreshes koordinieren | Keine SQL-/HTTP-Implementierung und kein zweiter Netzwerkdispatcher |
| Lokaler Leseadapter | Angeforderte Auswahl gezielt aus SQLite lesen und typisierte Modelle samt internem Kenntnisstand liefern | Kein eigener Serverabruf; Filter und Pagination vor Modellrekonstruktion |
| Remote-Leseadapter | Den entsprechenden Serverbefehl ausfuehren und dessen Antwort typisiert verarbeiten | Derselbe fachliche Lesevertrag wie lokal; Netzwerk-Leseanfragen laufen durch den bestehenden Prioritaetsdispatcher |
| Cache-Schreibowner | Erfolgreiche Serverbeobachtungen atomar speichern und nach durablem Abschluss betroffene Abfragen benachrichtigen | Keine Entscheidung ueber UI-Bedarf oder Netzwerkprioritaet; kein vollstaendiger Bibliotheksreadback |
| Bildcache-Dateiowner | Vorhandene Bilder liefern, erforderliche Proxyabrufe koordinieren und gepruefte Dateien sicher publizieren | Getrennt vom Medien-Writer; keine zweite Bildcache-Implementierung in der UI |

Der Pfad lautet: UI/Seitencontroller -> Anwendung -> Lade-Koordination -> lokaler Leseadapter; bei erforderlichem Online-Abruf zusaetzlich Remote-Leseadapter -> bestehender Prioritaetsdispatcher -> MA-Server -> Cache-Schreibowner -> gezielte Benachrichtigung und erneuter lokaler Read. Die Anwendung erhaelt einen gemeinsamen fachlichen Ladezugang, nicht zwei getrennte UI-Aufrufpfade fuer online und lokal. Diese Verantwortlichkeiten sind keine Vorgabe, fuer jeden Tabelleneintrag ein eigenes Modul anzulegen.

- Eine aktive Leseoperation kann zuerst bestaetigte lokale Daten und danach aktualisierte Daten liefern. Daten, Kenntnisstand und Refresh-/Fehlerzustand werden typisiert beobachtbar gemacht; ein einziges Future, das vorhandene Daten bis zum Ende des Server-Refreshs zurueckhaelt, erfuellt Cache-first nicht. Eine Benachrichtigung allein beweist noch nicht den Abschluss des Refreshs.
- Bei lokalem Miss wird Ladezustand geliefert; offline bleibt der Miss ausdruecklich unbekannt statt leer. Refresh-Fehler erhalten vorhandene Daten. Nach Commit wird nur die weiterhin aktive betroffene Auswahl neu gelesen; alte Sitzungen und geschlossene Seiten erhalten keine spaeten UI-Aktualisierungen.
- Playlist-Inhalte fuer Anzahl und Gesamtdauer duerfen beim Startup als Hintergrundarbeit ueber denselben Ladezugang angefordert werden. Die Anwendung berechnet diese Werte aus den gelieferten Eintraegen unter Beachtung ihrer Vollstaendigkeit; eigene persistierte Summenfelder sind zunaechst nicht erforderlich. Die Playlistlistenabfrage selbst laedt weiterhin keine Tracklisten mit.
- Der bestehende Prioritaetsdispatcher und sein Prioritaetsmodell werden weiterverwendet. Prioritaet ist interner Ausfuehrungskontext, kein neuer MA-Wire-Parameter. Sie wird durch Lade-Koordination, Remote-Adapter und zusammengehoerige technische Folgeabrufe erhalten. Startup-Nachladen ist Hintergrundarbeit; aktive Detailseiten erhalten Vordergrundprioritaet. Bestehende Startup-Regeln und Parallelitaetsgrenzen werden nicht durch einen neuen Cachedispatcher umgangen.
- Identische, semantisch teilbare Refreshes desselben Server-/Benutzer-/Abfragekontexts teilen Abruf, Mapping und Commit. Tritt ein Vordergrundverbraucher hinzu, wird die gemeinsame Lesegruppe auf dessen Prioritaet angehoben, statt einen zweiten Refresh zu starten. Unterschiedliche dynamische Stichproben oder nicht vereinbare Refresh-Anforderungen werden nicht zusammengelegt.
- Jede Beobachtung besitzt einen Abbruchkontext. Das Schliessen einer Seite beendet deren Teilnahme, nicht die anderer Verbraucher. Wenn kein Verbraucher oder ausdruecklicher Anwendungsauftrag den Abruf mehr benoetigt, wird die verbleibende Netzwerkarbeit abgebrochen. Bereits akzeptierte Schreiboperationen behalten ihren definierten Abschluss; Serverwechsel und Leerung verhindern trotzdem eine spaete Publikation alter Ergebnisse.
- Lokale Reads warten nicht in der Netzwerk-Warteschlange und nicht auf das Ende aller Startup-Abrufe. Mapping und Blocking I/O laufen ausserhalb des UI-Threads und Async-Executors. Hintergrund-Commits duerfen Vordergrund-Reads nicht sekundenlang blockieren; laufende atomare Transaktionen werden nicht zur Prioritaetswechsel-Unterstuetzung unterbrochen.

### Serverdatei und Cache-Leerung

- Je Server wird eine eigene SQLite-Datenbankdatei verwendet. Die geoeffnete Datei legt den Serverkontext fuer alle Tabellen und Beziehungen fest; wiederholte Server-Spalten in jedem Primaer- und Fremdschluessel sind nicht erforderlich. Die Serveridentitaet und ihre Abgrenzung zu blossen Adresswechseln werden vor der Umsetzung anhand des Servercodes festgelegt.
- Es wird nur der Cache des aktuell ausgewaehlten Servers behalten. Beim Wechsel zu einem anderen Server werden die bisherige Datenbank einschliesslich SQLite-Sidecars und temporaerer DB-Dateien sowie dessen Bildcache geloescht, bevor der neue Servercache aktiviert wird. Ein Wiederverbinden mit demselben Server oder ein Benutzerwechsel allein loest diese Leerung nicht aus. Beim spaeteren Zurueckwechseln wird der Cache neu aufgebaut, nicht wiederhergestellt.
- Es gibt getrennte fachliche Funktionen `clear_database_cache` und `clear_image_cache`. Sie sind sowohl intern aufrufbar als auch fuer spaetere UI-Commands ueber die aktiven Bindings bereitzustellen; eine neue UI wird durch diesen Vertrag allein nicht beauftragt. Der Serverwechsel verwendet dieselben Funktionen, keine eigene zweite Loeschimplementierung.
- `clear_database_cache` loescht die Cache-Datenbank samt Sidecars und temporaerer DB-Dateien und initialisiert bei weiterhin aktivem Server eine leere Datenbank. Bilddateien bleiben bei diesem einzelnen Aufruf erhalten. `clear_image_cache` leert Bilddateien, temporaere Bilddateien, cacheeigene Bild-RAM-Daten und die dazugehoerigen Cache-Zustaende; Medien und deren serverseitige Bildreferenzen bleiben erhalten. Die fuer vorhandene Bilddateien erforderliche Zuordnung darf nicht allein durch das Leeren der Mediendatenbank verloren gehen; ihr Lifecycle wird vom Medienbestand getrennt.
- Vor dem Leeren wird neue Arbeit fuer den betroffenen Cache gesperrt. Laufende Abrufe und Worker werden beendet oder bis zum definierten Abschluss abgewartet; Handles und Dateileases werden freigegeben. Bereits gestartete Ergebnisse werden invalidiert und duerfen nach dem Leeren keine alten Daten oder Dateien erneut publizieren. Beim Serverwechsel betrifft dies beide Caches.
- Die Funktionen sind idempotent und behandeln bereits fehlende Dateien als geleert. Pfadpruefung und Nutzerdatenschutz aus Abschnitt 2 gelten auch fuer diese Laufzeitfunktionen. Loeschfehler werden explizit gemeldet; ein fehlgeschlagener Serverwechsel aktiviert keinen vermischten Cachezustand. Nach erfolgreicher Leerung werden aktive Ansichten gezielt benachrichtigt; neue Daten entstehen nur durch neue regulaere Abrufe.
- Zunaechst gibt es keine eigenen Cache-Budgets, Medienanzahl-/Byte-Quoten oder budget-/altersgesteuerte Eviction fuer Datenbank, Bilddateien oder Bild-RAM. Vollstaendige Leerung erfolgt nur beim Serverwechsel oder durch ausdruecklichen Aufruf der beiden Funktionen. Fachliche Aktualisierung, Invalidierung, Ersatz alter Varianten und Bereinigung unvollstaendiger Absturzreste bleiben davon getrennt; Frischeablauf allein loescht keine gueltigen Inhalte. Begrenzte Arbeitsparallelitaet und sichere Dekodierung sind keine Speicherquoten.

## 5. Medien-Datenmodell

Alle Tabellen und eindeutigen Schluessel gelten innerhalb der Datenbankdatei des ausgewaehlten Servers. Benutzerdaten und benutzerspezifische Ergebnislisten beruecksichtigen zusaetzlich die Benutzer-ID dieses Servers. Die Zuordnung ist Datenpartitionierung; lokale Cache-Lesezugriffe erfordern keine Serverauthentifizierung.

| Tabelle oder Gruppe | Verantwortung |
| --- | --- |
| Servermetadaten der Datei, `users` | Identitaet des zugehoerigen Servers und dessen Benutzer-IDs ohne Zugangsdaten |
| `artists`, `albums`, `tracks`, `playlists` | Identitaet, Name, Sortiername und typspezifische Attribute |
| `radios`, `audiobooks`, `podcasts`, `podcast_episodes` | Weitere von aktiven Verbrauchern benoetigte Medientypen |
| `provider_manifests`, `provider_instances` | Beobachtete Providerinformationen, Icons und Verfuegbarkeit |
| `provider_mappings` | Medienzuordnung zu Domain, Instanz und Provider-Item-ID; Audioformat als typisiertes JSON-Objekt |
| `external_id_lookup` | Externe Identitaeten zur gezielten Aufloesung |
| `track_artists`, `album_artists`, `album_tracks` | Geordnete Beziehungen zwischen Medien |
| `library_entries` | Bestaetigte Bibliothekszugehoerigkeit, getrennt von bloss im Cache vorhandenen Medien |
| `favorites`, `media_progress`, `play_history` | Bestaetigte benutzerspezifische Bewertung, Fortschritt und Verlauf |
| `playlist_items`, `playlist_content_states` | Playlist-Vorkommen und Kenntnisstand ihrer Inhalte |
| `result_sets`, `result_items`, `recommendation_sections` | Beobachtete geordnete Katalog- und Empfehlungsergebnisse |
| `cache_read_states`, `cache_read_ranges` | Kontext, Frische, Vollstaendigkeit und geladene Bereiche einer Auswahl |
| `image_sources`, `image_variants` | Bildquellen und persistente Darstellungen gemaess Abschnitt 9 |

Die Tabelle ist der fachliche Schemakern, kein Auftrag fuer ungenutzte Tabellen. Weitere Typen wie Genres werden mit ihren aktiven API-Verbrauchern ergaenzt und vertraglich geprueft.

### Attribute und Identitaeten

- Filter-, Join- und Sortierschluessel stehen in indexierbaren Spalten. Zusammengesetzte Metadaten, Playlist-Zugriff und Audioformate werden als modellkonformes JSON gespeichert.
- JSON wird ueber strukturierte Parser und typisierte Modelle verarbeitet, nicht ueber String-Manipulation. Es gibt keinen generischen Rohantwort-/Request-Blob als Ersatz fuer das Fachschema.
- Vom Server gelieferte IDs werden unveraendert als fachliche Identitaeten und Beziehungsschluessel verwendet. Es werden keine eigenen Medien-, Provider-, Benutzer- oder Bildquellen-IDs und keine zusaetzlichen Zuordnungstabellen von lokalen Ersatz-IDs zu Server-IDs eingefuehrt.
- Serverbibliotheks-ID und Provider-Item-ID bleiben verschiedene Begriffe. Innerhalb der Serverdatei werden Bibliotheksobjekte durch Medientyp und Bibliotheks-ID identifiziert; Providerobjekte durch Medientyp, den vom Server verwendeten Providerkontext und Provider-Item-ID. Der Serverkontext ergibt sich aus der Datei, nicht aus einer lokalen Medien-ID. Gleiche IDs in unterschiedlichen Kontexten werden nicht gleichgesetzt. Vom Server gelieferte `provider_mappings` bleiben fachliche Beziehungen, keine lokal erfundene Identitaetszuordnung.
- Wo der Server keine eigene ID liefert, werden vorhandene Kontextmerkmale und stabile Positionen als zusammengesetzte Schluessel verwendet, etwa fuer Abfrageergebnisse, Beziehungsvorkommen und Bildvarianten. Eine generierte lokale Ersatz-ID wird nicht allein fuer einfachere Joins eingefuehrt. Inhalts-Hashes identifizieren Bildbytes, nicht Medien oder Serverobjekte.
- Providerobjekte werden durch das Cachen nicht automatisch in die Bibliothek aufgenommen. `library_entries` wird nur aus bestaetigten Mitgliedschaftsinformationen gepflegt.
- Feldabwesenheit in Summaries bedeutet unbekannt, nicht leer oder geloescht. Eine Summary darf bekannte Details nicht durch synthetische Defaults ersetzen.
- Explizites `null`, fehlendes Feld und bestaetigte leere Beziehung bleiben dort unterscheidbar, wo sie unterschiedliche API-Bedeutung haben. Die dafuer erforderliche Feld-/Beziehungskenntnis wird persistent erhalten.
- Favoriten sind dreistufig: positiv, negativ oder ausdruecklich unbewertet; fehlende Kenntnis ist kein negatives Rating.
- Fremdschluessel, Unique Constraints und stabile Ordnung sichern die aktiven Beziehungen. Fuer produktive Filter und Joins werden passende Indizes und Query-Plan-Nachweise verlangt.

## 6. Playlists und Katalogergebnisse

- Playlist-Summary, Playlist-Details und Playlist-Inhalte sind unabhaengige Abfragen. Eine Playlistliste laedt keine Tracklisten.
- `playlist_items` speichert Vorkommen mit stabiler Position, Medienreferenz und kompakten modellkonformen Eintragsmetadaten. Derselbe Eintrag darf mehrfach vorkommen; unterschiedliche unterstuetzte Medientypen sind erlaubt.
- Ein Eintrag bleibt anzeigbar, wenn sein Detailmedium oder dessen Provider momentan nicht verfuegbar ist. Details werden nur fuer tatsaechlich benoetigte Eintraege gezielt aufgeloest.
- Sortierung, Suche und Pagination erfolgen in SQL vor der Modellrekonstruktion, sofern die gespeicherte Auswahl diese Operation korrekt beantworten kann.
- `result_sets` beschreibt typisierte Ergebnisarten und deren Kontext: Quelle, Benutzer und ergebnisrelevante Parameter. `result_items` bewahrt die vom Server gelieferte Reihenfolge und Vorkommen.
- Empfehlungen, Top-Tracks, aehnliche Medien und dynamische Auswahlen werden als beobachtete Serverergebnisse gespeichert. Sie werden nicht lokal als vermeintlich identische Serverempfehlung neu berechnet.
- Eine laufende dynamische Auswahl wird nicht unbemerkt mit einer anderen Stichprobe vermischt. Gespeicherte Zufalls-/Dynamikdaten sind nur ein letzter beobachteter Stand.

## 7. Lokaler API-Lesevertrag

| MA-Befehl | Lokale Verantwortung |
| --- | --- |
| `music/<type>/library_items` | Bestaetigte Mitgliedschaft, Benutzerfilter, SQL-Suche, Sortierung, Summary und Pagination |
| `music/<type>/get` | Bibliotheks-/Provider-ID aufloesen und angeforderte Detailkenntnis lesen |
| `music/playlists/playlist_tracks` | Angeforderte Playlist-Auswahl nach Kontext und Position lesen |
| `music/recommendations` | Beobachtete Empfehlungsabschnitte lesen |
| `music/recommendations/items` | Geordneten Inhalt des angeforderten Abschnitts lesen |
| Weitere benoetigte Katalog-Lesebefehle | Ihre ausdruecklich registrierte Ergebnisart lesen, nicht eine allgemeine Bibliothek filtern |

- Lokaler und Remote-Adapter verwenden dieselben fachlichen Begriffe, Parameterbedeutungen und Ergebnismodelle. Die unterstuetzten Befehle und Parameter werden in einer expliziten Matrix mit Vertragstests festgehalten.
- Nicht implementierte Parameter werden nicht stillschweigend ignoriert. Sie erfordern einen Remote-Leseweg oder ein explizit nicht lokal beantwortbares Ergebnis.
- `summary`, `limit`, `offset`, Sortierrichtung, Providerfilter und Refresh-Flags entsprechen dem jeweiligen Serverbefehl; eine allgemeine Annahme ueber die Bedeutung von `0` ist unzulaessig.
- Der Cache liefert neben den Fachmodellen intern einen typisierten Status: Miss, teilweise oder vollstaendig sowie Frische. Diese Zusatzinformation veraendert nicht heimlich das MA-Wire-Modell.
- Vollstaendigkeit bezieht sich auf den genauen Kontext. Eine bekannte Seite beweist weder eine vollstaendige Liste noch Vollstaendigkeit bei anderer Suche, Sortierung oder Filterung.
- Lokal gefilterte Teilmengen koennen vorlaeufig angezeigt werden, duerfen aber nicht als serveraequivalente Seite oder vollstaendige Trefferzahl ausgegeben werden.
- Gespeicherte Suchkontexte dienen nur erforderlicher Ergebnisabdeckung, nicht einer unbegrenzten Suchhistorie. Es gibt keine Speicherung von Tokens oder rohen authentifizierten Requests.

## 8. Cache-first und Aktualisierung

1. Die angeforderte kleine lokale Projektion wird gelesen.
2. Ein vorhandenes Ergebnis wird sofort mit seinem internen Kenntnisstand geliefert, ohne die Serverantwort abzuwarten.
3. Wenn online und erforderlich, startet ein sitzungseigener Refresh desselben fachlichen Befehls. Identische laufende Anfragen teilen Abruf, Mapping und Commit; abgeschlossene Futures bilden keinen zweiten Ergebniscache.
4. Eine erfolgreiche Antwort aktualisiert Medien, Zugehoerigkeit, Ergebnisvorkommen und Kenntnisstand atomar. Nur explizit vollstaendige Auswahlen erlauben das Entfernen nicht mehr gelieferter Eintraege.
5. Nach durablem Abschluss wird die betroffene Ansicht benachrichtigt und gezielt neu gelesen. Es erfolgt kein vollstaendiger Bibliotheksreadback nach jedem Commit.

Refresh-Fehler erhalten bestaetigte Inhalte. Bei einem lokalen Miss ist Ladezustand sichtbar; offline ist unbekannt nicht gleich leer. Explizit erzwungene Aktualisierung darf einen alten Treffer nicht als erfolgreiche Serveraktualisierung ausgeben.

Schreibbefehle gehen ausschliesslich an den Server. Erst dessen Erfolg und erforderliche Bestaetigung erlauben den Cache-Commit. Keine Offline-Mutationsqueue und kein lokaler bestaetigter Ersatzschreibpfad werden eingefuehrt.

## 9. Persistenter Bildcache

### Quellen und Varianten

- `image_sources` verwendet die beobachtete `proxy_id` direkt als Quellenschluessel innerhalb des servereigenen Bildcaches, ohne eigene Bildquellen-ID oder Ersatz-ID-Zuordnung. Ohne Proxy-ID dient die andere modellkonforme Referenz im Serverkontext als Quellenschluessel. Gespeichert werden ausserdem Quellenrevision, Beobachtungszeit und Refresh-Zustand. Der Bildcache folgt der Serverauswahl und den getrennten Leerungsfunktionen aus Abschnitt 4.
- Eine `proxy_id` ist eine Quellenidentitaet, kein Inhalts-Hash. Gleiche IDs koennen neue Bytes liefern. Serverzuordnung ist Teil der lokalen Identitaet.
- `image_variants` wird durch Quellenschluessel und Variantenparameter identifiziert und speichert Quelle und Revision, angeforderte Groesse/Format/Transparenzbehandlung, tatsaechlichen MIME-Typ, Inhalts-Hash, Dateireferenz, Bytegroesse und Zugriffsinformation.
- Bildbytes liegen ausserhalb von SQLite in eindeutig cacheeigenen Dateien. Identische Bytes duerfen ueber ihren Inhalts-Hash dedupliziert werden. Medienmetadaten enthalten Referenzen, keine persistierten Base64-Bildkopien.
- Inline-SVGs, Data-URIs und Providericons werden an der Eingangsgrenze als Bildinhalt normalisiert und ueber denselben Dateiowner gespeichert. Sie erhalten keinen zweiten Cachepfad.

### Abruf und Anzeige

- Primarer Netzwerkweg ist der MA-Bildproxy mit beobachteter `proxy_id`. Serverlokale Pfade und Provider-Interna werden nicht lokal nachimplementiert oder als lokale Dateipfade interpretiert.
- Anfangs werden fertige Servervarianten gecacht. Es gibt keine neue lokale Thumbnail-Engine; der Decoder darf vorhandene Dateien fuer die Darstellung skalieren.
- Die angeforderten Varianten orientieren sich an den erlaubten Servergroessen: `0`, `80`, `160`, `256`, `512`, `1024`. Der tatsaechlich gelieferte MIME-Typ ist massgeblich, nicht Dateiendung oder angeforderter Typ.
- Ohne verwendbare Proxyreferenz bleiben Inline-Daten nutzbar. Ein anderer Netzwerkweg braucht eine ausdruecklich definierte Referenz-/Serverversionsunterstuetzung; kein stiller beliebiger URL-Fallback wird eingefuehrt.
- Der Ablauf ist RAM-Treffer, lokale Datei, dann bei Bedarf deduplizierter Proxyabruf. Downloads sind begrenzt und priorisieren sichtbare Bilder gegenueber Prefetch.
- Abgelaufene gueltige Bilder werden sofort angezeigt und online im Hintergrund erneuert. Offline bleiben sie nutzbar. Ein Refresh-Fehler entfernt keine gueltige Datei.
- Ein neuer Inhalt invalidiert Varianten und eventuell daraus abgeleitete Farbpaletten derselben Quelle. Die bisherige sichtbare Variante bleibt bis zum erfolgreichen Ersatz verfuegbar.

### Sicherheit, Speicher und Lifecycle

- Downloads werden vor Publikation auf tatsaechliches Bildformat und sichere Dekodierbarkeit geprueft. SVG-Verarbeitung darf keine externen Ressourcen oder aktiven Inhalte ausfuehren. Eigene Download-/Cache-Byte-Budgets werden zunaechst nicht eingefuehrt; die Sicherheit der Verarbeitung bleibt erforderlich.
- Datei-Publikation erfolgt ueber temporaere Datei und atomaren Austausch. Fehler erzeugen keine bestaetigte DB-Referenz auf einen unvollstaendigen Inhalt; Absturzreste werden cacheeigen bereinigt.
- Negative Eintraege haben kurze Frischezeiten. Fehlendes Bild, voruebergehend nicht erreichbarer Server und Dekodierfehler bleiben unterscheidbar.
- Es gibt zunaechst keine eigenen Bildspeicherbudgets oder Eviction. Serverbudgets werden nicht als Clientlimits uebernommen. Downloads bleiben in ihrer Parallelitaet begrenzt; aktive Dateileases werden bei Ersatz und expliziter Leerung respektiert.
- Eine fehlende Datei wird als reparierbarer Cache-Miss behandelt, nicht als fehlerhaftes gesamtes Medium.
- Frischezeit und Speicherverbleib sind getrennt. Es gibt keinen pauschalen einjaehrigen Immutable-Vertrag fuer eine unveraenderte Proxy-ID.

## 10. Abnahme der Implementierung

Die Implementierung wird erst mit folgenden Nachweisen abgenommen:

1. Summary-/Detail-Roundtrips, Identitaeten, Benutzer-/Servertrennung, Feldkenntnis und Mitgliedschaften bleiben nach Neustart korrekt.
2. Listen- und Playlistabfragen waehlen in SQL aus, erhalten Duplikate und lesen keine unbeteiligten Detailbaeume. Tests enthalten grosse, fuer die Auswahl irrelevante Datenmengen.
3. Die API-Matrix prueft lokale und Remote-Ergebnisse fuer alle unterstuetzten Filter, Sortierungen und Seiten einschliesslich bekannter leerer und unvollstaendiger Ergebnisse.
4. Ein verzoegerter Testserver beweist, dass vorhandene Medien und Bilder vor seiner Antwort angezeigt werden. Netzwerkfehler erhalten diese Anzeige.
5. Gleichzeitige identische Refreshes bzw. Bildabrufe verursachen nur einen Abruf; Sitzungswechsel, Abbruch und Shutdown publizieren keine veralteten Ergebnisse und hinterlassen keine offenen Handles.
6. Bilder bleiben nach Neustart und offline darstellbar; Invalidierung, defekte/fehlende Dateien, Inline-Bilder und negative Eintraege sind fokussiert getestet. Getrenntes Leeren der Datenbank und des Bildcaches sowie Serverwechsel werden mit isolierten Dateien, laufenden Abrufen, wiederholten Aufrufen und Loeschfehlern geprueft. Alte Ergebnisse duerfen keine geleerten Caches wiederbevoelkern; geschuetzte Daten bleiben unveraendert.
7. Rust-, Dart-, Kotlin- und aktive native Verbraucher werden bei Vertragsaenderungen gemeinsam angepasst und auf ihren aktiven Plattformen geprueft.
8. Messungen trennen Queue-Wartezeit, SQL, Modellbildung, Bilddateilesen/Dekodierung und Uebergabe an die UI. Ein schneller SQL-Teil allein reicht nicht als Latenznachweis.
9. Der gemeinsame Ladezugang liefert lokale Daten vor einem verzoegerten Refresh und anschliessend den strukturierten aktualisierten Zustand. Startup-Hintergrundabrufe blockieren keine neue Vordergrundanfrage hinter der gesamten Startup-Liste. Tests pruefen Prioritaetsweitergabe, Promotion eines gemeinsamen Refreshs ohne doppelten Abruf/Commit, Abbruch einzelner und letzter Verbraucher sowie ausbleibende UI-Aktualisierung nach Seiten- oder Sitzungsende.

Als initiale Abnahmeziele gelten fuer typische begrenzte lokale Bildschirmabfragen p95 unter 50 ms warm und unter 150 ms nach Wiederoeffnen bei festgehaltenem Testdatenbestand und Referenzgeraet. Ein grosser laufender Schreibcommit darf diese Reads nicht sekundenlang blockieren. Bildabrufe werden separat nach Variante und Dekodierkosten gemessen; sie duerfen nicht hinter Mediencommits warten.

Diese Werte sind Zielkriterien, keine bereits erbrachten Nachweise. Geraet, Datensatz, Seitengroesse und Messgrenzen werden vor der Performanceabnahme festgehalten. Abweichungen brauchen ausdrueckliche Zustimmung, keine nachtraegliche Umdeutung.

Es gibt zunaechst keine eigenen Cache-Budgets, Medienanzahl- oder DB-/Bild-MB-Quoten und keine Eviction. Leerung erfolgt gemaess Abschnitt 4 durch Serverwechsel oder ausdruecklichen Funktionsaufruf. Eine Performancepruefung darf keine Nutzerdaten loeschen, Bibliotheken kuenstlich begrenzen oder den Vollstaendigkeitsvertrag abschwaechen.