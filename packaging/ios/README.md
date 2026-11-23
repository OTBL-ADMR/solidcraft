# SolidCraft on iPad

`apps/solidcraft-ios` runs the same app as the desktop build: the Rust kernel and UI, natively on
the iPad, drawing with Metal. It does not run inside a web page, so it isn't held to Safari's
memory limit for one page.

This is a development port in a fork, not an official SolidCraft release.

## Getting the app onto the iPad

### A: from GitHub, no Mac needed

1. Push to the `ipad-port` branch, or start the **iOS** workflow from the Actions tab.
2. When it finishes, download the `SolidCraft-ipad-unsigned` artifact and unzip it to get
   `SolidCraft.ipa`.
3. Install it with [SideStore](https://sidestore.io), [AltStore](https://altstore.io) or
   [Sideloadly](https://sideloadly.io). Each one signs the app with your Apple ID as it installs.
   - With a free Apple ID the app stops opening after 7 days. Reinstall or refresh it then; your
     designs in On My iPad › SolidCraft are kept. A paid developer account lasts a year.
   - The first time, turn on Developer Mode on the iPad (Settings › Privacy & Security) and trust
     your Apple ID under Settings › General › VPN & Device Management.

### B: build on a Mac

```sh
xcode-select --install            # or install Xcode from the App Store
rustup target add aarch64-apple-ios
packaging/ios/build-ipa.sh        # → target/ios/SolidCraft.ipa
```

On an 8 GB Mac use `JOBS=4 NO_LTO=1 packaging/ios/build-ipa.sh`. The first build takes a while;
later builds only recompile what changed. Install the .ipa as in A, or sign it here with a paid
account (`SIGN_IDENTITY` and `PROFILE`, see the top of `build-ipa.sh`), then install it with
`xcrun devicectl device install app --device <id> target/ios/Payload/SolidCraft.app`.

## Files

Files you open and save live in the app's own folder, which the Files app shows as
**On My iPad › SolidCraft**. To open a STEP, STL or other file kept elsewhere (iCloud Drive, a USB
stick, AirDrop), copy it into that folder in Files, then use Open in SolidCraft.

## What works, what doesn't yet

Expected to work: modelling, sketching, the 3D view (Metal), touch navigation (one finger orbits,
two pan and pinch; on at first start, Preferences › Navigation), Apple Pencil, opening and saving
designs, importing and exporting in the folder above.

A mouse or trackpad is a mouse: left, right and middle buttons (so the navigation preset's
mouse mapping applies), the pointer over the view (hover highlights), the wheel, and two-finger
trackpad scrolling, with Shift, Control, Option and Command held. How far one wheel notch
scrolls hasn't been tuned on a device yet.

Korean, Japanese and Chinese text draw with the iPad's own fonts (Apple SD Gothic Neo for
Hangul), in the interface and in sketch text. Choose 한국어 under Preferences › Language.

Not done yet:

- **The on-screen keyboard** should appear when a text or number box is tapped (winit shows it
  when egui asks for text input), but it hasn't been tried on a device. Its suggestions and
  autocorrect bar aren't connected.
- **Korean input by the on-screen keyboard**: the view takes typed text one character at a time
  and has no composing (marked) text, so Hangul syllables may arrive as separate letters. Typing
  Korean on a hardware keyboard may behave the same; this needs checking on a device.
- **Keyboard shortcuts and arrow keys** on a hardware keyboard: only typed characters and
  Backspace reach the app.
- **Copy and paste** with other apps. The system clipboard isn't connected.
- **Files outside the app's folder.** There's no iOS document picker yet, and no "Open in
  SolidCraft" from other apps.
- **Split View and Slide Over.** The app runs full screen.
- **Desktop-only features:** the update check, the 3D mouse, slicer hand-off, the agent chat and
  the MCP control port.

## Reporting problems

To see where a slow operation spends its time, turn on Preferences › Graphics › **Profile
operations** and run the operation. The profiler's report is written to
**On My iPad › SolidCraft › Profile**: `report.json` (time per category and per step, the
same reply `diagnostics.profile` gives on the desktop) and `trace.json` (a Chrome trace, for
Perfetto or chrome://tracing). It is updated every couple of seconds while profiling is on.


When the app quits on start, connect the iPad to a Mac and open Console.app; filter by
`solidcraft-ios` to see why. Crash reports are under Settings › Privacy & Security › Analytics
Data, as `solidcraft-ios-…`.
