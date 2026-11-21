#!/usr/bin/env bash
# run-all.sh VM [--cold]: reverts VM to its ucrt snapshot, inserts the PGM0 disc and runs every
# m0.ps1 step, one report per step in $M0_DIR/results/VM. --cold adds start times on an HDD-speed disk.
# needs: virsh
set -uo pipefail
vm=$1
cold=${2:-}
here=$(cd "$(dirname "$0")" && pwd)
lab=$(cd "$here/.." && pwd)
base=${M0_DIR:-/mnt/storage/pastegrab-m0}
iso=${M0_ISO:-/mnt/storage/ISO/pastegrab-m0-tools.iso}
probe=$lab/probe/target/x86_64-win7-windows-gnu/release/pastegrab-probe.exe
out=$base/results/$vm
mkdir -p "$out"
v() { virsh -c qemu:///system "$@"; }

[ "$(v domstate "$vm")" = "shut off" ] || { echo "$vm is running; shut it down first"; exit 1; }
v snapshot-revert "$vm" ucrt
v start "$vm" >/dev/null
for _ in $(seq 1 120); do
    v qemu-agent-command "$vm" '{"execute":"guest-ping"}' >/dev/null 2>&1 && break
    sleep 5
done
v change-media "$vm" sdb "$iso" --insert --live >/dev/null
sleep 5

run() { "$here/m0-run" "$vm" "$@"; }
run setup > "$out/setup.txt" 2>&1
run smoke > "$out/smoke.txt" 2>&1
for p in p1 p2 p3; do
    run youtube "$p" > "$out/youtube-$p.txt" 2>&1
    run loadall "$p" > "$out/loadall-$p.txt" 2>&1
done
for step in unicode ffmpeg cyrpath precompile; do run "$step" > "$out/$step.txt" 2>&1; done
if [ -f "$probe" ]; then
    "$lab/guest-put" "$vm" "$probe" 'C:\pastegrab-lab\probe.exe' >/dev/null
    "$lab/guest-run" "$vm" '& C:\pastegrab-lab\probe.exe' > "$out/probe.txt" 2>&1
fi
if [ "$cold" = --cold ]; then
    v blkdeviotune "$vm" sda --total-iops-sec 80 --total-bytes-sec 83886080 --live
    run cold p1x,p1,p2,p3,p2c,p3c > "$out/cold.txt" 2>&1
    v blkdeviotune "$vm" sda --total-iops-sec 0 --total-bytes-sec 0 --live
fi
echo "$vm: reports in $out"
