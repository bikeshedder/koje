#set document(title: "koje – Isolierte Arbeitskontexte unter Hyprland", author: "Michael P. Jung")
#set page(paper: "a4", margin: (x: 2.2cm, y: 2.2cm),
  footer: context [#set text(8pt, fill: gray); koje · Konzept #h(1fr) #counter(page).display()])
#set text(lang: "de", size: 10.5pt)
#set par(justify: true, leading: 0.65em)
#show heading.where(level: 1): set text(20pt, fill: rgb("#1f3a5f"))
#show heading.where(level: 2): it => block(above: 1.4em, below: 0.7em)[
  #set text(13pt, fill: rgb("#1f3a5f")); #it.body
  #v(-0.5em); #line(length: 100%, stroke: 0.5pt + rgb("#c5d3e3"))]
#show heading.where(level: 3): set text(11pt, fill: rgb("#1f3a5f"))
#show heading.where(level: 4): set text(10pt)
#show raw.where(block: true): it => block(fill: rgb("#f4f7fb"), inset: 9pt, radius: 4pt, width: 100%, text(8.5pt, it))
#show raw.where(block: false): set text(9pt)
#show link: set text(fill: rgb("#2a5d9f"))

// ---------- Hilfsfunktionen ----------
#let navy = rgb("#1f3a5f")
#let line-c = rgb("#c5d3e3")
#let warn-c = rgb("#fdf3e1")
#let box-note(title, body, color: rgb("#e8f0f8")) = block(
  fill: color, inset: 10pt, radius: 4pt, width: 100%)[*#title* #h(0.4em) #body]
#let thead(..cols) = table.header(..cols.pos().map(c => text(fill: white)[*#c*]))
#let zebra = (_, y) => if y == 0 { navy } else if calc.even(y) { rgb("#f4f7fb") }
#let tbl(cols, ..cells) = table(columns: cols, inset: 6pt, stroke: 0.5pt + line-c, fill: zebra, ..cells)
#let caption(t) = text(8.5pt, fill: gray, t)

#let dark = rgb("#1e2229")
#let dark2 = rgb("#2a2f38")
#let fg = rgb("#d8dee9")
#let muted = rgb("#8a93a3")
#let c-kp = rgb("#2e7d5b")
#let c-sh = rgb("#3b6fb6")
#let c-ib = rgb("#d9822b")
#let c-ag = rgb("#8a5cc2")
#let c-ws = rgb("#c94f4f")
#let c-gl = rgb("#7a8494")
#let dot(c) = box(circle(radius: 3.2pt, fill: c), baseline: 1pt)
#let chip(t, c: c-kp) = box(fill: c.lighten(75%), stroke: 0.5pt + c, inset: (x: 4pt, y: 2pt), radius: 2pt, text(7.5pt, t))

#let layer(title, fill, body, stroke: line-c) = block(fill: fill, stroke: 0.8pt + stroke, inset: 6pt, radius: 4pt, width: 100%, breakable: false)[
  #text(8pt, weight: "bold", fill: navy)[#title] #v(-0.3em) #set text(8pt); #body]
#let app(t) = box(fill: white, stroke: 0.5pt + line-c, inset: (x: 4pt, y: 2.5pt), radius: 2pt, text(7.5pt, t))

// Monitor-Vorschau im Seitenverhältnis 5120 × 2160 (≈ 2,37 : 1); Fenster als (x, y, b, h, Text) in Anteilen
#let lmon(wins, c: c-kp, h: 2.15cm) = box(width: 100%, height: h, fill: white, stroke: 0.6pt + line-c, radius: 2pt, clip: true, layout(sz => {
  for w in wins { place(dx: w.at(0) * sz.width, dy: w.at(1) * sz.height,
    box(width: w.at(2) * sz.width - 2pt, height: w.at(3) * sz.height - 2pt, fill: c.lighten(72%), stroke: 0.7pt + c, radius: 1.5pt, inset: 2.5pt, text(6pt, fill: c.darken(35%), w.at(4)))) }
}))
#let L-code = ((0.01,0.02,0.52,0.97,[Editor]),(0.53,0.02,0.46,0.56,[Browser]),(0.53,0.58,0.46,0.41,[Terminal]))
#let L-review = ((0.01,0.02,0.62,0.97,[Browser · Merge Request]),(0.63,0.02,0.36,0.97,[Editor]))
#let L-docs = ((0.01,0.02,0.36,0.97,[Dokument · Spezifikation]),(0.37,0.02,0.40,0.97,[Editor]),(0.77,0.02,0.22,0.60,[Browser]))

// ---------- Deckblatt ----------
#page(fill: rgb("#16283f"), margin: 0pt, footer: none)[
  #set text(fill: white)
  #place(top + left, dx: 2.4cm, dy: 3.2cm)[
    #text(11pt, fill: rgb("#9fb3cc"), tracking: 2pt)[KONZEPT]
    #v(0.2cm)
    #text(64pt, weight: "bold")[koje]
    #v(-0.6cm)
    #text(17pt, fill: rgb("#dbe4ef"))[Isolierte Arbeitskontexte unter Hyprland]
    #v(0.5cm)
    #block(width: 12cm)[#text(12pt, fill: rgb("#9fb3cc"))[Jedes Projekt in seiner eigenen Koje – isoliert, farbig markiert, auf Tastendruck da und wieder weg.]]
  ]
  // Illustration: Bildschirm mit Fenstern verschiedener Kontexte
  #place(top + left, dx: 2.4cm, dy: 13.6cm)[
    #box(width: 16.2cm, height: 6.84cm, fill: rgb("#1e3350"), stroke: 1pt + rgb("#2f4a6e"), radius: 6pt, clip: true)[
      #let w(x, y, ww, hh, c, t, a: false) = place(dx: x, dy: y, box(width: ww, height: hh, fill: rgb("#24395a"), stroke: (if a { 2.2pt } else { 1.2pt }) + c, radius: 3pt, inset: 6pt, text(8pt, fill: c.lighten(40%), t)))
      #w(0.4cm, 0.4cm, 7.6cm, 5.6cm, c-kp, [Kundenportal · Editor], a: true)
      #w(8.2cm, 0.4cm, 5.3cm, 3.2cm, c-kp, [Kundenportal · Browser])
      #w(8.2cm, 3.8cm, 5.3cm, 2.2cm, c-kp, [Kundenportal · Terminal])
      #w(13.7cm, 0.4cm, 2.1cm, 5.6cm, c-gl, [Overlay · Chat])
      // Leiste
      #place(bottom + left, box(width: 100%, height: 0.55cm, fill: rgb("#121f33"), inset: (x: 8pt, y: 4pt))[
        #set text(7pt)
        #box(fill: c-kp, inset: (x: 5pt, y: 1.5pt), radius: 6pt)[● KP] #h(3pt)
        #box(fill: rgb("#2a3f60"), inset: (x: 5pt, y: 1.5pt), radius: 3pt)[code] #h(2pt) #text(fill: rgb("#9fb3cc"))[review · docs]
        #h(1fr)
        #box(stroke: 0.6pt + c-sh, inset: (x: 5pt, y: 1.5pt), radius: 6pt)[#text(fill: rgb("#9fb3cc"))[SH ‖]] #h(2pt)
        #box(stroke: 0.6pt + c-ib, inset: (x: 5pt, y: 1.5pt), radius: 6pt)[#text(fill: rgb("#9fb3cc"))[IB]]
      ])
    ]
  ]
  #place(bottom + left, dx: 2.4cm, dy: -2.4cm)[
    #set text(10pt, fill: rgb("#9fb3cc"))
    #grid(columns: (auto, auto), column-gutter: 1.2cm, row-gutter: 0.35cm,
      [Autor], text(fill: white)[Michael P. Jung],
      [Stand], text(fill: white)[13. Oktober 2026],
      [Technik], text(fill: white)[Rust · Podman · Hyprland],
    )
  ]
]

// ---------- Elevator Pitch ----------
#page(footer: none)[
  #v(2.2cm)
  #text(10pt, fill: gray, tracking: 1.5pt)[ELEVATOR PITCH]
  #v(0.1cm)
  #text(24pt, weight: "bold", fill: navy)[Schluss mit dem Fenster-Chaos.]
  #v(0.5cm)
  #set text(12pt)
  #set par(leading: 0.8em)
  Wer an mehreren Projekten gleichzeitig arbeitet, verliert schnell den Überblick: Editoren, Browser, Terminals und Dokumente aller Projekte liegen durcheinander, Links öffnen sich im falschen Fenster, und Projekte, an denen gerade niemand arbeitet, belegen trotzdem Speicher.

  *koje* trennt die Projekte: Jedes wird in einem eigenen, isolierten *Kontext* bearbeitet – seiner Koje. Ein Kontext ist ein Container, in dem alle Programme für dieses Projekt laufen, samt eigener Dev Containers; die Projekte sehen sich gegenseitig nicht. Auf dem Desktop trägt jedes Fenster die Farbe seines Kontexts. Benannte Layouts ordnen die Fenster per Tastendruck neu, und mit einem Befehl wird ein Kontext schlafen gelegt oder genau so wiederhergestellt, wie man es verlassen hat. Die Programme selbst bleiben unverändert.

  #v(0.8cm)
  #let pillar(title, body, c) = block(width: 100%, inset: 12pt, radius: 6pt, fill: c.lighten(88%), stroke: (left: 3pt + c))[
    #text(12pt, weight: "bold", fill: c.darken(20%), title) #v(-0.2em) #text(10pt, body)]
  #grid(columns: (1fr, 1fr, 1fr), gutter: 10pt,
    pillar([Isoliert], [Jedes Projekt in seinem eigenen Kontext – mit eigenem Browser, eigenem Podman und ohne Namenskonflikte.], c-kp),
    pillar([Geordnet], [Kontextfarbe an jedem Fensterrahmen, Layouts statt Workspace-Suche, Overlays für E-Mail und Chat.], c-sh),
    pillar([Sparsam], [Kontexte pausieren oder stoppen, Speicherlimits mit Warnung – und alles kommt zurück, wie es war.], c-ib),
  )
  #v(0.8cm)
  #line(length: 100%, stroke: 0.5pt + line-c)
  #v(0.2cm)
  #set text(10pt)
  #grid(columns: (auto, 1fr), column-gutter: 1cm, row-gutter: 0.4cm,
    text(fill: gray)[Für wen], [Entwickler, die unter Linux an mehreren Projekten parallel arbeiten],
    text(fill: gray)[In einem Satz], [Jedes Projekt in seiner eigenen Koje – isoliert, farbig markiert, auf Tastendruck da und wieder weg],
    text(fill: gray)[Technik], [Rust-Werkzeug mit Hintergrunddienst, Podman in Podman, Hyprland als Desktop],
  )
]

// ======================================================================
#counter(page).update(1)
= koje – Isolierte Arbeitskontexte unter Hyprland
#text(9pt, fill: gray)[Stand: 13. Oktober 2026 · Michael P. Jung]

#v(0.6em)
*Kurzfassung:* Mit `koje` wird jedes Projekt in einem eigenen, isolierten *Kontext* bearbeitet: einem Podman-Container, in dem alle Programme für dieses Projekt laufen – einschließlich eines eigenen Podman für die Dev Containers. Die Projekte sind dadurch voneinander getrennt und sehen sich gegenseitig nicht. Hyprland zeigt die Fenster aller Kontexte auf einem Desktop an und färbt jeden Fensterrahmen in der Farbe seines Kontexts. Innerhalb eines Kontexts wechselt man zwischen benannten *Layouts*; nicht benötigte Fenster liegen in einer Ablage in der Leiste. Kontextübergreifende Programme wie E-Mail oder Chat erscheinen in einblendbaren Overlays. `koje` startet und stoppt Kontexte als Einheit, stellt ihre Fenster wieder her, begrenzt ihren Speicher und warnt vor dem Limit. Die Programme selbst müssen dafür nicht angepasst werden.

== Ziele

#tbl((1fr, 1.2fr),
  thead[Ziel][Umsetzung],
  [Fenster eines Kontexts gehören zusammen und sind als solche erkennbar], [Ein Kontext = ein Container, ein Hyprland-Workspace, eine Rahmenfarbe],
  [Links und Dateien öffnen im richtigen Kontext], [Jeder Container hat seine eigenen Standardprogramme; was im Kontext geöffnet wird, bleibt dort],
  [Kontexte sind voneinander isoliert], [Eigene Namensräume für Prozesse, Netzwerk, IPC und Dateisystem; eigenes Podman pro Kontext],
  [Inaktive Kontexte kosten nichts], [Kontexte als Einheit stoppen oder pausieren; Speicherlimits pro Kontext],
  [Die Arbeitsumgebung lässt sich wiederherstellen], [Zustände und Layouts; laufender Absturzschutz],
  [Keine Anpassung der Programme], [Programme laufen im Container wie auf einem normalen System; optionale Integrationen für besseres Starten und Wiederherstellen],
)

== Grundprinzip

Kein Desktop kennt Einstellungen pro Workspace – Standardbrowser und Dateizuordnungen (XDG) gelten für die ganze Sitzung. Deshalb isoliert `koje` nicht den Workspace, sondern die *Arbeit daran*: Jeder Kontext ist ein Container mit eigenem Home-Verzeichnis und eigenen Standardprogrammen. Ein Link, der im Kontext geöffnet wird, landet automatisch beim Browser dieses Kontexts – ohne Profile, Wrapper oder Umgebungsvariablen.

*Begriffe:* Projekte werden in Kontexten isoliert – üblicherweise ein Projekt pro Kontext, bei zusammengehörigen Projekten (etwa Frontend und Backend eines Kunden) auch mehrere. `koje` selbst kennt dabei nur *Kontexte*: Die Projekte bindet ein Kontext einfach als Ordner ein; ein eigenes Projekt-Objekt gibt es in `koje` nicht. *Container* bezeichnet nur die technische Umsetzung (Kontext-Container, Dev Container), *koje* nur das Werkzeug.

Grafische Programme sprechen direkt mit dem Wayland-Compositor (Hyprland), über den Wayland-Socket. `koje` reicht diesen Socket in den Container durch; deshalb erscheinen Container-Fenster wie gewöhnliche Fenster auf dem Desktop. Alles Übrige – Farbe, Platzierung, Layouts – erledigt `koje` von außen über die Schnittstelle von Hyprland. Die Programme merken davon nichts.

// ======================================================================
== Architektur

#block(breakable: false)[
#layer([Host · Hyprland · `kojed` · Podman (rootless)], rgb("#f4f7fb"))[
  #grid(columns: (1fr, 1fr, 0.8fr), gutter: 8pt,
    layer([Kontext „Kundenportal“], rgb("#e8f3ee"), stroke: c-kp)[
      #app[Editor] #app[Browser] #app[Terminal] \ #v(0.2em)
      #layer([eigenes Podman], white)[#app[Dev Container] #app[`backend_db`]]
    ],
    layer([Kontext „Shop-Relaunch“], rgb("#eaf0f9"), stroke: c-sh)[
      #app[Editor] #app[Browser] \ #v(0.2em)
      #layer([eigenes Podman], white)[#app[Dev Container] #app[`backend_db`]]
    ],
    layer([Kontext „global“], rgb("#f0f1f3"), stroke: c-gl)[
      Overlays: \ #app[E-Mail] #app[Kalender] #app[Chat] \ #v(0.2em)
      #text(7.5pt, fill: gray)[nicht isoliert, läuft beim Anmelden]
    ],
  )
  #v(0.2em)
  #block(fill: white, stroke: (paint: line-c, dash: "dashed"), inset: 5pt, radius: 3pt, width: 100%)[#text(7.5pt)[*Gemeinsames Basis-Image* – schreibgeschützte Schichten, von allen Kontexten genutzt; pro Kontext nur eine eigene Schreibschicht und ein eigenes Home-Volume]]
]
#caption[Beide Kontexte enthalten ein Compose-Projekt namens `backend` – dank eigenem Podman pro Kontext entstehen keine Namenskonflikte.]
]

=== Kontext-Container

`koje` erzeugt die Kontext-Container direkt mit Podman. Jeder Container bekommt:

- ein eigenes Home-Volume und eigene Namensräume für Prozesse, Netzwerk und IPC (Standard bei Podman),
- den Wayland-Socket, den PipeWire-Socket für Audio und die Grafikkarte (`/dev/dri`),
- einen *gefilterten* D-Bus-Zugang über einen Proxy, wie ihn auch Flatpak nutzt: Benachrichtigungen und Dateiauswahl-Dialoge sind erlaubt, das Öffnen von Links auf dem Host ausdrücklich nicht – sonst würden Links den Kontext verlassen,
- ein Label mit dem Kontextnamen, über das `kojed` Prozesse und Fenster eindeutig zuordnet,
- Speicherlimits (siehe Abschnitt Speicher).

=== Podman in Podman

Jeder Kontext bringt sein eigenes rootless Podman mit. Dev Containers und Compose-Projekte laufen darin und sind für andere Kontexte und den Host unsichtbar. Gleichnamige Compose-Projekte in verschiedenen Kontexten kollidieren nicht.

#tbl((auto, 1fr),
  thead[Thema][Umsetzung],
  [Image-Speicher innen], [Eigenes Volume pro Kontext (verschachteltes overlayfs auf overlayfs ist nicht möglich)],
  [Gemeinsame Images], [Häufige Basis-Images über einen schreibgeschützten, gemeinsamen Image-Speicher (`additionalimagestores` in `storage.conf`) bereitstellen, statt sie pro Kontext erneut zu laden],
  [Voraussetzungen], [Eigener Bereich an Sub-UIDs und -GIDs im Container, Zugriff auf `/dev/fuse`, passende Storage-Konfiguration],
)

#box-note("Zu prüfen:", [Die Einrichtung von verschachteltem rootless Podman und die Nutzung eines gemeinsamen Image-Speichers aus dem inneren Podman einmal an einem Test-Kontext durchspielen und als Vorlage für das Basis-Image festhalten. #link("https://redhat.com/sysadmin/image-stores-podman")[Quelle]], color: warn-c)

=== Sicherheit unter Wayland

Wayland trennt Programme grundsätzlich: Ein Fenster kann weder Eingaben noch Inhalte anderer Fenster lesen. Ausnahmen sind privilegierte Protokolle, etwa für Bildschirmaufnahmen. Hyprland bringt ein Berechtigungssystem mit, über das sich solche Zugriffe einschränken lassen; `koje` sollte es so konfigurieren, dass Programme aus Kontexten keine Bildschirmaufnahmen anderer Fenster anfertigen können. #link("https://wiki.hypr.land/Configuring/Advanced-and-Cool/Permissions/")[Quelle]

// ======================================================================
== Speicher

=== Was zwischen Kontexten geteilt wird

Programmcode und Bibliotheken werden vom Kernel als Dateien in den Speicher eingeblendet. Prozesse, die *dieselbe Datei* nutzen, teilen sich deren Speicherseiten – auch über Container hinweg. Voraussetzung ist, dass es auf dem Host tatsächlich dieselbe Datei ist: Alle Kontexte nutzen dasselbe Basis-Image, und Podman verwendet das native overlayfs des Kernels (rootless seit Kernel 5.13, bei aktuellen Systemen Standard).

#tbl((1fr, auto, 1.3fr),
  thead[Speicherart][Geteilt?][Anmerkung],
  [Programmcode, Bibliotheken], [Ja], [Wenn aus derselben Image-Schicht; ein im Kontext nachinstalliertes Programm ist eine eigene Datei und wird nicht geteilt],
  [Privater Speicher (Heap, JavaScript, Caches, Grafikpuffer)], [Nein], [Bei Browsern und Editoren der weitaus größte Anteil],
  [Images der Dev Containers], [Teilweise], [Nur über den gemeinsamen, schreibgeschützten Image-Speicher],
  [Dateicache der eingebundenen Ordner], [–], [Pro Datei; jeder Kontext arbeitet ohnehin an eigenen Dateien],
)

*Ein Browser pro Kontext* kostet deshalb kaum mehr als parallel laufende Browser-Profile: Auch ein zweites Profil startet eine eigene Prozessfamilie mit eigenem privatem Speicher. Der Programmcode wird in beiden Fällen geteilt, solange er aus dem gemeinsamen Basis-Image stammt. Häufig genutzte Programme gehören daher ins Basis-Image, statt pro Kontext installiert zu werden.

#box-note("Hinweis zur Messung:", [Geteilte Seiten rechnet der Kernel dem Kontext zu, der sie zuerst geladen hat. Die Speicheranzeige pro Kontext ist deshalb leicht ungenau; für Warnungen und Limits ist das unerheblich.])

=== Limits pro Kontext

#tbl((auto, 1fr, 1fr),
  thead[Grenze][Verhalten][Umsetzung],
  [Soft-Limit], [Warnung bei 90 % und bei 100 %; der Kontext läuft weiter], [`kojed` überwacht den Speicherverbrauch der cgroup und sendet eine Desktop-Benachrichtigung],
  [Hard-Limit], [`koje` fährt den Kontext geordnet herunter – der Stand wird in `default` gespeichert – und meldet das], [Überwachung durch `kojed`; Herunterfahren wie `koje down`],
  [Sicherheitsnetz], [Der Kernel greift ein, falls das geordnete Herunterfahren nicht rechtzeitig gelingt; betroffen ist nur dieser Kontext], [cgroup-Grenze `memory.max`, etwas oberhalb des Hard-Limits],
)

Limits lassen sich im laufenden Betrieb ändern (`koje limit`, intern über `podman update`). Ohne Angabe gelten Standardwerte aus der globalen Konfiguration.

==== Mockup: Speicherübersicht und Warnung

#let membar(name, c, used, soft, hard, label, warn: false) = grid(columns: (3.4cm, 1fr, 3.6cm), gutter: 8pt, align: horizon,
  [#box(width: 3pt, height: 1em, fill: c, radius: 1pt, baseline: 20%) #h(3pt) #text(8.5pt)[*#name*]],
  box(width: 100%, height: 9pt, fill: rgb("#e9edf3"), radius: 2pt, clip: true, layout(sz => {
    place(box(width: used / hard * sz.width, height: 9pt, fill: if warn { rgb("#e0a03a") } else { c.lighten(20%) }))
    place(dx: soft / hard * sz.width - 0.6pt, box(width: 1.2pt, height: 9pt, fill: navy))
  })),
  text(8pt, fill: if warn { rgb("#b06a00") } else { gray }, label),
)
#block(breakable: false)[
#grid(columns: (1.7fr, 1fr), gutter: 10pt,
  block(fill: rgb("#f7f9fc"), stroke: 0.8pt + line-c, radius: 6pt, inset: 9pt, width: 100%)[
    #text(8pt, fill: gray)[SPEICHER PRO KONTEXT] #h(1fr) #text(7.5pt, fill: gray)[Strich = Soft-Limit · Ende = Hard-Limit]
    #v(0.2em)
    #membar("Kundenportal", c-kp, 5.5, 6, 8, [5,5 / 6 GB · 92 %], warn: true)
    #v(0.25em)
    #membar("Shop-Relaunch", c-sh, 2.1, 6, 8, [2,1 / 6 GB · pausiert])
    #v(0.25em)
    #membar("global", c-gl, 1.4, 3, 4, [1,4 / 3 GB])
  ],
  block(fill: dark, radius: 6pt, inset: 9pt, width: 100%)[
    #set text(8.5pt, fill: fg)
    #box(width: 8pt, height: 8pt, fill: rgb("#e0a03a"), radius: 2pt, baseline: 10%) #h(3pt) *koje*
    #v(0.1em)
    Kundenportal hat 92 % des Soft-Limits erreicht (5,5 von 6 GB). Bei 8 GB wird der Kontext gespeichert und heruntergefahren.
    #v(0.2em)
    #text(fill: muted)[Limit erhöhen · Details]
  ],
)
#caption[Links die Übersicht im Panel, rechts die Desktop-Benachrichtigung beim Erreichen von 90 %.]
]

// ======================================================================
== Desktop: Hyprland

Hyprland läuft komplett schwebend: Fenster werden frei platziert, nie automatisch angeordnet. Damit bleibt klassisches Arbeiten erhalten, und `koje` bekommt eine offene Schnittstelle zum Lesen und Steuern aller Fenster – Prozess-ID, Position, Größe, Workspace – sowie einen Ereignis-Socket, der jedes neue Fenster meldet. #link("https://wiki.hypr.land/IPC/")[Quelle]

=== Grundkonfiguration

```
# alle Fenster schwebend – Tiling ist damit faktisch aus
windowrule = float, class:.*
# Super + linke Maustaste: verschieben; Super + mittlere Maustaste: Größe ändern
bindm = SUPER, mouse:272, movewindow
bindm = SUPER, mouse:274, resizewindow
```

#text(9pt, fill: gray)[Die Konfigurationssyntax von Hyprland ändert sich zwischen Versionen öfter; das Beispiel vor dem Einsatz gegen die installierte Version prüfen. Leiste, Benachrichtigungen und Bildschirmsperre werden aus Einzelteilen zusammengestellt (etwa Waybar).]

=== Kontextfarbe am Fensterrahmen

`kojed` erhält von Hyprland jedes neue Fenster, ermittelt über Prozess-ID und cgroup den zugehörigen Kontext und setzt Farbe und Stärke des Rahmens – für aktive und inaktive Fenster getrennt, auch als Verlauf. So trägt *jedes* Fenster eines Kontexts dessen Farbe, unabhängig vom Programm und ohne dessen Einstellungen anzufassen.

=== Workspaces

Ein Kontext belegt genau einen Hyprland-Workspace pro Monitor. Mehrere Arbeitsbereiche innerhalb eines Kontexts sind *Layouts* (eigener Abschnitt). Der Wechsel in einen anderen Kontext – per Menü oder `koje focus` – holt dessen Workspace mit dem zuletzt aktiven Layout nach vorn.

=== Einrasten

#tbl((auto, 1fr, auto),
  thead[Art][Umsetzung][Stand],
  [Kantenmagnetismus], [Hyprland-Option für schwebende Fenster: Beim Ziehen rasten sie am Bildschirmrand und an Nachbarfenstern ein], [eingebaut],
  [Zonen per Taste], [Raster pro Monitor in der `koje`-Konfiguration; `koje snap` setzt das aktive Fenster pixelgenau in eine Zone, ausgelöst per Tastenkombination], [`koje`],
  [Ziehen in Zonen mit Vorschau], [Über die Schnittstelle nicht möglich; bräuchte ein Hyprland-Plugin], [später],
)

```toml
# ~/.config/koje/config.toml – Zonen für den 5K-Monitor (5120 × 2160)
[zones.DP-1]
links  = { x = 0,    y = 0, w = 1700, h = 2160 }
mitte  = { x = 1700, y = 0, w = 2100, h = 2160 }
rechts = { x = 3800, y = 0, w = 900,  h = 2160 }
chat   = { x = 4700, y = 0, w = 420,  h = 2160 }
```

=== Overlays

Kontextübergreifende Programme – E-Mail, Kalender, Chat – laufen im nicht isolierten Kontext `global`. Sie liegen in *Special Workspaces* von Hyprland: benannte Ebenen, die per Tastendruck über dem aktuellen Workspace ein- und wieder ausgeblendet werden.

- *Schnell:* Ein- und Ausblenden ist ein Tastendruck, der Workspace darunter bleibt unverändert.
- *Anheften:* `koje pin` holt das aktive Fenster aus dem Overlay auf den aktuellen Workspace und heftet es an – es bleibt sichtbar, auch wenn das Overlay ausgeblendet oder der Kontext gewechselt wird. So lässt sich eine Nachricht schreiben, während der Kontext daneben liegt. `koje unpin` schickt es zurück.

```toml
# ~/.config/koje/config.toml
[overlay.mail]
key  = "SUPER+M"
apps = ["mail", "calendar"]    # Integrationen oder Startbefehle

[overlay.chat]
key  = "SUPER+C"
apps = ["chat"]
```

=== Dateien zwischen Kontexten

Kopieren und Ziehen laufen über den Compositor, nicht über das Dateisystem. Bei Text und Bildern wird der Inhalt übertragen – das klappt immer. Bei Dateien wird meist nur der *Pfad* übergeben; das Zielprogramm muss die Datei selbst öffnen können.

#tbl((auto, 1fr),
  thead[Richtung][Ergebnis],
  [Text, Bilder (Zwischenablage)], [Funktioniert immer, auch zwischen Kontexten],
  [Kontext → Overlay (`global`)], [Funktioniert, wenn der Pfad im Container und auf dem Host gleich ist: `koje` bindet Home-Volume und eingebundene Ordner unter demselben Pfad ein wie auf dem Host],
  [Host → Kontext], [Nur, wenn der Pfad im Container eingebunden ist],
  [Kontext A → Kontext B], [Scheitert – das ist die gewollte Trennung],
)

Ein Austauschordner (etwa `~/Austausch`) wird in alle Kontexte unter demselben Pfad eingebunden – ein bewusstes, kleines Loch in der Trennung.

// ======================================================================
#pagebreak(weak: true)
== Layouts

*Prinzip:* Ein Layout ist eine benannte Anordnung der Fenster eines Kontexts – etwa `code`, `review` oder `docs`. Ein Fenster darf in mehreren Layouts vorkommen, jeweils mit eigener Position und Größe. Fenster, die im aktiven Layout nicht vorkommen, liegen in der *Ablage* und erscheinen als Symbol in der Leiste. Layouts ersetzen mehrere Workspaces pro Kontext.

=== Begriffe

#let nest(title, fill, body) = block(fill: fill, stroke: 0.8pt + line-c, inset: 6pt, radius: 4pt, width: 100%, breakable: false)[#text(8pt, weight: "bold", fill: navy, title) #v(-0.2em) #set text(8pt); #body]
#block(breakable: false, grid(columns: (1fr, 1.25fr), gutter: 12pt,
  nest([Kontext „Kundenportal“ · Container, Farbe], rgb("#f4f7fb"))[
    #nest([Zustand `default` · im Alltag der einzige], rgb("#e8f0f8"))[
      *Fensterbestand:* #chip[Editor] #chip[Browser] #chip[Terminal] #chip[Dokument] \
      #v(0.3em)
      *Layouts:* #chip(c: navy)[code ●] #chip(c: navy)[review] #chip(c: navy)[docs]
    ]
    #v(0.1em)
    #text(7.5pt, fill: gray)[weitere Zustände optional, etwa `fresh` oder `workshop`]
  ],
  table(columns: (auto, 1fr, auto), inset: 5pt, stroke: 0.5pt + line-c, fill: zebra,
    thead[Begriff][Legt fest][Wechsel],
    [Kontext], [Container, Farbe, eingebundene Ordner, Fenster], [sofort],
    [Zustand], [*Welche* Programme laufen – und deren Layouts], [teuer, selten],
    [Layout], [*Wie* die laufenden Fenster angeordnet und welche sichtbar sind], [Millisekunden],
    [Ablage], [Laufende Fenster, die im aktiven Layout fehlen], [–],
  ),
))

=== Ein Layoutwechsel

+ Fenster, die im Ziel-Layout fehlen, wandern in die Ablage – einen versteckten Special Workspace des Kontexts.
+ Fenster des Ziel-Layouts erhalten dessen Position und Größe; Fenster aus der Ablage werden dabei eingeblendet.
+ Der Fokus geht an das Fenster, das im Ziel-Layout zuletzt aktiv war.

Es wird nichts gestartet und nichts beendet. `koje` schickt alle Verschiebungen gebündelt in einem Aufruf an Hyprland; spürbar ist im Wesentlichen nur die Animation.

#let lpanel(name, wins, stash, active: false) = block(breakable: false, width: 100%)[
  #text(9pt)[#if active [*#name* #text(fill: c-kp)[● aktiv]] else [*#name*]]
  #v(-0.4em) #lmon(wins) #v(-0.3em)
  #text(7.5pt, fill: gray)[Ablage:] #for s in stash [#chip(c: luma(150), s) ]
]

==== Mockup: dieselben Fenster in drei Layouts

#block(breakable: false)[
#grid(columns: (1fr, 1fr, 1fr), gutter: 10pt,
  lpanel("code", L-code, ([Dokument],), active: true),
  lpanel("review", L-review, ([Terminal], [Dokument])),
  lpanel("docs", L-docs, ([Terminal],)),
)
#caption[Monitor im Seitenverhältnis 5120 × 2160. Der Editor ist in allen drei Layouts sichtbar, aber jedes Mal anders platziert; Terminal und Dokument erscheinen nur dort, wo sie gebraucht werden.]
]

=== Ablage

Hyprland kennt kein echtes Minimieren; üblich ist ein versteckter Special Workspace – genau das ist die Ablage. Das Verschwinden und Erscheinen animiert Hyprland, etwa als Ausblenden oder Herauszoomen. Ein Flug-in-die-Leiste-Effekt bräuchte ein Hyprland-Plugin und ist für später vorgemerkt.

- *Leiste:* Das `koje`-Modul zeigt die Ablage des aktiven Kontexts als Symbole. Ein Klick holt ein Fenster ins aktive Layout – an die Stelle, an der es zuletzt in diesem Layout lag. Rechtsklick bietet „In Layout … holen“ und „Schließen“.
- *Tastatur:* `koje stash` (etwa Super + H) legt das aktive Fenster ab und nimmt es aus dem aktiven Layout.

==== Mockup: Leiste mit Kontext, Layouts, Ablage und Speicher

#let tab(t, active: false) = box(fill: if active { c-kp } else { dark2 }, inset: (x: 7pt, y: 2.5pt), radius: 3pt, text(fill: if active { white } else { fg }, t))
#let sicon(t) = box(fill: dark2, stroke: 0.5pt + muted, inset: (x: 5pt, y: 2pt), radius: 3pt, text(fill: fg, t))
#block(breakable: false)[
#block(fill: dark, radius: 4pt, inset: (x: 8pt, y: 5pt), width: 100%)[
  #set text(8.5pt, fill: fg)
  #box(fill: c-kp, inset: (x: 6pt, y: 2.5pt), radius: 8pt, text(fill: white)[● *KP*])
  #h(4pt) #tab([1 code], active: true) #tab[2 review] #tab[3 docs] #text(fill: muted)[ +]
  #h(1fr) Di 13. Okt · 16:09 #h(1fr)
  #text(fill: muted)[Ablage] #sicon[▤ Dokument]
  #h(5pt) #box(fill: rgb("#e0a03a"), inset: (x: 5pt, y: 2pt), radius: 3pt, text(fill: dark)[5,5 / 6 GB])
  #h(4pt) #box(stroke: 0.6pt + c-sh, inset: (x: 6pt, y: 2pt), radius: 8pt, text(fill: muted)[SH ‖])
]
#caption[Links der aktive Kontext mit seinen Layouts (Super + 1 … 9 wechselt, Super + Tab blättert). Rechts die Ablage, der Speicherverbrauch – orange ab 90 % des Soft-Limits – und weitere Kontexte, hier ein pausierter. Klick auf die Kontext-Kapsel öffnet das Schnellstartmenü.]
]

=== Layouts bearbeiten

- *Automatisch speichern* (Checkbox pro Layout, standardmäßig an): Was im aktiven Layout verschoben oder in der Größe geändert wird, übernimmt `koje` sofort in dieses Layout.
- *Ohne automatisches Speichern* wird ein Layout zur Vorlage: Änderungen gelten bis zum nächsten Wechsel; beim Zurückkehren erscheint wieder die gespeicherte Anordnung. `koje layout save` übernimmt den aktuellen Stand bewusst.
- *Neue Fenster* gehören zum aktiven Layout. *Geschlossene Fenster* verschwinden aus allen Layouts.
- *Neues Layout:* `koje layout new docs` startet leer, alle Fenster liegen zunächst in der Ablage; mit `--copy` übernimmt es die aktuelle Anordnung.

==== Mockup: Layout-Übersicht (Super + L)

#let othumb(name, wins, active: false, noauto: false) = block(width: 100%, breakable: false, fill: if active { dark2 } else { none }, stroke: if active { 1.2pt + c-kp } else { 0.5pt + muted }, radius: 5pt, inset: 5pt)[
  #text(8.5pt, fill: fg)[*#name* #if noauto [#text(fill: muted)[· Autosave aus]]]
  #v(-0.4em) #lmon(wins)]
#block(breakable: false)[
#block(fill: dark, radius: 8pt, inset: 10pt, width: 100%)[
  #text(8pt, fill: muted)[KUNDENPORTAL · LAYOUTS]
  #v(0.1em)
  #grid(columns: (1fr, 1fr, 1fr, 0.55fr), gutter: 8pt,
    othumb("1 code", L-code, active: true),
    othumb("2 review", L-review, noauto: true),
    othumb("3 docs", L-docs),
    block(width: 100%, height: 2.75cm, stroke: (paint: muted, dash: "dashed", thickness: 0.6pt), radius: 5pt, align(center + horizon, text(8pt, fill: muted)[\+ Neues \ Layout])),
  )
  #v(0.2em)
  #text(8pt, fill: muted)[Ablage von „code“:] #sicon[▤ Dokument]
  #v(0.1em)
  #text(7.5pt, fill: muted)[1 – 9 wechseln · Fenster zwischen Vorschauen ziehen ordnet zu · Entf entfernt aus Layout · A Autosave an/aus · Esc schließt]
]
#caption[Eingeblendet über dem Desktop. Ein Fenster in eine andere Vorschau zu ziehen, nimmt es in dieses Layout auf – so entsteht ein neues Layout, ohne Fenster einzeln zu verschieben.]
]

==== Mockup: Panel, Reiter „Layouts“

#let yes = text(fill: c-kp)[●]
#let no = text(fill: luma(170))[○]
#let cb(on) = box(width: 8pt, height: 8pt, radius: 1.5pt, stroke: 0.8pt + if on { c-kp } else { luma(150) }, fill: if on { c-kp } else { white }, baseline: 10%, if on { align(center + horizon, text(6pt, fill: white, weight: "bold")[x]) })
#let gc(body) = table.cell(fill: rgb("#edf1f7"), body)
#block(breakable: false)[
#block(fill: rgb("#f7f9fc"), stroke: 0.8pt + line-c, radius: 8pt, inset: 10pt, width: 100%)[
  #box(width: 10pt, height: 10pt, fill: c-kp, radius: 2pt, baseline: 15%) #h(4pt) #text(11pt)[*Kundenportal*]
  #h(1fr) #text(8.5pt)[Zustände · #underline[*Layouts*] · Fenster · Speicher]
  #v(0.3em)
  #table(columns: (1.4fr, 1fr, 1fr, 1fr, 0.9fr), inset: 5pt, stroke: 0.5pt + line-c, align: (left, center, center, center, center),
    fill: (_, y) => if y == 0 { rgb("#edf1f7") },
    [*Fenster*], [*code* #text(fill: c-kp)[aktiv]], [*review*], [*docs*], [#text(fill: navy)[\+ Layout]],
    [Editor · `editor`], yes, yes, yes, [],
    [Browser · `browser`], yes, yes, yes, [],
    [Terminal · `term` · Dev Container], yes, no, no, [],
    [Dokument · `spec`], no, no, yes, [],
    gc[_Automatisch speichern_], gc(cb(true)), gc(cb(false)), gc(cb(true)), gc[],
  )
  #text(7.5pt, fill: gray)[● sichtbar im Layout · ○ in der Ablage · Klick schaltet um · Doppelklick auf einen Layoutnamen wechselt dorthin]
]
#caption[Die Fenster-IDs (`editor`, `term` …) vergibt `koje` automatisch aus Integration bzw. Programmname und einem Zähler; sie sind umbenennbar.]
]

=== Wo Layouts gespeichert werden

*Layouts sind Teil des Zustands.* Ein Zustand speichert den Fensterbestand, alle Layouts mit Monitor, Position und Größe je Fenster sowie das aktive Layout. Ein Layout verweist auf konkrete Fenster – in einem anderen Zustand gäbe es diese womöglich nicht.

Für den Anwender bleibt es einfach: Im Alltag existiert nur `default`, die Layouts gehören also gefühlt zum Kontext. Ein weiterer Zustand ist ein Schnappschuss des ganzen Kontexts samt Layouts – nur für einen grundlegend anderen Arbeitsmodus.

```toml
# ~/.local/share/koje/kundenportal/states/default.toml
active_layout = "code"

windows = [
  { id = "editor",  integration = "editor",   path = "~/src/kundenportal" },
  { id = "browser", integration = "browser" },
  { id = "term",    integration = "terminal", where = "dev", cwd = "/workspace" },
  { id = "spec",    command = "evince docs/spezifikation.pdf" },
]

[layout.code.windows]          # Pixel, je Monitor
editor  = { monitor = "DP-1", x = 0,    y = 0,    w = 2660, h = 2160 }
browser = { monitor = "DP-1", x = 2660, y = 0,    w = 2460, h = 1240 }
term    = { monitor = "DP-1", x = 2660, y = 1240, w = 2460, h = 920 }

[layout.review]
autosave = false
[layout.review.windows]
browser = { monitor = "DP-1", x = 0,    y = 0, w = 3170, h = 2160 }
editor  = { monitor = "DP-1", x = 3170, y = 0, w = 1950, h = 2160 }
```

=== Layout oder Zustand?

#tbl((1fr, auto),
  thead[Situation][Mittel],
  [Dieselben Programme, andere Anordnung (Code schreiben, Review, Doku lesen)], [Layout],
  [Ein Programm kommt dazu oder fällt weg], [Einfach öffnen oder schließen – landet automatisch in `default`],
  [Grundlegend anderer Arbeitsmodus mit anderen Programmen (Präsentation, Workshop)], [Zustand],
  [Sauber neu anfangen], [Zustand `fresh`],
)

// ======================================================================
== Integrationen

Programme brauchen keine Integration: Jedes Fenster wird erkannt, eingefärbt, in Layouts gespeichert und beim nächsten Start über seine ursprüngliche Kommandozeile wieder geöffnet. Integrationen sind *optional* und verbessern zwei Dinge – wie ein Programm gestartet wird und was beim Wiederherstellen gemerkt wird.

#tbl((auto, 1fr),
  thead[Integration][Leistung],
  [`generic` (immer)], [Erkennt Fenster über App-ID und Prozess; merkt sich die Kommandozeile],
  [`terminal`], [Merkt sich das Arbeitsverzeichnis der Shell; startet wahlweise im Kontext oder im Dev Container],
  [`editor`], [Merkt sich den geöffneten Ordner; öffnet eingebundene Ordner gezielt],
  [`devcontainer`], [Startet den Dev Container im inneren Podman, bevor abhängige Fenster geöffnet werden],
  [`browser`], [Überlässt Tabs der Sitzungswiederherstellung des Browsers; nimmt Links von `koje open` entgegen],
)

Eine Integration ist eine kleine Beschreibungsdatei; eigene lassen sich ergänzen:

```toml
# ~/.config/koje/integrations/editor.toml
name     = "editor"
match    = { app_id = ["code", "codium", "zed"] }   # Fenster erkennen
launch   = "{command} --new-window {path}"           # Start mit gemerkten Werten
remember = ["path"]                                  # was im Zustand landet
detect   = { path = "argv-last" }                    # woher der Wert stammt
```

#text(9pt, fill: gray)[Welche Programme ein Kontext enthält, entscheidet allein der Anwender bzw. das Basis-Image; Integrationen legen nur fest, *wie* `koje` mit ihnen umgeht.]

// ======================================================================
== Das Werkzeug: `koje`

=== Befehle

#block(breakable: false, tbl((auto, 1fr),
  thead[Befehl][Wirkung],
  [`koje up <kontext> [-s <state>]`], [Kontext starten, Fenster des Zustands öffnen und anordnen (Standard: `default`). Läuft er schon: in den Zustand wechseln],
  [`koje up <kontext> --recover | --no-recover`], [Nach einem Absturz den gesicherten Stand übernehmen bzw. verwerfen, ohne Rückfrage],
  [`koje down <kontext> [-s <state>]`], [Stand speichern (Standard: `default`), Fenster geordnet schließen, Container stoppen],
  [`koje down <kontext> --no-save`], [Herunterfahren ohne zu speichern; alle Zustände bleiben unverändert],
  [`koje save <kontext> [-s <state>]`], [Stand speichern, ohne herunterzufahren],
  [`koje state list | cp | mv | rm`], [Zustände verwalten, etwa `state cp kp default fresh`],
  [`koje layout <name> | next | prev`], [Layout wechseln],
  [`koje layout new | save | autosave | list | mv | rm`], [Layouts verwalten],
  [`koje stash` / `koje unstash <fenster>`], [Fenster in die Ablage legen bzw. ins aktive Layout holen],
  [`koje focus <kontext>`], [Zum Kontext wechseln],
  [`koje snap <zone>`], [Aktives Fenster in eine Zone setzen],
  [`koje overlay <name>` / `koje pin` / `koje unpin`], [Overlay umschalten; Fenster anheften bzw. zurückschicken],
  [`koje sleep | wake <kontext>`], [Kontext pausieren bzw. fortsetzen],
  [`koje stats` / `koje limit <kontext>`], [Speicher pro Kontext anzeigen; Limits ändern],
  [`koje open <kontext> <url | datei>`], [Link oder Datei im Kontext öffnen],
  [`koje shell <kontext> [--dev]`], [Shell im Kontext bzw. im Dev Container],
  [`koje new | edit | rm <kontext>`, `koje list`], [Kontexte anlegen, bearbeiten, entfernen, auflisten],
))

#caption[Bei `down` schließen sich `-s` und `--no-save` aus. Herunterfahren entscheidet nur, *ob* gespeichert wird; Hochfahren nur, *was* geladen wird. Gelöscht wird ausschließlich mit `state rm`.]

=== Zustände

#tbl((auto, auto, 1fr),
  thead[Name][Art][Rolle],
  [`default`], [normal], [Wird von `up` und `down` ohne `-s` gelesen bzw. geschrieben],
  [`fresh`], [normal], [Gepflegter Startzustand, geladen mit `up -s fresh`],
  [`empty`], [reserviert], [Virtuell und nur lesbar: Kontext starten, keine Fenster],
  [_beliebig_], [normal], [Eigene Schnappschüsse für besondere Arbeitsmodi],
)

*Fallback beim Laden:* Existiert der angefragte Zustand nicht, lädt `koje` erst `fresh`, dann `empty` und meldet das. Das passiert nur, wenn ein Zustand nie angelegt wurde. Die Rollennamen `default` und `fresh` sind in der `context.toml` umbenennbar; nur `empty` ist fest.

*Zustand wechseln:* Läuft der Kontext bereits, vergleicht `up -s` die offenen Fenster mit dem Ziel. Fenster in beiden bleiben offen und übernehmen die Layouts des Ziels; nur aktuell offene werden geordnet geschlossen (Programme fragen bei Ungespeichertem selbst nach); nur im Ziel vorhandene werden gestartet. Vorher wird wie bei `down` in `default` gespeichert, außer mit `--no-save`.

=== Absturzschutz

Läuft immer mit und braucht keine Einstellung:

+ `kojed` schreibt den aktuellen Stand nach jeder Änderung – kurz verzögert – nach `~/.local/state/koje/<kontext>/recovery.toml`, atomar über Temporärdatei und Umbenennen. Auch ein Stromausfall hinterlässt keine halb geschriebene Datei.
+ Bei `down` wird wie gewohnt gespeichert oder verworfen; danach wird die Datei gelöscht.
+ Findet `up` noch eine Wiederherstellungsdatei, fragt `koje`: „Kundenportal wurde nicht sauber beendet – Stand von 15:42 wiederherstellen?“

Wiederhergestellt werden Programme, Layouts und Fensterpositionen; Inhalte stellen die Programme selbst wieder her.

=== Pausieren

`koje sleep` friert den ganzen Kontext ein, einschließlich seiner Dev Containers: CPU-Last null, Fortsetzen in unter einer Sekunde. Der Speicher bleibt belegt, kann aber in den Swap ausgelagert werden. Die Fenster verschwinden vorher in der Ablage. Zu beachten: Netzwerkverbindungen können während der Pause ablaufen, und Hyprland darf eingefrorene Fenster nicht als „reagiert nicht“ melden (Einstellung prüfen).

=== Speicherorte

#tbl((auto, 1fr),
  thead[Pfad][Inhalt],
  [`~/.config/koje/config.toml`], [Globale Einstellungen: Zonen, Overlays, Standard-Limits],
  [`~/.config/koje/contexts/<kontext>/context.toml`], [Von Hand gepflegt: Name, Farbe, Image, Limits, Ordner],
  [`~/.config/koje/integrations/*.toml`], [Eigene Integrationen],
  [`~/.local/share/koje/<kontext>/states/<name>.toml`], [Zustände inklusive Layouts],
  [`~/.local/state/koje/<kontext>/recovery.toml`], [Absturzschutz; nach sauberem `down` gelöscht],
)

=== Kontext-Konfiguration (Beispiel)

```toml
name   = "kundenportal"
prefix = "KP"
color  = "#2E7D5B"
image  = "koje/base:latest"         # gemeinsames Basis-Image

mounts = ["~/src/kundenportal", "~/Austausch"]   # gleicher Pfad wie auf dem Host

[limits]
memory_soft = "6G"                  # Warnung bei 90 % und 100 %
memory_hard = "8G"                  # geordnetes Herunterfahren

[states]                            # optional, Rollen umbenennen
default = "default"
fresh   = "fresh"
```

=== Aufbau

#block(breakable: false, tbl((auto, 1fr),
  thead[Baustein][Umsetzung],
  [Aufteilung], [Cargo-Workspace: `koje-core` (Logik), `kojed` (Hintergrunddienst), `koje` (Kommandozeile), `koje-gui` (Oberfläche)],
  [Kommandozeile], [`clap`; sich ausschließende Flags über Argumentgruppen],
  [Konfiguration und Zustände], [`serde` + `toml`; Pfade nach XDG über `directories`],
  [Container], [Podman zunächst über die Kommandozeile; später über dessen REST-Schnittstelle],
  [Hyprland], [Befehls- und Ereignis-Socket direkt ansprechen],
  [Speicher], [cgroup-Werte des Containers lesen; Benachrichtigungen über die Desktop-Schnittstelle],
  [Clients ↔ `kojed`], [D-Bus (`zbus`) oder eigener Unix-Socket],
  [Oberfläche], [iced, egui oder Slint; Overlay über das Layer-Shell-Protokoll],
))

// ======================================================================
== Oberfläche

Kommandozeile und Oberfläche sind gleichwertige Clients von `kojed`. Die Oberfläche entsteht in drei Stufen:

#tbl((auto, 1fr),
  thead[Stufe][Umsetzung],
  [1 · Schnellstartmenü], [Launcher (fuzzel oder rofi) mit Kontexten und Aktionen aus `koje`],
  [2 · Leistenmodul], [Waybar-Modul mit Kontext, Layouts, Ablage und Speicher (Mockup im Abschnitt Layouts)],
  [3 · Panel], [Eigenes Overlay in Rust über das Layer-Shell-Protokoll; Zustände, Layouts, Speicher],
)

#let lrow(c, name, status, sel: false) = block(fill: if sel { dark2 } else { none }, inset: (x: 6pt, y: 4.5pt), radius: 3pt, width: 100%, below: 1pt)[
  #set text(8.5pt, fill: fg); #dot(c) #h(4pt) #name #h(1fr) #text(fill: muted, status)]
#let arow(t, key: "", sel: false) = block(fill: if sel { dark2 } else { none }, inset: (x: 6pt, y: 4.5pt), radius: 3pt, width: 100%, below: 1pt)[
  #set text(8.5pt, fill: fg); #t #h(1fr) #text(fill: muted, raw(key))]
#let launcher(prompt, body) = block(fill: dark, radius: 8pt, inset: 8pt, width: 100%, breakable: false)[
  #block(fill: dark2, radius: 4pt, inset: (x: 7pt, y: 5pt), width: 100%, below: 6pt)[#text(9pt, fill: fg)[#text(fill: muted)[›] #prompt▏]]
  #body]

==== Mockup: Schnellstartmenü

#block(breakable: false)[
#grid(columns: (1fr, 1fr), gutter: 10pt,
  launcher("")[
    #lrow(c-kp, [*KP* · Kundenportal], "läuft · code", sel: true)
    #lrow(c-sh, [*SH* · Shop-Relaunch], "pausiert")
    #lrow(c-ib, [*IB* · Infra-Backup], "gestoppt")
    #lrow(c-ag, [*AG* · Agentur-Website], "gestoppt")
    #lrow(c-ws, [*WS* · Workshop-Demo], "gestoppt")
  ],
  launcher(text(fill: fg)[KP · ])[
    #arow([Zum Kontext wechseln], key: "enter", sel: true)
    #arow([Layout wechseln …], key: "layout")
    #arow([Herunterfahren (in default speichern)], key: "down")
    #arow([Herunterfahren ohne Speichern], key: "down --no-save")
    #arow([Pausieren], key: "sleep")
    #arow([Neu starten mit Zustand …], key: "up -s")
  ],
)
#caption[Links: Kontexte mit Farbe und Laufzustand, Tippen filtert. Rechts: Aktionen für den gewählten Kontext; bei einem gestoppten steht oben „Starten“, darunter „Starten mit fresh“ und „Starten leer“.]
]

#let staterow(name, info, wins, sel: false) = block(fill: if sel { rgb("#eef3f9") } else { white }, stroke: 0.5pt + line-c, radius: 4pt, inset: 6pt, width: 100%, below: 4pt, breakable: false)[
  #grid(columns: (4.6cm, 1fr), gutter: 8pt, lmon(wins, h: 1.94cm),
    [#text(9pt)[*#name*] \ #text(8pt, fill: gray, info) #v(0.3em)
     #text(7.5pt, fill: navy)[Laden · Kopieren · Umbenennen · Löschen]])]
#let prow(c, n, s, sel: false) = block(fill: if sel { white } else { none }, stroke: if sel { 0.5pt + line-c } else { none }, radius: 3pt, inset: (x: 5pt, y: 4pt), width: 100%, below: 2pt)[#box(width: 3pt, height: 1.1em, fill: c, radius: 1pt, baseline: 25%) #h(3pt) #text(8.5pt)[*#n*] #h(1fr) #text(7.5pt, fill: gray, s)]

#block(breakable: false)[
==== Mockup: Panel, Reiter „Zustände“
#block(fill: rgb("#f7f9fc"), stroke: 0.8pt + line-c, radius: 8pt, inset: 0pt, width: 100%, clip: true)[
  #grid(columns: (4.8cm, 1fr), fill: (x, _) => if x == 0 { rgb("#edf1f7") },
    block(inset: 8pt, width: 100%)[
      #text(8pt, fill: gray)[KONTEXTE] #v(0.2em)
      #prow(c-kp, "Kundenportal", "läuft", sel: true)
      #prow(c-sh, "Shop-Relaunch", "pausiert")
      #prow(c-ib, "Infra-Backup", "aus")
      #prow(c-ag, "Agentur-Website", "aus")
      #prow(c-ws, "Workshop-Demo", "aus")
      #v(0.6em) #text(8pt, fill: navy)[\+ Neuer Kontext]
    ],
    block(inset: 10pt, width: 100%)[
      #box(width: 10pt, height: 10pt, fill: c-kp, radius: 2pt, baseline: 15%) #h(4pt) #text(11pt)[*Kundenportal*] #text(9pt, fill: gray)[· KP · läuft seit 09:12]
      #h(1fr) #box(fill: white, stroke: 0.6pt + line-c, inset: (x: 6pt, y: 3pt), radius: 3pt, text(8pt)[Pausieren]) #box(fill: c-kp, inset: (x: 6pt, y: 3pt), radius: 3pt, text(8pt, fill: white)[Herunterfahren ▾])
      #v(0.1em)
      #text(8pt)[#underline[*Zustände*] · Layouts · Fenster · Speicher] #h(1fr) #text(8pt, fill: gray)[4 Fenster · 3 Layouts · 5,5 GB]
      #v(0.4em)
      #staterow("default", "zuletzt gespeichert heute 08:57 · 4 Fenster · 3 Layouts", L-code, sel: true)
      #staterow("fresh", "gepflegter Startzustand · 3 Fenster · 1 Layout", ((0.01,0.02,0.45,0.97,[Editor]),(0.46,0.02,0.36,0.56,[Browser]),(0.46,0.58,0.36,0.41,[Terminal])))
      #staterow("workshop", "angelegt 10. Okt · 2 Fenster · 1 Layout", ((0.01,0.02,0.6,0.97,[Präsentation]),(0.61,0.02,0.38,0.97,[Terminal])))
    ],
  )
]
#caption[Die Vorschau zeigt jeweils das aktive Layout eines Zustands im Seitenverhältnis 5120 × 2160.]
]

// ======================================================================
== Nächste Schritte

+ Hyprland einrichten: alle Fenster schwebend, Mausbelegung, Kantenmagnetismus, Overlays `mail` und `chat`; eine Woche im Alltag testen.
+ Basis-Image bauen und einen Kontext-Container von Hand starten: Wayland, PipeWire, Grafik, gefilterter D-Bus, Home-Volume.
+ Podman in Podman im Kontext einrichten, ein Compose-Projekt und einen Dev Container darin starten; gemeinsamen Image-Speicher testen.
+ Prüfen, dass sich Bibliotheken zwischen zwei Kontexten den Speicher teilen (gleiches Basis-Image, natives overlayfs).
+ Austauschordner einbinden und Ziehen von Dateien in ein Overlay-Programm testen.
+ Prototyp `kojed`: Fenster per Prozess-ID und cgroup zuordnen, Rahmenfarbe setzen, Speicher überwachen.
+ Prototyp Layoutwechsel: Fenster gebündelt verschieben, in die Ablage und zurück; Tempo und Animation beurteilen.
+ Erste Version von `koje up` und `koje down`: zunächst `empty` und `fresh`, danach `default` und Absturzschutz.
+ Schnellstartmenü als erste Oberfläche.

== Quellen

- Hyprland IPC (Befehls- und Ereignis-Socket): #link("https://wiki.hypr.land/IPC/")[wiki.hypr.land/IPC]
- Hyprland Berechtigungen: #link("https://wiki.hypr.land/Configuring/Advanced-and-Cool/Permissions/")[wiki.hypr.land – Permissions]
- Podman, zusätzliche Image-Speicher: #link("https://redhat.com/sysadmin/image-stores-podman")[Red Hat – Exploring additional image stores]
- Minimieren unter Hyprland per Special Workspace: #link("https://github.com/Alexays/Waybar/issues/3374")[Waybar Issue 3374]
- Podman-Checkpoints mit CRIU: #link("https://fedoramagazine.org/podman-checkpoint-in-fedora-linux/")[Fedora Magazine – Podman Checkpoint]

// ======================================================================
#pagebreak()
== Anhang: Offene Punkte

Diese Fragen sind noch nicht entschieden oder müssen an einem Prototyp geprüft werden.

=== Container und Isolation

#tbl((1fr, 1.7fr),
  thead[Punkt][Offene Frage],
  [Podman in Podman], [Welche Einstellungen braucht verschachteltes rootless Podman im Basis-Image (Sub-UIDs und -GIDs, `/dev/fuse`, Storage-Treiber)? Wie stabil läuft es im Alltag?],
  [Gemeinsamer Image-Speicher], [Lässt sich ein schreibgeschützter Image-Speicher (`additionalimagestores`) aus dem inneren Podman nutzen, und wie wird er aktualisiert?],
  [D-Bus-Filter], [Welche Schnittstellen erlaubt der Proxy genau? Gesetzt: Benachrichtigungen und Dateiauswahl ja, Öffnen von Links auf dem Host nein. Offen: Bildschirmfreigabe, Kalender, Schlüsselbund],
  [Gleiche Pfade], [Bindet `koje` Home-Volume und Ordner zuverlässig unter demselben Pfad wie auf dem Host ein, und klappt das Ziehen von Dateien in Overlay-Programme?],
  [Kontext `global`], [Ist es vertretbar, dass Overlay-Programme nicht isoliert laufen und alle eingebundenen Ordner sehen?],
  [Berechtigungen in Hyprland], [Wie werden Bildschirmaufnahmen für Programme aus Kontexten gesperrt, ohne gewollte Bildschirmfreigaben (etwa in Videokonferenzen) zu verhindern?],
)

=== Speicher

#tbl((1fr, 1.7fr),
  thead[Punkt][Offene Frage],
  [Geteilte Bibliotheken], [Teilen sich zwei Kontexte mit gleichem Basis-Image tatsächlich die Speicherseiten der Bibliotheken? Messung am Prototyp],
  [Standard-Limits], [Welche Soft- und Hard-Limits gelten ohne Angabe? Fester Wert oder Anteil am Arbeitsspeicher?],
  [Hard-Limit], [Wie lange darf das geordnete Herunterfahren dauern, bevor `koje` hart beendet? Wie groß ist der Abstand zur Kernel-Grenze `memory.max`?],
  [Swap], [Soll Swap pro Kontext begrenzt werden, und zählt er zum Limit?],
)

=== Desktop und Layouts

#tbl((1fr, 1.7fr),
  thead[Punkt][Offene Frage],
  [Geschlossene Fenster], [Verschwinden sie aus allen Layouts (derzeit vorgesehen), oder bleibt ein Platzhalter, den ein erneut gestartetes Programm wieder belegt?],
  [Mehrere Monitore], [Wie verhält sich ein Layout, wenn ein gespeicherter Monitor fehlt? Gilt ein Kontext-Workspace pro Monitor oder einer für alle?],
  [Angeheftete Fenster], [Gehört ein aus dem Overlay angeheftetes Fenster zum aktiven Layout, oder schwebt es über allen?],
  [Zuordnung gleicher Programme], [Wie zuverlässig lassen sich mehrere Fenster desselben Programms beim Wiederherstellen und beim Zustandswechsel zuordnen?],
  [Pausierte Kontexte], [Lässt sich die „reagiert nicht“-Meldung von Hyprland für eingefrorene Fenster gezielt unterdrücken?],
  [Tempo des Layoutwechsels], [Wirkt der Wechsel mit gebündelten Befehlen flüssig, und welche Animation passt?],
  [Hyprland-Versionen], [Wie fängt `koje` Änderungen an Konfigurationssyntax und Schnittstelle von Hyprland ab?],
)

=== Werkzeug und Oberfläche

#tbl((1fr, 1.7fr),
  thead[Punkt][Offene Frage],
  [Integrationen], [Reicht ein deklaratives Format, oder brauchen manche Integrationen ausführbare Hooks? Wie erkennt `terminal` das Arbeitsverzeichnis zuverlässig?],
  [Fenster-IDs], [Nach welcher Regel vergibt `koje` IDs, und wie bleiben sie über Neustarts stabil?],
  [Schnittstelle zu `kojed`], [D-Bus oder eigener Unix-Socket?],
  [Podman-Anbindung], [Ab wann lohnt der Wechsel von der Kommandozeile zur REST-Schnittstelle?],
  [Oberflächen-Toolkit], [iced, egui oder Slint für das Panel?],
  [Sprache], [Befehle, Hilfe und Meldungen auf Englisch, Deutsch oder beides?],
  [Lizenz und Veröffentlichung], [Wird `koje` Open Source, und unter welcher Lizenz?],
)

#pagebreak()
== Anhang: Ausblick

Ideen, die heute nicht umsetzbar oder bewusst zurückgestellt sind, aber später interessant werden können.

=== Echter Ruhezustand per CRIU

Das Pausieren hält den Speicher eines Kontexts belegt. Ein echter Ruhezustand würde den Kontext samt Arbeitsspeicher auf die Platte schreiben und später exakt so wiederherstellen – CPU *und* RAM wären frei. Podman unterstützt das über CRIU (`podman container checkpoint` und `restore`).

Heute scheitert das an zwei Punkten:

- *Nur mit Root:* Checkpoints funktionieren nur für Container, die als Root laufen; `koje` arbeitet rootless.
- *Offene Verbindungen nach außen:* CRIU kann Verbindungen zu Prozessen außerhalb des Containers kaum sichern. Jedes grafische Programm hängt am Wayland-Socket von Hyprland.

Interessant wird es, wenn CRIU rootless nutzbar wird. Ein erster Schritt könnten Kontexte oder Dev Containers ohne grafische Programme sein – etwa Datenbanken und Hintergrunddienste eines Projekts. #link("https://fedoramagazine.org/podman-checkpoint-in-fedora-linux/")[Quelle]

=== Weitere Ideen

#tbl((1fr, 1.7fr),
  thead[Idee][Beschreibung],
  [Flug-in-die-Leiste], [Fenster fliegen beim Ablegen sichtbar in ihr Symbol in der Leiste; bräuchte ein Hyprland-Plugin],
  [Zonen mit Vorschau], [Fenster in vordefinierte Zonen ziehen, mit Vorschau während des Ziehens; bräuchte ein Hyprland-Plugin],
  [Titelleisten in Kontextfarbe], [Farbige Titelleisten über das offizielle Hyprland-Plugin, zusätzlich zum farbigen Rahmen],
)
