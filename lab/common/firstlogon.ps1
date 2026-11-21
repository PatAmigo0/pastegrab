# First logon in a pastegrab lab VM: virtio drivers, the QEMU guest agent, no sleep. Log in C:\pastegrab-lab.
$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force -Path 'C:\pastegrab-lab' | Out-Null
Start-Transcript -Path 'C:\pastegrab-lab\firstlogon.log' -Append | Out-Null

# The registry reports 6.3 on 8.1 regardless of manifests; Environment.OSVersion can claim 6.2.
$ver = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').CurrentVersion
$os = if ($ver -eq '6.3') { 'w8.1' } else { 'w8' }
$arch = if ([Environment]::Is64BitOperatingSystem) { 'amd64' } else { 'x86' }
$msiName = if ($arch -eq 'amd64') { 'qemu-ga-x86_64.msi' } else { 'qemu-ga-i386.msi' }
Write-Output "Windows $ver -> virtio folders $os\$arch"

$virtio = Get-PSDrive -PSProvider FileSystem |
    Where-Object { Test-Path (Join-Path $_.Root "guest-agent\$msiName") } |
    Select-Object -First 1

if ($virtio) {
    $root = $virtio.Root
    foreach ($driver in 'vioserial', 'Balloon', 'viorng', 'NetKVM', 'vioscsi') {
        Get-ChildItem -Path (Join-Path $root "$driver\$os\$arch") -Filter '*.inf' -ErrorAction SilentlyContinue |
            ForEach-Object { pnputil -i -a $_.FullName }
    }
    $msi = Join-Path $root "guest-agent\$msiName"
    Start-Process -FilePath 'msiexec.exe' -ArgumentList '/i', "`"$msi`"", '/qn', '/norestart' -Wait
} else {
    Write-Output 'virtio-win CD not found'
}

powercfg -h off
powercfg -change -monitor-timeout-ac 0
powercfg -change -standby-timeout-ac 0
powercfg -change -disk-timeout-ac 0

Set-Content -Path 'C:\pastegrab-lab\ready.txt' -Value (Get-Date -Format o)
Stop-Transcript | Out-Null
