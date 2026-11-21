# lab

| VM | Windows | Files |
|---|---|---|
| `pastegrab-w81` | 8.1 Pro VL x64, Russian | `win81-x64/` |
| `pastegrab-w80` | 8 (core) x64, Russian | `win80-x64/` |

Both run on `qemu:///system`, unactivated, with automatic logon as `tester` / `pastegrab`. ISOs live in `/mnt/storage/ISO`, disks in `/mnt/storage/VM Disks`. `common/firstlogon.ps1` runs at first logon and installs the virtio drivers, the QEMU guest agent and no-sleep power settings.

**Networking is passt in user mode.** The guest goes out through the host's own route, and the host needs no bridge, firewall rule or root for it.

Rebuild an unattend ISO from `lab/` after editing its `autounattend.xml` or `common/firstlogon.ps1`:

```sh
v=w81; xorriso -as mkisofs -quiet -J -R -V PGUNATTEND -graft-points \
  -o /mnt/storage/ISO/pastegrab-$v-unattend.iso \
  autounattend.xml=win${v#w}-x64/autounattend.xml firstlogon.ps1=common/firstlogon.ps1
```

These start a VM, open its screen and ping its guest agent.

```sh
virsh -c qemu:///system start pastegrab-w81
virt-viewer -c qemu:///system pastegrab-w81
virsh -c qemu:///system qemu-agent-command pastegrab-w81 '{"execute":"guest-ping"}'
```

Snapshots are taken and restored with the VM shut off.

```sh
virsh -c qemu:///system snapshot-create-as pastegrab-w81 clean
virsh -c qemu:///system snapshot-revert pastegrab-w81 clean
```
