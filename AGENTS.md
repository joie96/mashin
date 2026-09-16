# Anforderungen an die Codequalität

Status: Verbindliche Implementierungsanforderungen
Datum: 2026-09-13
Geltungsbereich: Jeder für Mashin geänderte Code, einschließlich Rust, Dart, Kotlin, Build-Skripte und Tests.

## 1. Verbindlichkeit und Geltungsbereich

Diese Anforderungen gelten für jede Implementierung, jedes Refactoring und jede Review. Sie gelten gleichermaßen für neuen Code und für bestehende Dateien, die durch eine Änderung berührt werden.

Jeder berührte Bereich muss vor Abschluss der Änderung mit diesem Dokument übereinstimmen. Das Alter oder der bisherige lokale Stil einer Datei rechtfertigt nicht, Inkonsistenzen im berührten Bereich beizubehalten.

## 2. Benennung und API-Weiterentwicklung

Namen müssen Verantwortung, Aktion, Bedingung oder Seiteneffekt präzise beschreiben. Klare, konsistente Namen haben Vorrang vor der Beibehaltung eines schwachen bestehenden Namens.

- Objekte mit derselben Verantwortung verwenden übereinstimmende Methodennamen, Parameterreihenfolge, Rückgabekonventionen, Fehlerverhalten und Methodenreihenfolge.
- Für eine fachliche Aktion wird in Rust, FFI, JNI, Dart und Kotlin derselbe Begriff verwendet. Aliasse wie `play`, `resume` und `start` dürfen nicht dieselbe Aktion bezeichnen.
- Boolesche Werte erhalten explizite Bedingungsnamen, zum Beispiel `is_connected`, `has_session`, `requires_event_hub` und `should_keep_running`.
- In Business-Modulen werden typisierte Fachmodelle anstelle generischer Maps oder untypisiertem JSON verwendet. Serialisierung gehört an eine externe Grenze.
- Öffentliche Modellfelder haben stabile, explizite Namen. Bei einer Umbenennung müssen alle aktiven Produzenten und Konsumenten in derselben Änderung angepasst werden.

Kompatibilität ist kein Grund, unklare APIs, Namen oder Strukturen beizubehalten. Wenn ein besserer Methoden-, Feld- oder Command-Name erforderlich ist, wird er umbenannt und alle Projektaufrufer, Bindings, Tests, Dokumentationen und Plattformadapter werden in derselben Änderung angepasst. Es dürfen keine Kompatibilitätsaliase, veralteten Weiterleitungsmethoden, doppelten Command-Pfade oder parallelen Benennungsschemata eingeführt werden, außer ein externer Release-Vertrag macht eine zeitlich begrenzte Migration unvermeidbar und diese Ausnahme ist ausdrücklich dokumentiert.

## 3. Datei- und Methodenorganisation

Jede Datei hat eine primäre Verantwortung. Eine Datei wird aufgeteilt, wenn nicht zusammenhängende Verantwortlichkeiten Lifecycle, Commands, Persistenz, Abfragen oder Plattform-Interop schwer auffindbar machen.

Zusammengehörige Methoden bleiben benachbart. Öffentliche Commands dürfen nicht mit Low-Level-Parsing, HTTP-Erstellung, Plattform-Interop oder Serialisierungs-Helpern vermischt werden.

Aufeinanderfolgende Methoden mit einer gemeinsamen fachlichen Verantwortung erhalten unmittelbar vor dem Block einen knappen Abschnittskommentar. In C# werden dafür `#region` und `#endregion` verwendet; in Rust und allen Sprachen ohne native Regionen wird ein einzelner normaler Kommentar verwendet. Abschnittskommentare benennen die Verantwortung des gesamten Blocks, nicht die einzelnen Anweisungen, und ersetzen keine erforderlichen Kommentare zu Ownership, Lifecycle oder Nebenwirkungen.

Für Klassen, Structs und Implementierungsblöcke gilt folgende Reihenfolge, sofern die lokale Abstraktion keinen stärker begründeten und dokumentierten Grund dagegen hat:

1. Konstruktoren und Konfiguration;
2. Lifecycle (`initialize`, `activate`, `deactivate`, `shutdown`);
3. Commands und Transport;
4. Abfragen und Snapshots;
5. Ereignisverarbeitung und Worker-Koordination;
6. private Mapping- und Serialisierungs-Helper.

Entsprechende Methoden verwandter Implementierungen, etwa von Playback-Engines, müssen in derselben Reihenfolge stehen.

## 4. Kommentare und Dokumentation

Kommentare erklären Absicht, Einschränkungen, Ownership, Lifecycle-Anforderungen oder nicht offensichtliche Folgen. Sie beschreiben keine offensichtlichen Anweisungen und wiederholen keinen Code.

- Ergänze einen knappen Kommentar, wenn Zweck, Ownership, Vorbedingung oder Seiteneffekt einer Methode aus ihrer Signatur nicht deutlich werden.
- Bei kurzen Methoden steht dieser Kommentar unmittelbar über der Methodendefinition, niemals innerhalb ihres Bodys.
- Kommentare innerhalb einer Methode sind nur bei langen oder strukturell verschachtelten Methoden zulässig.
- Ein interner Kommentar steht unmittelbar über dem zugehörigen fachlichen Block oder Entscheidungspunkt, insbesondere an Zustands-, Lock-, Abbruch-, I/O-, Kompatibilitäts- oder Sicherheitsgrenzen.
- Kommentare werden in derselben Änderung wie die Implementierung aktuell gehalten. Veraltete Kommentare und Kommentare ohne hilfreichen Kontext werden entfernt.
- Modul-Dokumentation beschreibt Ownership, Nebenläufigkeit und plattformübergreifende Einschränkungen, wenn sie für ein vollständiges Modul gelten.

## 5. Migrations- und Änderungsdisziplin

Migriere jeweils nur ein autoritatives Verhalten: Implementiere die Ablösung, passe jeden Aufrufer an, prüfe sie und entferne anschließend das abgelöste Verhalten, bevor die nächste Verantwortung migriert wird.

- Es dürfen keine doppelten Schreibpfade für Queue-Zustand, Benutzereinstellungen, Sitzungsdaten oder die Output-Modus-Auswahl bestehen bleiben.
- Bei einer umbenannten oder umgestalteten API werden alle betroffenen Projektmethoden und Bindings angepasst. Eine unvollständige Weitergabe ist eine fehlgeschlagene Migration, keine Kompatibilitätsanforderung.
- Für jedes neue Modul werden fokussierte Tests ergänzt, bevor seine Aufrufer umgeleitet werden.
- Vertragstests werden für jede Änderung an Commands, Snapshots, Events oder persistierten Datenschemata über alle aktiven Konsumenten aktualisiert.
- Bevorzuge kleine, fachlich vollständige Änderungen. Unzusammenhängende Bereiche dürfen nicht formatiert oder refaktoriert werden.
- Externes Error-Mapping bleibt an der Grenze, die den Fehler erzeugt. Ein Fehler wird einmal gemappt, nicht unabhängig in mehreren Aufrufern.

## 6. Review-Checkliste

Vor einer Review ist zu prüfen:

- Namen beschreiben ihr Verhalten präzise und verwenden das etablierte fachliche Vokabular des Projekts.
- Zusammengehörige Methoden stehen benachbart und folgen in verwandten Implementierungen derselben Reihenfolge.
- Zusammenhängende Methodengruppen sind durch eine passende Region oder einen knappen Abschnittskommentar erkennbar gegliedert.
- Berührte bestehende Dateien erfüllen im geänderten Bereich dasselbe Qualitätsniveau wie neuer Code.
- Erforderliche Kommentare sind knapp und gemäß den obigen Regeln positioniert.
- Jeder Aufrufer, jedes Binding, jeder Test und jedes Dokument wurde bei beabsichtigten API-Änderungen angepasst.
- Es bleibt kein Kompatibilitätsalias oder keine doppelte Implementierung allein zur Bewahrung eines schlechten Namens oder einer schlechten Struktur zurück.
