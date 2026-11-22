# Release

**Every file a release ships goes through Defender in both lab VMs and through VirusTotal, and each false positive gets reported before he receives the release.** An unknown, unsigned exe is what antivirus heuristics distrust most, and pastegrab ships several.

## Signing key

**`keys/release.pub` is the minisign key every app update must be signed with, and the app carries it.** The secret key and its password stay in your password manager, never in the repository. The app refuses updates signed by any other key. Losing the secret key ends updates until he reinstalls from a new setup.

## Publishing

**A release is three steps: `just package <tools>`, `just sign <version>`, `just publish <version>`.** The version is the one in `apps/window/Cargo.toml`, and it must grow with every release, because installs only take a newer one.

1. `just package` builds the setup, the update package `pastegrab-app-<version>-x86_64.tar.zst` and its `update.json`, which names the package's URL, SHA-256 and size and expires a year later, in `target/dist/<version>/`.
2. `just sign` signs `update.json` with the release key and asks for its password.
3. `just publish` creates the GitHub release with all four files. Every install checks `releases/latest/download/update.json` once a day, so a published release reaches him within a day of his next start.

An update replaces only `app\<version>\`. The launcher and the tools change only through a new setup.

A release published with `just publish <version> --prerelease` stays out of `latest`. The lab points one install at it by starting the window with `PASTEGRAB_RELEASES` set to `https://github.com/PatAmigo0/pastegrab/releases/download/v<version>/`; the signature check applies all the same.

## Antivirus

1. `lab/defender-scan pastegrab-w81 FILE...`, then the same with `pastegrab-w80`. The script updates Defender first and prints its engine and definition versions with the verdict.
2. `VT_API_KEY=... tools/release/virustotal.sh FILE...` uploads what VirusTotal hasn't seen and lists every engine that flags a file. Uploads reach the antivirus vendors, which is the point for an open-source release.
3. A false positive from Defender goes to Microsoft's form at https://www.microsoft.com/en-us/wdsi/filesubmission as a developer reporting an incorrect detection. Other vendors each run a form of their own.

Every exe we build carries version info and a manifest, and none is packed with UPX or PyInstaller, the two things behind most false alarms on downloaders.

## SmartScreen

**The setup is unsigned, so SmartScreen stops its first start, and he passes it with «Подробнее» and «Выполнить в любом случае».** The browser marks the downloaded setup as coming from the internet, and SmartScreen judges marked files by reputation. Updates come through pastegrab's own downloads without that mark, and SmartScreen never sees them.

A signature removes the window once the certificate has built reputation. SignPath Foundation signs open-source projects for free, and Certum sells a certificate for open-source developers; both check the developer's identity first.
