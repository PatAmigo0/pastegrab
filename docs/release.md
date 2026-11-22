# Release

**Every file a release ships goes through Defender in both lab VMs and through VirusTotal, and each false positive gets reported before he receives the release.** An unknown, unsigned exe is what antivirus heuristics distrust most, and pastegrab ships several.

## Antivirus

1. `lab/defender-scan pastegrab-w81 FILE...`, then the same with `pastegrab-w80`. The script updates Defender first and prints its engine and definition versions with the verdict.
2. `VT_API_KEY=... tools/release/virustotal.sh FILE...` uploads what VirusTotal hasn't seen and lists every engine that flags a file. Uploads reach the antivirus vendors, which is the point for an open-source release.
3. A false positive from Defender goes to Microsoft's form at https://www.microsoft.com/en-us/wdsi/filesubmission as a developer reporting an incorrect detection. Other vendors each run a form of their own.

Every exe we build carries version info and a manifest, and none is packed with UPX or PyInstaller, the two things behind most false alarms on downloaders.

## SmartScreen

**The setup is unsigned, so SmartScreen stops its first start, and he passes it with «Подробнее» and «Выполнить в любом случае».** The browser marks the downloaded setup as coming from the internet, and SmartScreen judges marked files by reputation. Updates come through pastegrab's own downloads without that mark, and SmartScreen never sees them.

A signature removes the window once the certificate has built reputation. SignPath Foundation signs open-source projects for free, and Certum sells a certificate for open-source developers; both check the developer's identity first.
