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

Snapshots are taken and restored with the VM shut off. `clean` is Windows right after setup; `ucrt` adds the UCRT, which his PC has. The install ISOs were deleted after setup and every snapshot's install drive is empty, so rebuilding a VM starts with putting its ISO back.

```sh
virsh -c qemu:///system snapshot-create-as pastegrab-w81 clean
virsh -c qemu:///system snapshot-revert pastegrab-w81 clean
```
