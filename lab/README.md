# lab

| VM | Windows | Files |
|---|---|---|
| `pastegrab-w81` | 8.1 Pro VL x64, Russian | `win81-x64/` |
| `pastegrab-w80` | 8 (core) x64, Russian | `win80-x64/` |

Both run on `qemu:///system`, unactivated, with automatic logon as `tester` / `pastegrab`. ISOs live in `/mnt/storage/ISO`, disks in `/mnt/storage/VM Disks`. `common/firstlogon.ps1` runs at first logon and installs the virtio drivers, the QEMU guest agent and no-sleep power settings.

**Networking is passt in user mode.** The guest goes out through the host's own route, and the host needs no bridge, firewall rule or root for it.

Rebuild an unattend ISO from `lab/` after editing its `autounattend.xml` or `common/firstlogon.ps1`. A VM that ran leaves its ISOs owned by libvirt, and the first line deletes the old file before the rebuild.

```sh
v=w81; rm -f /mnt/storage/ISO/pastegrab-$v-unattend.iso
xorriso -as mkisofs -quiet -J -R -V PGUNATTEND -graft-points \
  -o /mnt/storage/ISO/pastegrab-$v-unattend.iso \
  autounattend.xml=win${v#w}-x64/autounattend.xml firstlogon.ps1=common/firstlogon.ps1
```

The XML files carry no UUID. The first `virsh define` assigns one, and a later redefine has to reuse it, or libvirt refuses the domain as a duplicate.

```sh
v=w81; d=pastegrab-$v; uuid=$(virsh -c qemu:///system domuuid $d)
sed "s#<name>$d</name>#<name>$d</name>\n  <uuid>$uuid</uuid>#" win${v#w}-x64/domain.xml > /tmp/$d.xml
virsh -c qemu:///system define /tmp/$d.xml
```

These start a VM, open its screen and ping its guest agent.

```sh
virsh -c qemu:///system start pastegrab-w81
virt-viewer -c qemu:///system pastegrab-w81
virsh -c qemu:///system qemu-agent-command pastegrab-w81 '{"execute":"guest-ping"}'
```

`guest-run` runs a PowerShell script in a running VM through the guest agent and prints its output. The script travels base64-encoded, which keeps the agent's quoting away from it.

```sh
./guest-run pastegrab-w81 'Get-Item C:\Windows\System32\ucrtbase.dll | Select-Object -Expand VersionInfo'
```

PowerShell starts with the execution policy bypassed, which lets a script call `.ps1` files already in the VM. A script that needs more than 600 seconds sets `GUEST_RUN_TIMEOUT`. `guest-put` copies a file into a running VM in base64 chunks through the same agent.

```sh
./guest-put pastegrab-w81 m0.ps1 'C:\pastegrab-lab\m0\m0.ps1'
```

The agent runs in session 0, where windows never show. `guest-gui` starts a program on the logged-on user's desktop through an interactive scheduled task, `screenshot` saves the screen as a PNG, `click` presses the mouse at a pixel, and `virsh send-key` presses keys. A window started this way doesn't get the foreground, so a click comes before keys. `defender-scan` copies files in, updates Defender and scans them.

```sh
./guest-gui pastegrab-w81 'C:\pg\pastegrab-setup.exe'
./screenshot pastegrab-w81 /tmp/setup.png
./click pastegrab-w81 713 215
virsh -c qemu:///system send-key pastegrab-w81 KEY_ENTER
./defender-scan pastegrab-w81 ../target/win7/x86_64-win7-windows-gnu/release/pgcli.exe
```

The unattended install skipped the first-run screen that turns SmartScreen on, and the VMs show Windows' older publisher warning for downloaded files instead.

Snapshots need the VM shut off. `clean` is Windows right after setup, and `ucrt` adds the UCRT that his PC has. The install ISOs were deleted after setup, with the install drive emptied in every snapshot, which means a rebuilt VM first needs its ISO back at the old path.

```sh
virsh -c qemu:///system snapshot-create-as pastegrab-w81 clean
virsh -c qemu:///system snapshot-revert pastegrab-w81 clean
```

## M0 harness

**`m0/` reruns the platform checks behind `docs/platform.md`, and a new Python, yt-dlp, QuickJS or ffmpeg version goes through it before it ships.** Downloads, builds and reports live in `$M0_DIR`, `/mnt/storage/pastegrab-m0` by default.

| Script | Does |
|---|---|
| `m0/fetch.sh` | downloads each candidate into its own folder and checks its hash |
| `m0/stage.sh` | assembles the three yt-dlp packages and burns them with `m0.ps1` to the PGM0 disc |
| `m0/run-all.sh VM [--cold]` | reverts the VM to `ucrt`, inserts the disc, runs every step, writes one report per step |
| `m0/m0-run VM STEP` | runs one step, such as `smoke`, `youtube p2` or `loadall p3` |
| `m0/host-control.sh` | the same yt-dlp release from this machine, to tell a Windows 8 failure from a network one |

QuickJS comes first, from `tools/build/quickjs/build.sh "$M0_DIR/qjs"`. `--cold` caps the VM's disk at 80 operations and 80 MB a second, about a 5400 rpm laptop drive, and empties the file cache before each start.

`probe/` is a Rust program that checks the Windows APIs pastegrab relies on, built with `tools/build/rust/build.sh lab/probe x86_64-win7-windows-gnu`. `run-all.sh` copies it into the VM and runs it when the build exists.
