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
two pan and pinch), Apple Pencil as a pointer, a Magic Keyboard trackpad and keys, opening and
saving designs, importing and exporting in the folder above.

Not done yet:

- **The on-screen keyboard** should appear when a text or number box is tapped (winit shows it
  when egui asks for text input), but it hasn't been tried on a device. Its suggestions and
  autocorrect bar aren't connected.
- **Copy and paste** with other apps. The system clipboard isn't connected.
- **Files outside the app's folder.** There's no iOS document picker yet, and no "Open in
  SolidCraft" from other apps.
- **Split View and Slide Over.** The app runs full screen.
- **Desktop-only features:** the update check, the 3D mouse, slicer hand-off, the agent chat and
  the MCP control port.

## Reporting problems

When the app quits on start, connect the iPad to a Mac and open Console.app; filter by
`solidcraft-ios` to see why. Crash reports are under Settings › Privacy & Security › Analytics
Data, as `solidcraft-ios-…`.
