# Desktop app environments from a request

Use this when someone asks for an environment of a Linux desktop app with a
task, such as "Build me an Inkscape environment where the agent changes a
poster's headline and saves it". You are the building agent: you drive the
app on a build computer through REA's MCP tools, write the task, export the
bundle and witness it in a fresh computer. REA runs no model. The tools are
MCP-only and need a Linux host with Docker, agentenv and REA's packed
runtime. When a start reports one missing, tell the person to run
`rea doctor --provider desktop_build` and apply its fixes, or
`rea setup --client <client> --desktop-build` and restart the client, and
stop.

For the whole build:

- Drive the app only with `act_on_build_computer`. Never script it, run
  commands in the computer or write its files another way. The one command
  you write for the computer is a task's reader, which REA runs to check a
  file; it never makes the seed or the edit.
- Ask the upfront questions once, then work on your own. Pause only when
  blocked.
- The seed's content is synthetic. Never put real personal data in it.
- Close every computer you start, on success and on failure.
- Note the time you start; the report gives the build time.

## Ask once, upfront

In one message, ask only what the request leaves open, and keep the list for
the report. Skip each question the request answers; when it answers them all,
start without asking:

1. The app and the task, if either is unclear: what changes, in which
   document.
2. Whether they have a starting file of their own; otherwise you make one.
3. Whether they want the tasks measured after the witness: with which
   model, within which budget, and whether to harden a task until it lands
   in the band.
4. If so, whether they want to review the tasks before those solves.

When you cannot ask, as in a headless `claude -p` run, do not stop: take
these defaults for whatever the request leaves open, say which you took in
your first message and in the report, and go on. Install from the Ubuntu
archive, make a synthetic seed, skip the review, and measure each task
within $4 in all, with the model the request names, without hardening.

## When you are blocked

Stop, say what you need and wait only for:

- a licence the app or its installer asks someone to accept: never accept it
  on your own; name it and where to read it, and go on only once the person
  says they accept it. Then record their words with `record_build_consent`,
  kind `licence`, and accept it on their behalf;
- credentials: ask the person to write each secret, and nothing else, in a
  file named after a handle you choose, such as `keepassxc-master`, in
  `~/.config/rea/build-secrets/` on this host (under `$XDG_CONFIG_HOME` when
  it is set), readable only by them (`chmod 600`), and wait until they say it
  is there; then type it with `type_secret` (see Drive the app). Never ask
  for a password or key in chat, and never read, list or print that folder;
- a signed-in state in the seed: by default the export withholds it (see
  Sign-in and licence state). Seed it only when the person says the bundle
  may hold it: record their words with `record_build_consent`, kind
  `seed_sign_in`, and export with `seed_sign_in: true`;
- an install this flow cannot do, below.

Decide everything else yourself and note it in the report.

## Sign-in and licence state

What an app writes when someone signs in or activates a licence is withheld
from the seed: the bundle's receiver supplies their own. Mark
`pre-sign-in` with `mark_build_checkpoint` just before the sign-in or
activation and `post-sign-in` just after, both before `seed`: every file
under the owned paths that changed between them is withheld, and the draft
leaves it out. A file the app writes at first launch, such as a trial's
licence file, changes before you can mark anything: name it in
`export_build_environment`'s `withhold`. The export lists each withheld file
in `withheld_files` with the environment variable a deploy reads it from and
the handle of the secret REA keeps its bytes in; a check may not name one.
The export refuses a seed file holding a withheld file's exact bytes, such as
a `.bak` copy, but a backup that differs from it is not caught: trash backups
before `seed`.
Explicitly withhold any other files the person identifies as sensitive;
REA does not infer secrecy from a value's shape or a file name.

## Choose the install

The computer is Ubuntu 24.04 (noble) on this host's architecture only. Use
the first source that has the app:

| Source                       | `install`                                                                                   |
| ---------------------------- | ------------------------------------------------------------------------------------------- |
| Ubuntu archive               | `{"kind": "archive", "package": "inkscape"}`                                                |
| PPA or vendor apt repository | `{"kind": "apt_repository", "package", "uri", "suite", "components", "key_file"}`           |
| `.deb` by URL                | `{"kind": "package_file", "url", "sha256", "architecture", "dependencies"}`                 |
| AppImage by URL              | `{"kind": "appimage", "url", "sha256", "architecture", "extra_packages"}`                   |
| Tarball by URL               | `{"kind": "tarball", "url", "sha256", "architecture", "program", "name", "extra_packages"}` |

- A PPA's `uri` is `https://ppa.launchpadcontent.net/OWNER/NAME/ubuntu`,
  `suite` `noble`, `components` `["main"]`. `key_file` is the repository's
  signing key saved on this host; for a PPA, fetch it from
  keyserver.ubuntu.com by the fingerprint its Launchpad page shows.
- When the app exits at launch for want of a runtime piece its package does
  not install, start a new build computer with that package in
  `extra_packages`, up to 16 archive names; the export installs them too.
  MyPaint needs
  `{"kind": "archive", "package": "mypaint", "extra_packages": ["librsvg2-common"]}`
  for its SVG icons, and Freeplane `["openjdk-17-jre"]`, since its launcher
  refuses noble's Java 21. Name one up front, too, when a package the app
  only recommends holds its content, such as QElectroTech's element library
  `qelectrotech-data`, without which no component can be placed.
- Take the `.deb` path only when the app is not in the archive at all, not
  to add a package. Ask the requester for its URL and SHA-256; do not fetch
  a vendor's download page yourself. List its `Depends` as `dependencies`,
  with any runtime piece it lacks: it installs with the network off.
- Take an AppImage or a tarball only when no package has the app. Pin it by
  the release file's URL for this host's architecture and the SHA-256 its
  project publishes, such as a GitHub release asset's digest or the
  release's `SHA256SUMS`; ask the requester when the project publishes none.
  The computer downloads it when the bundle is put, refuses other bytes, and
  unpacks it under `/opt/rea-apps/<desktop_entry>` with the network off; the
  export downloads the same file.
- An AppImage installs its own desktop entry, so `desktop_entry` is the
  `.desktop` file at the top of the image, such as FreeCAD's
  `org.freecad.FreeCAD`. A wrong name fails the start with the entries the
  image has: start again with one of them. The image brings its libraries,
  so `extra_packages` is rarely needed; FreeCAD 1.1.4's needs none.
- A tarball has no entry of its own. `program` is the program's path inside
  the archive, such as `firefox/firefox`; REA writes an entry named
  `desktop_entry` that runs it with the files it opens, and `name` is what
  the Applications menu shows. A tarball uses the computer's libraries: name
  what its program lacks in `extra_packages`. gzip, bzip2 and xz archives
  are read, zstd ones are not.
- Flatpak and snaps are not supported: say so and stop. Flatpak's sandbox
  needs user namespaces the computer's container does not grant, so a
  Flathub app installs but never opens; ask for the project's AppImage,
  tarball or `.deb` when it ships one. An archive package that only installs
  a snap, such as noble's `firefox`, counts as a snap. A package with no
  build for this host's architecture (some PPAs are amd64 only) cannot be
  installed either.
- `desktop_entry` is the package's `.desktop` file name under
  `/usr/share/applications` without `.desktop`, such as
  `org.inkscape.Inkscape`. Add `window_class` when you know it; Inkscape's
  is `Inkscape`. AbiWord's entry is `abiword` and its class `Abiword`.
- To find the entry, read the package's file list: its page on
  packages.ubuntu.com, or `apt-get download PKG && dpkg -c PKG_*.deb` where
  apt runs. The entry may ship in a companion `-data` or `-common` package,
  as Planner's in `planner-data`: read that package's list too. The entry's `StartupWMClass` may not be the window's real
  class, so do not take `window_class` from it: MuseScore 3's says
  `mscore3`, its window's is `MuseScore3`. The export uses the class the
  capture saw.

## Drive the app

- Pass `computer_id` and the action's fields at the top level: click
  `{x, y, count}`, scroll `{x, y, direction, amount}`, a screenshot region
  `[x0, y0, x1, y1]`. The rest are in
  [the build-computer guide](https://github.com/morluto/rea/blob/main/docs/desktop-build-computer.md).
- Take a `screenshot` before pointing: points are pixels of the latest
  frame. If a response is too large, ask for a `region`, or `jpeg` with a
  lower `quality`. A screenshot with `region` does not capture the screen
  again; it crops the latest frame, so take a full screenshot, or a `wait`
  with `screenshot: true`, first. The crop then becomes the latest frame:
  take a full screenshot before pointing again.
- A drawn caret does not prove keyboard focus. After using a toolbar
  control, click into the document, then type a few characters and look
  before a long text.
- Confirm a dialog with `key` Return, not a trailing `\n` in `type`.
- An action's own screenshot is taken at once. After opening a menu, a
  dialog or a window, look with
  `{"action": "wait", "ms": 500, "screenshot": true}`.
- On a computer started from a request the app is not open at first, and
  `start_build_capture` opens it: never open it yourself before that. Do not
  close it while capturing; the capture ends when the app exits. When the
  app is closed at any other time, open it from Applications → Other → the
  environment's name.
- Type a secret with `{"action": "type_secret", "secret": "<handle>"}`,
  only into a field that masks what is typed: check in a screenshot that it
  is a password field, which shows dots. A screenshot shows any field that
  shows its text, such as many licence-key boxes, so never type a secret into
  one: say the app needs a secret the build cannot type unseen, and stop.
  `type_secret` takes no `screenshot`; look with a `wait` after it. A refusal
  names the secret's file and what is wrong with it: tell the person, never
  the value.
- Prefer shortcuts and typing to long pointer paths: `key` takes `keys`, an
  array holding one chord's keys, such as
  `{"action": "key", "keys": ["Control", "s"]}`, and `type` turns `\n` into
  Enter.
- Before `type`, check in a screenshot that a text cursor or a focused
  field is there. In apps with single-key shortcuts, such as Inkscape, text
  typed without one runs as commands.
- Save As differs by app. Check the dialog's current folder first: most
  open in the home, so click Documents in the sidebar, but Scribus opens in
  `~/Documents` and its sidebar has no Documents entry. Type only the file
  name. A dialog taller than the 1280×800 screen hides its buttons:
  maximize it from its title bar, or confirm it with `key` Return, as when
  HomeBank's Save button is off-screen. When a GTK Save dialog ignores
  clicks or runs off the screen, type `~/Documents/<name>` in its Name field
  and press Return.
- Answer the app's save questions, such as keeping the format, then check
  the saved path in the title or status bar, or the dialog's folder when
  the app shows only the name, and that it shows no unsaved mark, before
  you mark a checkpoint or check. Where the title never shows one, as in
  HomeBank, a greyed Save or Revert, or the file reopened from recent
  files, confirms the save.
- Qt spin boxes change their value on scroll: move the pointer off them
  before you scroll.
- gLabels edits an object's text in Object properties → Text, not on the
  canvas.
- In Grisbi's transaction form Return moves to the next field; only Return
  in the last field, Notes, records the transaction.
- ghostwriter autosaves a new document as `~/Documents/untitled-1.md`: name
  it with File → Rename, not Save As; Ctrl+Shift+S does nothing, and Enter
  after a checklist item starts another `- [ ]` item.
- KMyMoney's first launch opens the New File Setup wizard; Finish asks for a
  storage type (XML), then a Save As where typing `Documents/<name>.kmy`
  works. Its date field is a sectioned Qt editor: double-click the month and
  type each part, since Ctrl+A garbles it. A new transaction's payee comes
  prefilled and selected. Amounts are saved as reduced fractions, such as
  `-1749/20`, so compare them exactly. Untick Help → Tip of the Day → "Show
  tips on startup" before `seed`, or the tip appears at every boot.
- QElectroTech: an element dropped from Collections follows the pointer until
  a click on the folio places it. Its title shows only the app, so it needs
  `state_paths`; its settings are in `~/.qet`.
- Rosegarden: after a dialog its Transport window can take keyboard focus,
  so save with File → Save.
- Pencil2D: the Tools dock is too narrow for its buttons, so use shortcuts
  (B for the brush); double-click a layer's name to rename it.
- Umbrello: the title shows no unsaved mark, so confirm a save with a probe
  checkpoint. An Untitled document autosaves `~/autosave.xmi`: delete it
  before `seed`. Add attributes from the tree: right-click the class → New →
  Attribute.
- Hydrogen: the computer has no audio device, so Hydrogen falls back to a
  null driver and its BPM field snaps back to 120 within about 100 ms.
  Before `seed`, set Preferences → Audio System → ALSA with device `null`
  and click "Apply and restart output", which `~/.hydrogen/hydrogen.conf`
  keeps for the seed; also set autosaves per hour to 0, and dismiss the
  development-version dialog with "Don't show again". `pulseaudio` as an
  extra package does not help.
- Poedit cannot add entries by hand. Use File → New, pick the language and
  save to `Documents` first (Sources Paths stays locked until then), then
  Properties → Sources Paths → + → Add Files with a source such as
  `/usr/lib/python3.12/argparse.py`. It writes `fr.mo` beside the `.po`, and
  the language box's autocomplete popup takes the first OK click.
- CherryTree: its first-run autosave "Choose Storage Type" dialog swallows
  keys, so save once early as `.ctd` and untick autosave.
- VSCodium (its tarball, with `extra_packages: ["libnss3",
  "libasound2t64"]`) runs on outside REA's reach, so `stop_build_capture`
  fails to clean it up: after `end`, quit it with Ctrl+Q, then stop. Enter
  accepts its word suggestions, so press Escape after typing a line.
- Glade: a new widget's id is empty until you set it, and a second save
  writes a `.glade~` backup, which shows in the draft as an unrelated
  change. A backup present at `seed` is seeded with the document, so trash
  it before `seed`. The draft names a button's `<child>` by the id
  its widget carries, `[name=child][>object@id=submit_button]`, which moving
  the button keeps; give the widget an id before `seed` so it can.
- VYM: the title never names the map, so pass `Documents/<map>.vym` in
  `state_paths`. Its settings live in `.config/InSilmaril` and are written
  only at a clean exit. It writes `<map>.vym.lock` while a map is open, so
  close the map before `seed`. Press A only after clicking a branch on the
  canvas: keys typed in the tree editor run other shortcuts.
- KmPlot: Create → Cartesian Plot, then edit the formula in the Function
  tab and press Return. It stores minus as U+2212 (`−`). Its file:// title
  still names the file.
- RedNotebook keeps each month in `~/.rednotebook/data/YYYY-MM.txt`, YAML
  read as text, folded at about column 80: pick check phrases the fold keeps
  on one line.
- Qt5 apps saving with QDom, such as KmPlot, write attributes in a random
  order in each process, so check their XML by element, never by whole line.
- Gnumeric's desktop entry is `org.gnumeric.gnumeric`. Its Save As dialog
  runs off the 1280×800 screen: type `~/Documents/<name>` in Name and press
  Return. Rename a sheet with right-click on its tab, then Rename….
- Parley's title shows the collection's title, so name the file in
  `state_paths`. File → New opens the new collection's Properties dialog:
  confirm it with OK, since Cancel keeps the old collection and its autosave
  lock open. A new collection starts with 15 blank entries: delete them.
  Pick languages from the dropdown; a typed name keeps the previous locale.
  While a document is open KDE keeps an autosave and its `.lock` in
  `~/.local/share/stalefiles/parley/`, and there is no File → Close: before
  `seed`, use File → New, then reopen the collection from Open Recent.
- Sigil: pass `extra_packages: ["libqt5svg5"]`, or its SVG toolbar icons are
  blank. Its title names the book, so it needs no `state_paths`. It unpacks
  the open book into `.local/share/sigil-ebook/sigil/workspace/Sigil-XXXXXX/`
  and deletes it on exit, so never name the whole `sigil-ebook` folder: that
  seeds the stale copy.
- What the app saved under the home before `seed`, such as a document in
  `~/Documents` or its settings, is part of the seed. A file you move to the
  Trash is left out, and the export lists it in `limitations`. Trash a stray
  file rather than starting over. JabRef 3.8 writes `<file>.bak` and
  `<file>.sav` beside a `.bib` it saves; in the owned folder they are seeded
  and show in the draft as noise, so trash them before `seed` if they should
  not ship.
- Dismiss first-run dialogs, such as a welcome screen, before `seed`. A
  choice the app saves only later, such as Inkscape's "show this every
  time", may not reach the seed, so expect the dialog in the witness too.
- Some apps write their settings late: Freeplane writes
  `~/.config/freeplane/1.7.x/auto.properties` only once Preferences is
  confirmed, and a seed without it reopens its documentation map. Before
  `seed`, open the app's preferences and confirm them, then read a probe
  checkpoint's `changed_files` to see the settings file written.
- Before `seed`, click into the document to clear any selection the app
  would save: Gnote saves its cursor and selection with the note.
- An app whose state folder is named after it, such as
  `~/.local/share/gnote`, or after it and its version, such as TreeLine's
  `~/.treeline-3.1`, is found without `state_paths`.
- TreeLine: File → New opens a template window. Book List's CATEGORY makes
  AUTHOR children by default; change it under Data → Configure Data Types →
  Default Child Type. A new child opens an inline rename showing "", so
  press Escape and fill its fields in the Data Edit tab. Ctrl+A adds a child
  node even while a field has focus. Clicking a node already selected starts
  an inline rename, so Ctrl+X then cuts its text: cut a node from its
  context menu. It saves nodes sorted by their
  generated uid1, so their order changes from run to run. The draft names a
  new node by its format and a field of its `data`,
  `/nodes/[format=BOOK][>data~1Title=The Silk Roads]`, and checks its other
  fields through that, so give each new node a title no other node has.
- After an app crashes, capture again on a fresh computer, so the seed
  carries no crash-recovery state, such as MyPaint's "Recover interrupted
  work?".
- Give what the task changes a stable name where the app has one, such as an
  id in Inkscape's Object Properties, so a check can select it.
- Expect apps to split text into runs: Writer's revision ids split an
  edited paragraph into spans and may give it an automatic style such as
  `P1`, and AbiWord wraps each insertion in `<c>`. Check such text whole
  with `text_equals` (see Write the task), and find the paragraph by
  something the edit keeps, such as AbiWord's `xid`.
- Freeplane's first launch opens a documentation map: close it with Ctrl+W.
  Its Ctrl+N asks for a template.
- MyPaint 2.0.1 crashes on brush strokes on the build computer: build its
  seed with flood fill and layer operations.
- GnuCash's Ctrl+S on a new book opens a Save dialog with no format choice:
  use File > Save As to pick XML. Close the book (File > New File) before
  `seed`, so its `.LCK` lock is not seeded.

## Build the seed and the edit

1. `start_build_computer` with
   `{"source": {"kind": "request", "desktop_entry": ..., "install": ...}}`.
   The first start of an app builds its image, and starts at the same time
   on this host wait for each other; either can take minutes. A failed
   start returns the build's error: fix the request, or stop when the
   install cannot work, and close any `computer_id` the error names.
2. `start_build_capture`. It returns once the app's window is open. A
   `package_lists_error` may stop the capture finding the package's
   sources; the export says when that matters. When the app exits before
   its window opens, the error gives its exit code and the end of its
   stderr, which may name what it lacks, such as an icon loader or a Java
   runtime: close the computer and start a new one with that package in
   `extra_packages` (see Choose the install), or stop as for an install
   this flow cannot do.
   - REA counts a file as the app's document only when the app's window
     title shows its file name or `~/` path. When the title shows something
     else, such as MuseScore's score title, pass the path you will save the
     document at in `state_paths`, relative to the home:
     `{"computer_id": ..., "state_paths": ["Documents/MuseScore3/Scores/Morning_Song.mscz"]}`.
     It need not exist yet. Add the app's settings folder when its name
     differs from the app's, such as `.config/MuseScore`, so the seed keeps
     the dismissed first-run dialogs. Do not pass the document as a launch
     argument to make REA see it: `state_paths` is the way. gLabels' title
     leaves out the file's extension and Grisbi's shows the entity's name,
     so both need it.
   - An app whose document is a folder of pages, such as a Zim notebook
     (`Notebooks/Notes`, a `.txt` file per page), never shows a file in its
     title: pass the folder in `state_paths`. Every page in it is then a
     document, so the bundle opens none at launch and the app starts where
     it keeps its place; check the page the task changes.
3. Make the seed through the app and save it from the app under
   `~/Documents`: build the document in the app, or write its source where
   the app edits its format as text, then save it again from the app. Make
   it look real but synthetic: the thing the task changes, plus a few things
   the change must leave alone. Keep it to one document: the bundle's app
   opens the seed's document at launch only when there is exactly one, and
   otherwise starts on its own screen. For a starting file of the requester's,
   read it here and rebuild it in the app with synthetic data in place of
   personal data; REA cannot copy a file into the computer.
   - An app that keeps its own data and saves after every change, such as
     Osmo's contacts in `~/.local/share/osmo/contacts_records.xml`, has no
     document: that state is the seed. Build it in the app and mark `seed`
     once the app has written. To see whether it has, mark a throwaway
     checkpoint, such as `probe-1`, and read its `changed_files`: up to ten
     files that changed since the checkpoint before, those a window title
     named first, then those under `state_paths` and those the app held
     open, caches last, with `more_changed_files` counting the rest.
   - The app's own compressed format works when it is gzip, such as
     Gnumeric's `.gnumeric`, Dia's `.dia` or Inkscape's `.svgz`: REA reads
     the document inside, and checks point into it as into the same
     document uncompressed. Compression may be off by default: Dia's
     "Compress diagram files" is unticked, which REA reads as plain XML. A format REA reads as binary, such as one
     compressed with xz, has no view: save in a format the app offers that
     REA reads, then reopen the file once and check that a plain Ctrl+S
     keeps that format. When the task is about the app's own binary
     format, such as GIMP's `.xcf` or Blender's `.blend`, keep it and check
     it through a reader (see Read a binary file with a reader).
4. Only once the seed is saved, `mark_build_checkpoint` with name `seed`.
5. Make the change the task asks for, as a user would, and save it.
6. Only once the change is saved, `mark_build_checkpoint` with name `end`.
   For more tasks in this environment, make each next task's change on top
   of the last as a user would, save it, and mark `end-<task name>`, such as
   `end-add-bread`. Each task is checked at its own end, so its checks must
   not read what a later task changes, and a solving agent starts each from
   the seed, so a task must not need an earlier one done first.
   Then, for each task you can, make a near miss of its change on top, a
   plausible wrong edit such as a typo or a wrong number, save it and mark
   `near-<task name>`: the export refuses a task whose near miss passes every
   check. Take the near misses after the last end, since each later draft
   starts from the end before it.
7. `stop_build_capture`. Read `draft.files` (each file that changed, with
   its net change and its `values`), `draft.task.checks` (suggested
   checks), `draft.guards`, `draft.notes` and `limitations`; each
   `later_drafts` entry is the same for one later task, from the end
   checkpoint before its own, so it shows only that task's change. When
   `draft` is null, read `draft_refusal` first: a missing `seed` or `end`
   checkpoint means capturing again on a new build computer, while a failed
   draft command names its own cause to fix. If a declared secret was used,
   automatic drafting is withheld: write the task manually and export it.
   When `draft.files` is empty,
   `draft.notes` says why:
   - `no_change`: nothing was saved between `seed` and `end`. Capture again
     on a new build computer, and save before marking `end`. When the app
     keeps the task's state only on screen, such as a calculator's result,
     judge the screen instead (see Judge the screen).
   - `unowned_changes` with `no_document`: the app saved your document
     where REA did not count it as the app's. Its `files` list what changed,
     with their roles; the document is among them. Close the computer and
     capture again on a new one with that path in `state_paths`; the export
     refuses a check on a file outside the owned paths.
   - `uncheckable_changes`: the app saved the change in dconf, as gedit does
     its preferences, and no check reads dconf. Saving again does not help:
     choose a change the app saves in a file, and capture again on a new
     build computer, or judge the screen (see Judge the screen).
   - `unreadable_changes`: a dconf database changed but REA could not read
     it; `error` says why. This is not evidence the app saved there: report
     the error rather than changing the task.

   A note with a `file` says why that file's suggestions are thin:
   - `by_position`: only position names what changed; its `elements` lack
     a naming attribute. Name a record that keeps its fields as child
     elements by one of them, as Osmo's contact by
     `[name=contact][>first_name=Delphine]`, a JSON record by a field
     inside it, `[>data/Title=…]` (`~1` for the `/` inside a pointer), or
     an element by an id its child carries, `[>object@id=…]`; count records
     with `member_count`, `matching` `[name=contact]`; otherwise check an
     ancestor's text, or give the element an id in the app and capture again.
   - `view_state_only`: every change is view state, a renumbered id or a
     time the app stamps, and its `pointers` are the checks kept. The edit
     was likely not saved: capture again and save it before `end`.
   - `long_text`: the element's text is too long to check whole; check a
     phrase in it with `text_contains`.
   - `cut_view` or `unreadable_view`: REA's view holds only part of the
     file, or none a check can read, such as a binary format (step 3). For
     a binary file, read it with a reader (see Read a binary file with a
     reader).

## Write the task

Write `{"name", "prompt", "checks"}` from the draft and what you did, one
per task, each with its own name.

- `name`: lowercase letters, digits and hyphens, not `smoke`.
- `prompt`: what the requester wants, in their words and the app's terms,
  starting with the app's name, naming the document as a user would and
  ending with saving it: "In Inkscape, change the poster's headline in
  Documents/poster.svg to Summer Bake Sale and save it." Name the app: the
  solving agent's own shell cannot see the computer's files, and the app's
  name sends it to the screen. Leave saving out for an app that saves on its own, after every change
  or a few seconds after the last one, such as Osmo or Gnote: grading waits
  until the app's files have held for 3 s, so the solving agent need not
  wait for the save. A save more than 3 s after the agent stops is still
  missed; name such an app in the report. Never say how the file stores it,
  and never give the checks.
- Checks are file-based: content checks that prove the edit, never only that
  a file exists or changed. Start from the suggestions; keep those that say
  what the user did and drop what follows from how the app saves.
- Compare exact values, so a typo or a wrong number fails.
- Read each file's `values` before writing checks: every value the change
  added, changed and removed, with `before` and `after` (an uncut scalar, or
  a preview cut at 600 characters) at the pointer a check names it by. Copy
  the pointer. Copy `after` into `value` only when it is an uncut scalar: a
  string without a trailing `…`, a number, or a boolean. Compact XML and a
  JSON array or object are preview strings, so a `value_equals` check needs
  the structured value from the file's view, and element text uses
  `text_equals` or `text_contains`.
  Only XML files, archives, JSON and key files have values: a text file's
  or SQLite database's `change` already lists its lines and rows.
  `refers_to` says what a number or id stands for, such as `category="44"`
  naming `<cat key="44" name="Groceries"/>`, or a new KMyMoney
  transaction's splits naming their account and payee; use it for the
  prompt's words. `alternatives` gives a changed value's pointer by the
  element's other names, such as a label beside an id, to pick another key
  without another export.
  Skip entries whose `note` is `by_position` (no check can use that
  pointer) or noise (`view_state`, `renumbered`, `volatile`,
  `timestamp`, `upkeep`, `layout`). `values_cut` means a limit left some out.
- Select elements by what they hold, never by position: SVG text by id,
  `/root/children/[name=g][@inkscape:label=Layer 1]/children/[name=text][@id=headline]/children/[name=tspan]/children/0`,
  or an ODF paragraph by style,
  `/entries/[name=content.xml]/view/root/children/[name=office:body]/children/[name=office:text]/children/[name=text:p][@text:style-name=Title]/children/0`.
  A selector must match exactly one element, so give the target a unique id
  or style in the seed. The draft's pointers already do this: a name-like
  attribute such as Scribus's `ANNAME` before a coordinate, and two
  attributes together where one is not enough, as a spreadsheet cell's
  `[@Col=3][@Row=2]`. Only an element's text is read by position, as
  `children/0`.
- When an app may split an element's text into runs, check it whole:
  `text_equals` (or `text_contains` for a value inside a sentence) on the
  element compares all its text, however it is split. Add
  `normalize_whitespace: true` when the app may reflow spaces. Example, an
  AbiWord paragraph by its stable id:
  `{"kind": "text_contains", "file": "Documents/memo.abw", "pointer": "/root/children/[name=section]/children/[name=p][@xid=6]", "value": "at 3:30 PM"}`.
  Point a text check at the paragraph or another element the edit keeps,
  never at a run such as AbiWord's `[name=c]`: a reopened document splits
  the text into several runs. The draft lists only what changed; an
  unchanged element is in `draft.guards` when it is a like sibling of a
  changed one, and otherwise use the app's usual numbering, such as another
  paragraph's `xid`, and let the export confirm it.
- In an ODS spreadsheet no attribute names a row: the draft names it by its
  label cell, `[name=table:table-row][>table:table-cell=February]`. Where no
  cell names it alone, name the sheet in the seed, and check its whole
  text: `text_equals` on
  `/entries/[name=content.xml]/view/root/children/[name=office:body]/children/[name=office:spreadsheet]/children/[name=table:table][@table:name=Q1 Sales]`,
  or a stretch of it with `text_contains`. Cell texts join with nothing
  between, so a row reads `February23100`.
- For a line added to free text, such as an item on a note's list, the
  draft compares the whole text with `text_equals`. This rejects longer or
  negated items, but can reject a correct edit with different ordering or
  spacing. Report that limitation when the task permits such variations.
- For a note placed by its own `x` and `y`, such as Xournal++'s `<text>`
  on a layer, the draft checks its text on the layer with `text_contains`,
  which passes wherever the user puts it, and guards each note it kept.
- Some apps keep text in an attribute, one element per run, such as
  Scribus's `<ITEXT CH=...>`: add `"text_attribute": "CH"` to `text_equals`
  or `text_contains` on the frame,
  `/root/children/[name=DOCUMENT]/children/[name=PAGEOBJECT][@ANNAME=Headline]`,
  to read its runs' values joined.
- When the draft shows no values or no checks, a scratch export can expose
  `envs/<env>/seed/` only after the task has at least one valid check: it must
  pass on the captured end state and fail on the seed. Before the first
  successful export, use `act_on_build_computer` to open the saved file in an
  available GUI text editor or viewer and inspect its actual representation
  without changing it. Use the visible saved content and the task's edit to
  write a check; an export refusal can also name the end state's value, such
  as `is "44", not "1"`. If the file cannot be inspected or expressed by a
  supported check, report the limitation instead of guessing a check.
- Where easy, add one check that the rest is untouched, such as another
  text's value the edit must leave alone. `draft.guards` offers them at
  their seed values, which the end holds too: a field on the nearest like
  siblings, one the user sets, such as Planner's `work`, before a default
  they all share; the text an addition kept; a text file's lines beside its
  new one. Copy one or two into `checks`; they pass on the seed, which is
  fine, since the export needs only one check to fail there.
- The draft suggests a decimal the app writes as text, such as `146.83`,
  as a number with `tolerance: 0.001`; keep the tolerance for any number
  you check that way.
- When the draft shows an XML document's changes as `lines`, REA read it
  as text, for example because its DOCTYPE declares entities. Selectors are
  impossible there, and `line_count` with `contains` only approximates a
  value: `3:30` also matches `13:30`. Bound it with neighbouring text and
  name the check as approximate in the report's limitations.
- Some apps save a document reloaded from disk differently from one typed
  in the same session: AbiWord wraps each new insertion in its own markup.
  Your build's end state hides this, which is why whole-text checks matter;
  expect the witness to show the real format.

The kinds and pointer rules are in
[the desktop tasks section](environments.md#pick-the-checks).

### Read a binary file with a reader

When the draft notes `unreadable_view` on a binary document, add a reader to
the task: the app's own headless command, which REA runs in the computer on a
private copy of the file, and whose output a check reads instead of the file.
Write it in the app's scripting mode, print each fact on a line starting
with `REA ` (with the space), and name the copy `{file}`:

- GIMP `.xcf`, layer names, as `lines`:

  ```json
  {
    "id": "layers",
    "file": "Documents/poster.xcf",
    "argv": ["gimp-console-2.10", "-i", "-d", "-f", "-b", "-"],
    "stdin": "(let ((image (car (gimp-file-load RUN-NONINTERACTIVE \"{file}\" \"{file}\")))) (for-each (lambda (layer) (display \"\\nREA layer \") (display (car (gimp-item-get-name layer)))) (vector->list (cadr (gimp-image-get-layers image)))) (newline))\n(gimp-quit 0)\n",
    "output": "lines"
  }
  ```

  checked with `{"kind": "lines_present", "file": "Documents/poster.xcf", "reader": "layers", "lines": ["layer Summer Sale"]}`.
  Script-Fu's prompt `ts> ` starts a line, so print a newline before each
  marked line, as `\nREA` does. A text layer's text is
  `(car (gimp-text-layer-get-text layer))`.

- Blender `.blend`, objects with their type and location, as `json`:

  ```json
  {
    "id": "objects",
    "file": "Documents/scene.blend",
    "argv": [
      "blender",
      "-b",
      "--factory-startup",
      "{file}",
      "--python-expr",
      "import bpy, json; print('REA ' + json.dumps({o.name: {'type': o.type, 'location': [round(v, 4) for v in o.location]} for o in bpy.data.objects}, sort_keys=True))"
    ],
    "output": "json"
  }
  ```

  checked with `{"kind": "value_equals", "file": "Documents/scene.blend", "reader": "objects", "pointer": "/Crate/location/0", "value": 2, "tolerance": 0.001}`.
  Print every value a check compares as a number or string a check can
  name, rounded where the app stores floats.
  Blender 4 shows its Quick Setup splash at every launch until its
  preferences are saved: with `.config/blender` in `state_paths`, confirm
  Quick Setup with Continue, save the preferences (Edit → Preferences, then
  Save Preferences in the menu at its bottom left), and read a probe
  checkpoint's `changed_files` for `.config/blender/4.0/config/userpref.blend`
  before `seed`, so solves start on the scene. Blender often takes the first click on a
  menu or a dialog's button only to focus it: click again.

- A format LibreOffice opens but REA cannot read, such as a legacy `.xls`,
  converted with `--convert-to`:

  ```json
  {
    "id": "cells",
    "file": "Documents/budget.xls",
    "argv": [
      "sh",
      "-c",
      "soffice --headless --convert-to csv --outdir out \"$1\" >/dev/null && sed 's/^/REA /' out/*.csv",
      "sh",
      "{file}"
    ],
    "output": "lines",
    "timeout_s": 120
  }
  ```

  Pass `{file}` to `sh -c` as an argument, as here, rather than inside the
  script.

The reader runs without the display, in a fresh home with the app's default
settings, so use the app's headless or batch mode; set `display: true` only
for an app that has none. It must exit 0 within `timeout_s` (60 s by
default, at most 300) and print at most 1 MiB. Pick facts the change sets and
the near miss gets wrong, such as a name or a number, not the app's whole
output.

Give the task the reader in `readers`, and each check through it `reader`
and the reader's `file`. `json` readers serve `value_equals`, `value_absent`
and `member_count`; `lines` readers `lines_present`, `lines_absent` and
`line_count`. The export runs the reader in the build computer on the seed
and the end: when a check through it fails on the end, the refusal shows
what the reader printed, and a reader that fails shows its error and the end
of its stderr. Fix the reader or the check from that and export again.

### Judge the screen

Only when no file shows the change, as when the draft notes `no_change`
because the app keeps its state on screen, write the task's checks as
screen judges: `{"id": "shown", "kind": "screen_judge", "question": "Does the
calculator's display show 8.568?"}`. A model, the judge, answers the question
on a screenshot three times, and the check passes when most answers are yes.

- REA keeps a screenshot at each checkpoint. Mark `seed` with the screen
  showing the state before the task, where the answer is no, and `end` with
  the change showing, where it is yes: the bundle ships both as the judge's
  examples.
- Ask about what the change makes visible, with exact values, so the near
  miss is a no: a result, a label, a selected item. Avoid what a window
  moved or a cursor blinking can change.
- The prompt asks to leave the result showing, since the judge sees only
  the screen when the agent stops.
- A task with screen judges holds only screen judges and no readers. The
  export refuses one when a file shows the change, and checks none of its
  questions: the witness does, running the bundle's own judge with the
  runner's model endpoint (`LITELLM_BASE_URL` and `LITELLM_API_KEY`, or
  `[model]` in `.agentenv/config.toml`) on a fresh screenshot at each stage.
  Without an endpoint the witness's check fails: say what to set and stop.
- The judge's model is `[model.roles] judge`, else `[model] default`, else
  `anthropic/claude-sonnet-5-5`. `agent-env run --judge-model` does not
  reach it. Each grade costs about $0.02.

## Export

`export_build_environment` with `tasks` and `directory`, the absolute path of
a new or empty folder here, such as `<home>/rea-builds/poster/bundle`. Each
entry of `tasks` is `{"task": ..., "end_checkpoint": ...}`: leave
`end_checkpoint` out for the task checked at `end`, and give
`end-<task name>` for each later one. The
export seeds what was saved at `seed`, replaces the computer's host name,
`user@host` and home path (a bare user name, such as AbiWord's author
`user`, stays), and checks each task against `seed`, its own end and its
`near-<task name>` when you took one. It refuses a task
unless at least one check fails on the seed, every check passes at its
end and one fails at its near miss, and says which task and why, such as a
pointer that names nothing at the end. A
check that the rest is untouched passes on the seed too, which is fine. On
a refusal, fix the task and export again, into another folder if that one
is no longer empty. The bundle's eval lists every task, so a run of the
bundle runs them all. A refusal naming a secret's handle means a file of the
bundle holds that secret, most often an app's sign-in or licence state in
the seed: do not work around it, but report it and stop. Keep
`seed_files`, `scrubbed_files`, `unscrubbed_files` and `limitations` for the
report.

The limitations name the folders the app wrote that the bundle neither owns
nor seeds, such as `~/.config/MuseScore`, so the app starts without them:
expect its first-run wizard, a tour or a "restore session?" question at every
boot. If the witness shows one, capture again on a new build computer with
those folders in `state_paths`, and dismiss the dialogs before `seed`. Lock
files the app holds while it runs, such as LibreOffice's `.lock` and
`.~lock.<name>#`, GnuCash's `<book>.LCK` or Grisbi's `.<file>.lock`, are
left out of the seed and listed after "Left out of the seed". An app can
still start fine yet refuse every save when a lock was seeded, so close the
document before `seed` where the app lets you (apps other than LibreOffice,
such as Rosegarden, also write `.~lock.<name>#` and autosaves beside a
document, and closing it covers them too), and treat "file is locked"
on a save in the witness as a seeded lock: delete it from the bundle's seed
and witness again in a new computer. In LibreOffice, own the profile
(`state_paths: [".config/libreoffice"]`) for a clean boot, and close the
document before `seed`, then reopen it from Recent Documents to make the
change: the export also leaves out the recovery list LibreOffice writes while
a document is open, which would start the bundle into document recovery. A
limitation naming `~/.config/dconf/user` as unread means the app's dconf
settings, such as a first-run tip you turned off, are not seeded: capture
again on a new build computer. The limitations also name seeded WebKit,
Chromium or QtWebEngine storage and caches; when the witness's boot is clean,
list them in the report.

Keep the build computer open until the witness holds: a task fix exports
again from it.

## Witness

Witness each task in its own fresh computer of the bundle, from the prompt,
as a solving agent would; a computer is witnessed once, so the steps below
repeat for each task, each with its own record folder. Do not replay the build's actions or coordinates, and do not
check between stages to steer the edit: a solving agent never sees the
checks. Give every `check_build_task` the task exactly as exported.

1. `start_build_computer` with
   `{"source": {"kind": "bundle", "path": "<bundle>"}}`, adding
   `"settings": {"<variable>": "<handle>"}` for each of the export's
   `withheld_files`, or the deploy is refused for want of them, and agentenv
   keeps that bundle's build failed, so start from a fresh copy of it. The app starts
   with the seed's files in place and opens the seed's document when the
   seed holds exactly one and the app takes a file. When the export's
   `limitations` say it opens no document, open it from the prompt's path,
   as a solving agent would. Take a `wait` screenshot first, since the
   desktop may not have drawn yet. When the start fails because the
   computer never got ready, and its error names lock files in the bundle's
   seed, delete them from the bundle and start again.
2. `check_build_task` with `stage: "seed"` and `directory`, the absolute
   path of a new or empty folder for the record. A check must fail.
   Recording starts. Later stages take no `directory`.
3. Do the task from the prompt with `act_on_build_computer` and save.
   `check_build_task` with `stage: "after_edit"`: every check must pass.
   Each check waits until the app's files have held for 3 s, as the
   bundle's verifier does, so an app that saves on its own needs no wait of
   yours. A result with `settled: false` means the files were still
   changing at the 15 s bound, and `read_cut: true` that a slow read was
   still running there.
4. Make a near miss on top of it: a plausible wrong edit, such as a typo or
   a wrong number, and save. `check_build_task` with `stage: "near_miss"`:
   a check must fail.
5. `act_on_build_computer` with `action: "reset"`, then `check_build_task`
   with `stage: "after_reset"`: a check must fail again.
6. The last stage ends the witness: `witness.record` has the verdict, the
   path of `witness.json` and the recording's segments in `directory`.
   `close_build_computer` on the witness computer.

A stage that does not hold ends the witness as `failed`. Read each check's
result and decide:

- `after_edit` failed though the saved file is right: the check is brittle.
  Fix it, export again and witness in a new computer.
- `after_edit` failed because your edit was wrong: witness again in a new
  computer; a computer is witnessed once.
- `near_miss` passed: the checks are too loose. Tighten them, export again
  and witness again.
- `after_reset` passed: the reset did not restore the seed. Report it as a
  limitation.

Stop after three witnesses of a task and report what still fails.

## Measure

Only if the requester asked for it, or by the headless default above. If
they wanted to review the tasks first, show them each prompt and checks and
wait for their approval. Then `measure_build_task` each task with `bundle`,
`task`, `model` and `budget_usd`, its share of the budget. It runs the task
in fresh computers, 3 times, then 2 more unless they passed none or all, and
keeps a task that passes 1 to 3 of 5 (`band: "in_band"`). It stops before a
solve the largest cost so far would take past the budget, and writes the
record into the bundle's `difficulty.json` and README. The solver needs a
model endpoint where REA starts: `LITELLM_BASE_URL` and `LITELLM_API_KEY`, or
`base_url` and `api_key` under `[model]` in `.agentenv/config.toml`; when the
measure says a solve did not run for want of one, tell the person what to
set and stop. Read the result's `failures_per_check` and each solve's
`failed_checks` before calling a task hard: a check that fails in most failed
solves while the app shows the right change is brittle, so fix the check,
export and witness again, rather than count it as difficulty.

## Harder tasks

When the requester wants tasks worth evaluating or training on, harden a
task that came out `too_easy` and measure it again, within the budget. The
solver sees and fixes its slips on screen, so one edit with the value in the
prompt is solved almost every time; so far, on TreeLine with Sonnet 5.5, so
were one more dialog, a conditional move of seven records, and ten
corrections read from a second document and applied across the list. Try,
one at a time, measuring each:

- A longer chain of exact edits than that, each a value the checks compare
  whole, so one slip in the chain fails the task.
- Facts read in one view and used in another: values the solver must read
  from a second document the app opens, or from another record, rather than
  from the prompt, such as "the same year as The Quiet Glacier's".
- Distractors beside each target: records with near-identical names or
  values, and a guard check on each that it stays as it was.
- Mistakes that do not show once saved: a value the app displays rounded or
  formatted unlike what it stores, or an edit the app keeps only when its
  field loses focus.

Stop when the budget left cannot cover 3 solves of the hardened task, and
report the bands measured.

Each hardened task stays sound: its checks still fail on the seed and pass on
its end, its near miss fails, and another valid way of doing it passes, so
check values, never the order or method the solver used. Put the second
document in the seed. Count records with `member_count` and check each
value by a selector that names its record, since no check joins two records.

## Close and report

`close_build_computer` on every computer still open, also after a failure.
If a close fails, call it again; if it still fails, tell the requester what
is left.

Report:

- the bundle path and the environment name;
- each task's name, prompt and end checkpoint;
- each check, and why it proves the edit and fails the near miss, and each
  reader's command;
- each task's witness stages with their results, the verdict, and the paths of
  `witness.json` and the recording;
- the seed's contents: `seed_files` and what each holds;
- `scrubbed_files` and `unscrubbed_files`; an unscrubbed file still names
  the host or user, so it needs review before sharing. `recompressed_files`
  are gzip seeds whose headers were cleared, their content unchanged: give
  their count;
- limitations: the capture's, the export's and anything you could not do;
- each measured task's band, passes of finished solves, turns, cost and
  failures per check, from `difficulty.json`;
- the build time and the questions you asked.

Never publish or share the bundle yourself.
