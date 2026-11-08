# Vertrag: Serverorientierter Medien- und Bildcache

Status: Verbindliches Zielmodell fuer die Implementierung
Datum: 2026-11-08
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

Der Cache liegt im Unterordner `Cache` des Mashin-App-Datenwurzelverzeichnisses, nicht im Installationsunterordner `App`. Unter Windows lautet der Pfad `%LOCALAPPDATA%\Mashin\Cache`, auf derselben Ebene wie `App` und `Logs`. Die Medien-SQLite-Datei samt Sidecars und temporaeren DB-Dateien liegt direkt in `Cache`; Bilddateien und ihr unabhaengiger Index liegen im Unterordner `Cache/images`. Cache-Ownership und Serveridentitaet liegen in der Cachewurzel und bleiben bei den getrennten Bereichs-Leerungen erhalten. Bestehende App-, Log- oder anders benannte Cacheordner werden durch diese Ablagevorgabe nicht automatisch verschoben oder geloescht.

Die cachebezogenen Rust-Dateien (`.rs`) sollen hauptsaechlich im Daten-Unterordner `rust/mashin_engine/src/data/` und dort unter `cache/` angelegt werden. Dies ist die bevorzugte Ablage, keine starre Verzeichnisvorgabe; auf Anfrage kann davon abgewichen werden.

### Verantwortlichkeiten des Datenladepfads

| Schicht | Verantwortung | Abgrenzung |
| --- | --- | --- |
| UI und Seitencontroller | Fachliche Daten anfordern, Ergebnisse und Lade-/Fehlerzustaende darstellen, Beobachtung an Sichtbarkeit und Seiten-Lifecycle binden | Kein direkter SQL-, Datei- oder Providerzugriff und keine eigene Online-/Cache-Auswahl |
| Anwendung | Benoetigte Auswahl, Benutzer-/Serverkontext, Prioritaet und Abbruchkontext festlegen; fachliche Berechnungen aus gelieferten Modellen ausfuehren | Kein zweiter bestaetigter Cachebestand und kein eigener persistenter Schreibpfad |
| Lade-Koordination | Lokale und bei bestehender Onlineverbindung serverseitige Reads parallel starten, Kenntnisstand und Frische bewerten, Serverautoritaet sichern und gemeinsame laufende Refreshes koordinieren | Keine SQL-/HTTP-Implementierung und kein zweiter Netzwerkdispatcher |
| Lokaler Leseadapter | Angeforderte Auswahl gezielt aus SQLite lesen und typisierte Modelle samt internem Kenntnisstand liefern | Kein eigener Serverabruf; Filter und Pagination vor Modellrekonstruktion |
| Remote-Leseadapter | Den entsprechenden Serverbefehl ausfuehren und dessen Antwort typisiert verarbeiten | Derselbe fachliche Lesevertrag wie lokal; Netzwerk-Leseanfragen laufen durch den bestehenden Prioritaetsdispatcher |
| Cache-Schreibowner | Erfolgreiche Serverbeobachtungen atomar speichern und nach durablem Abschluss betroffene Cache-Abfragen benachrichtigen | Serveranzeige wartet nicht auf den Commit; keine Entscheidung ueber UI-Bedarf oder Netzwerkprioritaet; kein vollstaendiger Bibliotheksreadback |
| Bildcache-Dateiowner | Vorhandene Bilder liefern, erforderliche Proxyabrufe koordinieren und gepruefte Dateien sicher publizieren | Getrennt vom Medien-Writer; keine zweite Bildcache-Implementierung in der UI |

Der Pfad lautet: UI/Seitencontroller -> Anwendung -> Lade-Koordination. Diese startet den lokalen Leseadapter und bei bestehender Onlineverbindung parallel den Remote-Leseadapter -> bestehenden Prioritaetsdispatcher -> MA-Server. Lokale Ergebnisse und gueltige Serverantworten werden ueber dieselbe Koordination unter Beachtung der Serverautoritaet geliefert. Serverbeobachtungen gehen zusaetzlich an den Cache-Schreibowner; erst dessen durable Benachrichtigung bestaetigt die Persistenz. Die Anwendung erhaelt einen gemeinsamen fachlichen Ladezugang, nicht zwei getrennte UI-Aufrufpfade fuer online und lokal. Diese Verantwortlichkeiten sind keine Vorgabe, fuer jeden Tabelleneintrag ein eigenes Modul anzulegen.

- Eine aktive Leseoperation liefert frueh eintreffende lokale Daten sofort, solange fuer dieselbe Auswahl noch keine gueltige Serverantwort angenommen wurde. Eine zuerst eintreffende Serverantwort wartet nicht auf den lokalen Read und darf durch dessen spaeteres Ergebnis nicht ersetzt werden. Daten, Herkunft, Kenntnisstand und getrennte Refresh-/Persistenzfehler werden typisiert beobachtbar gemacht; ein einziges Future, das vorhandene Daten bis zum Ende des Server-Refreshs zurueckhaelt, erfuellt diesen Ladevertrag nicht.
- Bei lokalem Miss wird Ladezustand geliefert; offline bleibt der Miss ausdruecklich unbekannt statt leer. Cache-Initialisierungs-, Lese- und Schreibfehler verhindern keine regulaere Onlineabfrage oder Anzeige ihrer gueltigen Antwort. Refresh-Fehler erhalten vorhandene Daten. Nach Commit darf ein gezielter Read keine bereits angenommene Serverantwort durch einen aelteren Stand ersetzen; alte Sitzungen und geschlossene Seiten erhalten keine spaeten UI-Aktualisierungen.
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

Aktueller Implementierungsumfang nach Nutzerentscheidung vom 2026-11-08: Kuenstler, Alben, Tracks, Playlists und Recommendations. Eigenstaendige Caches fuer Radio, Hoerbuecher, Podcasts, Podcastfolgen und Soundeffekte werden vorerst nicht umgesetzt. Vorhandene Onlineunterstuetzung bleibt davon getrennt. Recommendations mit nicht implementierten Cachetypen bleiben als Onlineantwort nutzbar; ihre gesamte Cachebeobachtung wird atomar zurueckgewiesen, statt eine gefilterte Teilmenge als vollstaendige Auswahl zu speichern. Die vorhandene Track-only-Playlistabfrage lehnt gemischte Medienantworten ausdruecklich ab. Die nachfolgende Schematabelle beschreibt weiterhin das allgemeine Zielmodell, keinen Auftrag fuer die zurueckgestellten Typen.

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

### Einheitlicher relationaler Medienbestand fuer alle Provider

- Das relationale Prinzip der MA-Bibliothek wird auf alle Provider erweitert. Alle beobachteten Medien werden als eigenstaendige kanonische Objekte in denselben typspezifischen Medientabellen gespeichert, unabhaengig davon, ob sie aus der MA-Bibliothek oder einem externen Provider stammen. Beispielsweise liegen alle Kuenstler in `artists`, alle Alben in `albums` und alle Tracks in `tracks`; ihre beobachtete Provideridentitaet unterscheidet die Objekte. Es gibt keine separaten dauerhaften Medienbestaende je Provider oder Ergebnisart.
- Dieselben Relationstabellen und Lese-/Schreibmechanismen gelten fuer alle Provider. Auch externe Tracks verwenden `track_artists`, externe Alben `album_artists` und Albuminhalte `album_tracks`; dasselbe Prinzip gilt fuer alle weiteren implementierten Medientypen und Beziehungen. Beziehungen referenzieren die tatsaechlich beobachteten Identitaeten beider Medien, ohne gleiche Provider an beiden Enden vorauszusetzen oder eine Bibliothekszuordnung zu erfinden.
- Auch Medien aus Suchtreffern, Kataloglisten, Empfehlungen und Kuenstlersektionen werden mit ihrer vorhandenen Feldkenntnis kanonisch gespeichert. Medien, beobachtete Beziehungen, Ergebnisreferenzen und Kenntnisstaende werden atomar ueber denselben Schreibowner aktualisiert. Fehlende Details erfordern keinen zusaetzlichen Netzwerkabruf nur fuer die Speicherung und werden nicht durch synthetische Defaults ersetzt.
- Ergebnislisten speichern Quelle, Benutzer, Parameter, Reihenfolge, Vorkommen und Abdeckung mit Referenzen auf diese kanonischen Medien. Sie ersetzen weder Medientabellen noch fachliche Beziehungen. Ein externer Top-Track wird deshalb ebenfalls in `tracks` samt beobachteter `track_artists`-Zuordnung gespeichert; sein Rang in der Top-Track-Auswahl bleibt getrennt davon. Dauerhafte Album-/Trackkopien ausschliesslich in Kuenstlersektionen oder serialisierten Ergebnislisten erfuellen diesen Vertrag nicht.
- Die Behandlung als kanonisches Cachemedium bedeutet keine Aufnahme in die MA-Bibliothek. Provideridentitaet, bestaetigte Bibliotheksmitgliedschaft, Benutzerbewertung und Abdeckung einer Providerauswahl bleiben getrennte Sachverhalte. Der separate Provider-Ergebniscache des Servers wird nicht als alternatives Medien-Datenmodell fuer Mashin uebernommen.

### Externe Provider und ihre Katalogdaten

- Der Cache ist nicht auf die MA-Bibliothek beschraenkt. Auch vom MA-Server gelieferte Providerobjekte werden mit Metadaten, Bildreferenzen und Beziehungen gespeichert, selbst wenn sie nicht in der Bibliothek liegen. Beobachtete Provider-Manifeste und -Instanzen werden getrennt von ihren Medien gehalten; Provider-Mappings allein ersetzen weder diese Informationen noch das Providerobjekt.
- Mashin fragt ausschliesslich die unterstuetzten MA-Serverbefehle ab. Der MA-Server ruft seine Provideradapter auf; diese verwenden die jeweiligen Dienst-APIs und Schnittstellen. Mashin implementiert keine eigenen Dienstadapter, kopiert keine Providerzugangsdaten in den Cache und umgeht weder den MA-Server noch dessen Providerregeln.
- Erfolgreiche Details, Suchtreffer und Provider-Katalogergebnisse werden ueber denselben Cache-Schreibowner gespeichert wie Bibliotheksbeobachtungen. Quelle, Providerkontext, Benutzer und ergebnisrelevante Parameter bleiben erhalten. Reihenfolge, Vorkommen und Kenntnisstand einer Providerauswahl werden separat gespeichert, nicht durch eine Filterung der MA-Bibliothek ersetzt.
- Ein Providerobjekt bleibt ueber seine beobachtete Provideridentitaet lesbar; eine bekannte Bibliothekszuordnung kann gemaess dem jeweiligen MA-Befehl bevorzugt werden. Beides darf weder anhand gleicher Item-IDs gleichgesetzt noch durch das Cachen automatisch zur Bibliotheksmitgliedschaft werden. Ein Provider- oder Netzwerkfehler erhaelt vorhandene Beobachtungen, bestaetigt aber keine aktuelle Verfuegbarkeit oder vollstaendige Auswahl.
- Gespeichert werden beobachtete Fachmodelle und Bildinhalte, nicht der gesamte externe Dienstkatalog, dessen API-Implementierung oder Musikdateien. Dass Daten von einem externen Provider stammen, ist kein Grund, sie vom Mediencache auszuschliessen.

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
- Kompakte Eintragsmetadaten enthalten nur vorkommens- oder kontextspezifische Angaben; allgemeine Medienmetadaten und fachliche Beziehungen gehoeren in den kanonischen Medienbestand aus Abschnitt 5. Dies gilt auch fuer `result_items` und Empfehlungseintraege.
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

Vor weiteren Medien- und Katalogmigrationen wird zuerst die Anlage und das atomare Beschreiben der Medien-DB im aktiven Anwendungspfad repariert und mit Wiedereroeffnen nachgewiesen. Danach wird der folgende parallele Ladevertrag im gemeinsamen Ladezugang und seinen aktiven Verbrauchern umgesetzt und geprueft. Isolierte Writer- oder Controller-Tests allein schliessen diesen Reparaturschritt nicht ab.

1. Fuer jede angeforderte Auswahl werden die kleine lokale Projektion und bei bestehender Onlineverbindung der regulaere Serverabruf parallel angestossen. Der Serverabruf wartet weder auf Cache-Initialisierung noch auf lokalen Treffer, Miss, Frischebewertung oder Fehler. Auch ein frischer Cachetreffer unterdrueckt diesen Abruf nicht. Bestehende Netzwerkprioritaeten, Parallelitaetsgrenzen und semantische Deduplizierung bleiben erhalten.
2. Trifft das Cacheergebnis zuerst ein, wird es sofort mit Herkunft und Kenntnisstand angezeigt. Trifft die gueltige Serverantwort zuerst ein, wird diese sofort angezeigt, ohne Cache-Read, DB-Anlage, Commit oder Readback abzuwarten. Ein Cachefehler ist kein Fehler dieser erfolgreichen Serverantwort.
3. Fuer denselben aktiven Server-/Benutzer-/Abfragekontext hat die Serverantwort hoehere Autoritaet als der Cache. Sie darf angezeigte Cachewerte ersetzen; nach ihrer Annahme duerfen spaete lokale Ergebnisse oder Commit-Readbacks die Auswahl nicht auf Cachewerte zuruecksetzen. Autoritaet gilt je unabhaengiger Abfrage bzw. Inhaltssektion, nicht pauschal fuer die ganze Seite. Alte Seiten-, Sitzungs- oder Abfragegenerationen duerfen weder aktuelle UI-Daten noch den Cache mit ueberholten Beobachtungen ersetzen.
4. Serverautoritaet gilt fuer tatsaechlich bekannte Felder und beobachtete Auswahlen. Fehlende Summaryfelder bleiben unbekannt und loeschen keine bekannten Details. Explizites `null`, bekannte leere Beziehungen und vollstaendig beobachtete leere Auswahlen gelten gemaess dem fachlichen API-Vertrag als Serverdaten. Teilantworten duerfen keine unbeobachteten Bereiche als leer ausgeben.
5. Die UI vergleicht fachliche Werte, Feldkenntnis, Reihenfolge und Vorkommenskontext mit dem bereits angezeigten Stand und aktualisiert nur tatsaechlich geaenderte Bereiche. Eine identische Serverantwort wechselt Herkunft und Refreshstatus, ersetzt aber keine unveraenderten Inhaltssektionen. Unveraenderte Bereiche behalten Scrollposition, Auswahl und laufende Interaktionen; es erfolgt kein pauschales Leeren oder vollstaendiger Seitenneuaufbau. Lade- und Fehleranzeigen werden unabhaengig davon gezielt aktualisiert.
6. Die gueltige Serverbeobachtung wird ueber den einzigen Cache-Schreibowner atomar gespeichert: Medien, Zugehoerigkeit, Ergebnisvorkommen und Kenntnisstand bleiben konsistent. Nur explizit vollstaendige Auswahlen erlauben das Entfernen nicht mehr gelieferter Eintraege. Ein Commitfehler wird getrennt als Persistenzfehler gemeldet und entfernt keine bereits angezeigten Serverdaten; er darf weder eine erfolgreiche Persistenz vortaeuschen noch einen zweiten dauerhaften RAM-Bestand erzeugen.
7. Nach durablem Abschluss werden betroffene Cache-Abfragen gezielt benachrichtigt. Falls ein erneuter lokaler Read erforderlich ist, wahrt er die bereits angenommene Serverautoritaet. Es erfolgt kein vollstaendiger Bibliotheksreadback nach jedem Commit. Identische laufende Serveranfragen teilen Abruf, Mapping und Commit; abgeschlossene Futures bilden keinen zweiten Ergebniscache.

Refresh-Fehler erhalten vorhandene Inhalte. Bei einem lokalen Miss ist Ladezustand sichtbar; offline ist unbekannt nicht gleich leer. Explizit erzwungene Aktualisierung darf einen alten Treffer nicht als erfolgreiche Serveraktualisierung ausgeben. Cachezugriff setzt weiterhin eine nachgewiesene Datenpartition und gueltige Generation voraus; der unabhaengige Online-Leseweg umgeht weder Authentifizierung noch die Sicherheits- und Leerungsregeln aus Abschnitt 4.

Schreibbefehle gehen ausschliesslich an den Server. Erst dessen Erfolg und erforderliche Bestaetigung erlauben den Cache-Commit. Keine Offline-Mutationsqueue und kein lokaler bestaetigter Ersatzschreibpfad werden eingefuehrt.

## 9. Persistenter Bildcache

### Quellen und Varianten

- `image_sources` verwendet die beobachtete `proxy_id` direkt als Quellenschluessel innerhalb des servereigenen Bildcaches, ohne eigene Bildquellen-ID oder Ersatz-ID-Zuordnung. Ohne Proxy-ID dient die andere modellkonforme Referenz im Serverkontext als Quellenschluessel. Gespeichert werden ausserdem Quellenrevision, Beobachtungszeit und Refresh-Zustand. Der Bildcache folgt der Serverauswahl und den getrennten Leerungsfunktionen aus Abschnitt 4.
- Eine `proxy_id` ist eine Quellenidentitaet, kein Inhalts-Hash. Gleiche IDs koennen neue Bytes liefern. Serverzuordnung ist Teil der lokalen Identitaet.
- `image_variants` wird durch Quellenschluessel und Variantenparameter identifiziert und speichert Quelle und Revision, angeforderte Groesse/Format/Transparenzbehandlung, tatsaechlichen MIME-Typ, Inhalts-Hash, Dateireferenz, Bytegroesse und Zugriffsinformation.
- Bildbytes liegen ausserhalb von SQLite in eindeutig cacheeigenen Dateien. Identische Bytes duerfen ueber ihren Inhalts-Hash dedupliziert werden. Medienmetadaten enthalten Referenzen, keine persistierten Base64-Bildkopien.
- Inline-SVGs, Data-URIs und Providericons werden an der Eingangsgrenze als Bildinhalt normalisiert und ueber denselben Dateiowner gespeichert. Sie erhalten keinen zweiten Cachepfad.

### Ein gemeinsamer proxyorientierter Bildindex

- Der eigene SQLite-Bildindex unter `Cache/images` ist der Index des gemeinsamen Proxybildcaches, kein zusaetzlicher Inline-, Cover- oder Iconcache. Seine Trennung vom Medienbestand stellt sicher, dass `clear_database_cache` weder Bilddateien noch deren Quellen-/Variantenzuordnung entfernt.
- Die Identitaetskette lautet: ausgewaehlter Server -> beobachtete `proxy_id` -> Variantenparameter -> gespeicherte Bilddatei. Ein Inhalts-Hash identifiziert ausschliesslich die Bytes fuer Dateiintegritaet und optionale Deduplizierung; er ersetzt weder Proxy-ID noch Variantenidentitaet und wird nicht als neue Bildquellen-ID verwendet.
- Der Quellen-/Variantenvertrag und seine Tests werden vor der Dateipersistenz festgelegt. Der Dateiowner wird darunter implementiert; es darf nicht zuerst ein eigenstaendiger Inline-/Covercache entstehen, dem spaeter ein zweiter Proxycache hinzugefuegt wird.
- Inlinebilder, erzeugte Playlistcover und Providericons verwenden denselben Quellen-/Variantenindex und Dateiowner. Eine vorhandene Server-Proxy-ID bleibt massgeblich. Ohne verwendbare Proxy-ID wird nur die modellkonforme Referenz im Serverkontext verwendet, keine erfundene Ersatz-ID oder stillschweigende beliebige Netzwerkquelle.
- Neue Bytes unter derselben Proxy-ID aktualisieren den beobachteten Quellenstand und invalidieren betroffene Varianten gemaess diesem Abschnitt. Eine andere Dateiadresse oder ein anderer Inhalts-Hash begruendet keine neue Serverquelle; der letzte gueltige sichtbare Inhalt bleibt bis zum erfolgreichen Ersatz nutzbar.

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
10. Providerobjekte ohne MA-Bibliotheksmitgliedschaft sowie beobachtete Such-/Provider-Katalogergebnisse werden nach Neustart aus dem Cache gelesen. Tests pruefen identische Item-IDs verschiedener Providerinstanzen, getrennte Bibliotheksidentitaeten, geordnete Ergebnisauswahlen und erhaltene Daten bei ausgefallenem Provider. Ein Testserver weist nach, dass Mashin fuer den Refresh MA-Befehle nutzt und keine externen Dienst-APIs direkt aufruft.
11. Fuer Bibliotheks- und externe Providerobjekte weisen Tests denselben kanonischen Speicher- und Beziehungspfad nach: Tracks aus Kuenstlersektionen stehen in `tracks` mit beobachteten `track_artists`-Beziehungen, Alben in `albums` mit `album_artists`, Albuminhalte mit `album_tracks`. Weitere implementierte Medientypen folgen demselben Prinzip. Ergebnisvorkommen referenzieren diese Objekte und erhalten ihre eigene Reihenfolge und Duplikate; sie sind kein Ersatz-Medienbestand. Die Nachweise umfassen Neustart, partielle Feldkenntnis, atomaren Rollback und unveraenderte Bibliotheksmitgliedschaft beim blossen Cachen externer Medien.
12. Der aktive Bootstrap-/Sitzungs-/Dispatcherpfad legt in einer isolierten leeren App-Datenwurzel die Medien-DB an, speichert gueltige Serverantworten atomar und liefert sie nach Wiedereroeffnen korrekt. Gezielte Initialisierungs-, Lese- und Commitfehler verhindern weder den regulaeren Serverabruf noch die Anzeige seiner Antwort; Persistenzfehler werden separat nachgewiesen. Ein fehlgeschlagener atomarer Write hinterlaesst keine teilweise aktualisierten Medien oder Ergebnislisten.
13. Kontrolliert verzoegerte lokale und Remote-Adapter beweisen beide Ankunftsreihenfolgen: Cache vor Server sowie Server vor Cache. Der Serverabruf beginnt auch bei langsamem lokalem Read, Cachefehler und frischem Cachetreffer ohne auf deren Abschluss zu warten. Spaete Cacheergebnisse und Commit-Readbacks ersetzen keine angenommenen Serverdaten. Tests umfassen unterschiedliche und identische Inhalte, partielle Feldkenntnis, bekannte leere Antworten, unabhaengige Sektionen und alte Abfragegenerationen.
14. Tests der aktiven UI-Verbraucher weisen nach, dass nur fachlich geaenderte Bereiche nach einer Serverantwort aktualisiert werden. Identische Inhalte verursachen keine Inhaltsbenachrichtigung oder Ruecksetzung von Scrollposition und Auswahl; Herkunft, Refresh- und Persistenzstatus werden dennoch korrekt aktualisiert. Eine geaenderte Sektion setzt unveraenderte Nachbarsektionen und den bestehenden Headerkontext nicht zurueck.

Als initiale Abnahmeziele gelten fuer typische begrenzte lokale Bildschirmabfragen p95 unter 50 ms warm und unter 150 ms nach Wiederoeffnen bei festgehaltenem Testdatenbestand und Referenzgeraet. Ein grosser laufender Schreibcommit darf diese Reads nicht sekundenlang blockieren. Bildabrufe werden separat nach Variante und Dekodierkosten gemessen; sie duerfen nicht hinter Mediencommits warten.

Diese Werte sind Zielkriterien, keine bereits erbrachten Nachweise. Geraet, Datensatz, Seitengroesse und Messgrenzen werden vor der Performanceabnahme festgehalten. Abweichungen brauchen ausdrueckliche Zustimmung, keine nachtraegliche Umdeutung.

Es gibt zunaechst keine eigenen Cache-Budgets, Medienanzahl- oder DB-/Bild-MB-Quoten und keine Eviction. Leerung erfolgt gemaess Abschnitt 4 durch Serverwechsel oder ausdruecklichen Funktionsaufruf. Eine Performancepruefung darf keine Nutzerdaten loeschen, Bibliotheken kuenstlich begrenzen oder den Vollstaendigkeitsvertrag abschwaechen.

## 11. Implementierungsstand und verbleibende Schritte

Stand: 2026-11-08. Dieser Abschnitt beschreibt Arbeitsreihenfolge und offene Abnahme, keine Absenkung der Anforderungen aus Abschnitt 10. Detaillierte Nachweise stehen in [cache_implementation_evidence.md](cache_implementation_evidence.md); abgeschlossene Teilschritte bedeuten keine Gesamtabnahme des Caches.

### Bereits nachgewiesene Grundlage

- Anlage und atomare Persistenz der Medien-DB im aktiven Ladepfad sowie parallele lokale und serverseitige Reads mit Serverautoritaet sind umgesetzt und fokussiert nachgewiesen.
- Kanonische Kuenstler, Alben und Tracks sowie bisherige Playlistlisten/-inhalte und Recommendations sind migriert. Private Playlists erstellen, umbenennen und entfernen sowie die Annahmereihenfolge bestaetigter Cachemutationen sind geprueft; weitere Mutations- und Lifecyclegates bleiben offen.
- Nachweis 41 schliesst die frische Serverbestaetigung des privaten Coveradapters ab, nicht die Coverpersistenz im Anwendungspfad. Letzte dokumentierte Gesamtpruefung: 299 Rusttests und 47 fokussierte Darttests bestanden.
- Danach wurde der Inline-Bitmapvalidator in einem fokussierten Test geprueft. Er ist noch kein aktiver Bildcache: Dateipublikation, Bildindex, Proxyabrufe und Lifecycleanbindung fehlen. Eine erneute Gesamtpruefung dieses Zwischenstands steht aus; sichere SVG-Normalisierung ist noch offen.

### Naechste Implementierungsreihenfolge

1. Den gemeinsamen typisierten Quellen-/Variantenvertrag anhand des aktuellen Serverbildproxys, seiner Helper und Modelle festlegen. Tests sichern Proxy-ID als Quelle, Variantenparameter, neue Bytes unter gleicher Proxy-ID und die ausdrueckliche modellkonforme Referenz ohne Proxy-ID. Es entstehen keine lokalen Ersatz-Quellen-IDs.
2. Darunter den unabhaengigen Bildindex und Dateiowner implementieren: validierte Bytes, temporaere Datei, atomare Publikation und bestaetigte Indexzuordnung. Wiedereroeffnen, identische Bytes, Ersatz, fehlende/defekte Dateien und Publikationsfehler mit isolierten Daten pruefen. Keine bestaetigte Referenz auf unvollstaendige Dateien.
3. Vor aktiven Schreibaufrufern Generationen, Annahme/Abschluss laufender Arbeit und Dateileases an Bildcacheleerung, Serverwechsel und Shutdown anbinden. Tests beweisen getrennte Bereichs-Leerung, geschlossene Handles und ausbleibende Wiederbevoelkerung durch alte Ergebnisse.
4. Den gemeinsamen Proxybildladepfad anschliessen: lokale Anzeige vor verzoegertem Server, deduplizierte priorisierte Abrufe, Varianten, Offlineanzeige, Refresh, Quelleninvalidierung und unterscheidbare negative Ergebnisse. Inlinebilder und Icons werden an derselben Eingangsgrenze sicher normalisiert, ohne zweiten Cachepfad.
5. Private Playlistcover daran anschliessen: frische Serverbestaetigung gezielt und sitzungsgesichert als Metadaten publizieren, ueber die gemeinsamen Owner persistieren und vorhandene Trackinhalte/Nachbarbereiche erhalten. Keine Base64-Bildkopien in der Mediendatenbank; Cachefehler bleiben getrennt vom erfolgreichen Serverergebnis.
6. Anschliessend die verbleibenden Medienmutationen und aktiven Verbraucher vervollstaendigen und die gemeinsamen Abnahmegates aus Abschnitt 10 schliessen.

### Weitere offene Arbeiten fuer den vollstaendigen Umfang

- Playlisttracks hinzufuegen und entfernen: Serverabschluss und erforderlichen Readback bestaetigen, betroffene Inhaltsauswahl gezielt aktualisieren und atomar persistieren, ohne pauschalen Bibliotheksrefresh.
- Providerinformationen: beobachtete Manifeste, Instanzen, Icons und Verfuegbarkeit dauerhaft speichern; kanonische Medien und Provider-Mappings ersetzen diese Informationen nicht.
- Cachecommands und Ansichten: vorhandene interne Leerungsmechanik ueber aktive Bindings bereitstellen, betroffene Ansichten gezielt benachrichtigen und regulaer neu laden. Durch diesen Schritt wird keine neue Einstellungsseite beauftragt.
- Durchgaengige Lifecycle- und Fehlerabnahme: Seitenende, letzter Verbraucher, Benutzer-/Serverwechsel, laufende Writes/Downloads, Abbruch, wiederholte Leerung, Loeschfehler und Shutdown ueber die aktiven Verbraucher pruefen.
- UI-Abnahme: bei Refresh und Mutationen nur fachlich geaenderte Bereiche aktualisieren; unveraenderte Header, Nachbarsektionen, Scrollposition, Auswahl und Interaktionen erhalten.
- Plattform- und Leistungsabnahme: aktive Rust-, Dart-, Kotlin- und native Verbraucher auf Android und Windows pruefen; Referenzgeraet/Datensatz festhalten und Queue, SQL, Modellbildung, Bildlesen/Dekodierung und UI-Uebergabe getrennt messen. Die p95-Ziele sind noch nicht insgesamt nachgewiesen; der urspruengliche 45-Sekunden-Haengefall ist weiterhin nicht reproduziert.

Radio, Hoerbuecher, Podcasts, Podcastfolgen und Soundeffekte bleiben ausserhalb des aktuellen eigenstaendigen Cacheumfangs. Ihre bestehende Onlineunterstuetzung bleibt erhalten; ihre Nichtimplementierung erweitert diesen offenen Arbeitsplan nicht stillschweigend.