# M0 platform checks inside one lab VM. Usage: m0.ps1 STEP [PACKAGES], PACKAGES a comma list such as p2,p3c.
# Prints its report as base64 between markers, which keeps the console code page out of the way.
param([Parameter(Mandatory = $true)][string]$Step, [string]$Pkg = '')
$ErrorActionPreference = 'Stop'
$root = 'C:\pastegrab-lab\m0'
$res = Join-Path $root 'results'
$null = New-Item -ItemType Directory -Force -Path $res
$inv = [Globalization.CultureInfo]::InvariantCulture
$strict = New-Object Text.UTF8Encoding($false, $true)
$plain = New-Object Text.UTF8Encoding($false)
$cp1251 = [Text.Encoding]::GetEncoding(1251)
$script:report = New-Object Collections.Generic.List[string]

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Text;

public class M0Run
{
    public int ExitCode;
    public long ElapsedMs;
    public long PeakWorkingSet;
    public long PeakPaged;
    public bool TimedOut;
    public List<string> Lines = new List<string>();

    public static string Quote(string a)
    {
        if (a.Length > 0 && a.IndexOfAny(new char[] { ' ', '\t', '"' }) < 0) return a;
        StringBuilder sb = new StringBuilder("\"");
        int bs = 0;
        foreach (char c in a)
        {
            if (c == '\\') { bs++; continue; }
            if (c == '"') { sb.Append('\\', bs * 2 + 1); sb.Append('"'); bs = 0; continue; }
            sb.Append('\\', bs); bs = 0; sb.Append(c);
        }
        sb.Append('\\', bs * 2);
        sb.Append('"');
        return sb.ToString();
    }

    // Output stays raw bytes: Latin-1 maps each byte to one char and back.
    public static M0Run Run(string exe, string[] args, string cwd, string[] env, int timeoutMs)
    {
        M0Run r = new M0Run();
        Encoding raw = Encoding.GetEncoding(28591);
        ProcessStartInfo psi = new ProcessStartInfo(exe);
        List<string> quoted = new List<string>();
        foreach (string a in args) quoted.Add(Quote(a));
        psi.Arguments = string.Join(" ", quoted.ToArray());
        psi.WorkingDirectory = cwd;
        psi.UseShellExecute = false;
        psi.CreateNoWindow = true;
        psi.RedirectStandardOutput = true;
        psi.RedirectStandardError = true;
        psi.StandardOutputEncoding = raw;
        psi.StandardErrorEncoding = raw;
        for (int i = 0; i + 1 < env.Length; i += 2) psi.EnvironmentVariables[env[i]] = env[i + 1];
        object gate = new object();
        Stopwatch sw = Stopwatch.StartNew();
        Process p = new Process();
        p.StartInfo = psi;
        p.OutputDataReceived += delegate(object s, DataReceivedEventArgs e) {
            if (e.Data != null) lock (gate) r.Lines.Add(sw.ElapsedMilliseconds + "|o|" + Convert.ToBase64String(raw.GetBytes(e.Data)));
        };
        p.ErrorDataReceived += delegate(object s, DataReceivedEventArgs e) {
            if (e.Data != null) lock (gate) r.Lines.Add(sw.ElapsedMilliseconds + "|e|" + Convert.ToBase64String(raw.GetBytes(e.Data)));
        };
        p.Start();
        p.BeginOutputReadLine();
        p.BeginErrorReadLine();
        while (!p.WaitForExit(50))
        {
            try
            {
                p.Refresh();
                r.PeakWorkingSet = Math.Max(r.PeakWorkingSet, p.PeakWorkingSet64);
                r.PeakPaged = Math.Max(r.PeakPaged, p.PeakPagedMemorySize64);
            }
            catch (Exception) { }
            if (sw.ElapsedMilliseconds > timeoutMs)
            {
                r.TimedOut = true;
                try { p.Kill(); } catch (Exception) { }
                break;
            }
        }
        p.WaitForExit();
        r.ElapsedMs = sw.ElapsedMilliseconds;
        r.ExitCode = r.TimedOut ? -1 : p.ExitCode;
        return r;
    }
}
'@

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

// Drops the file cache, so the next start reads its files from disk the way it does after a reboot.
public static class M0Mem
{
    [StructLayout(LayoutKind.Sequential, Pack = 4)]
    struct TokenPrivilege { public int Count; public long Luid; public int Attributes; }

    [DllImport("ntdll.dll")] static extern int NtSetSystemInformation(int cls, ref int info, int length);
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetSystemFileCacheSize(IntPtr min, IntPtr max, int flags);
    [DllImport("advapi32.dll", SetLastError = true)] static extern bool OpenProcessToken(IntPtr process, int access, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError = true)] static extern bool LookupPrivilegeValue(string system, string name, out long luid);
    [DllImport("advapi32.dll", SetLastError = true)] static extern bool AdjustTokenPrivileges(IntPtr token, bool disableAll, ref TokenPrivilege state, int length, IntPtr previous, IntPtr returned);

    static void Enable(IntPtr token, string name)
    {
        TokenPrivilege tp = new TokenPrivilege();
        tp.Count = 1;
        tp.Attributes = 2;
        LookupPrivilegeValue(null, name, out tp.Luid);
        AdjustTokenPrivileges(token, false, ref tp, 0, IntPtr.Zero, IntPtr.Zero);
    }

    public static string Purge()
    {
        IntPtr token;
        OpenProcessToken(GetCurrentProcess(), 0x28, out token);
        Enable(token, "SeProfileSingleProcessPrivilege");
        Enable(token, "SeIncreaseQuotaPrivilege");
        // Running processes keep their working sets; only cached file pages go.
        bool cache = SetSystemFileCacheSize(new IntPtr(-1), new IntPtr(-1), 0);
        int cmd = 3;
        int flush = NtSetSystemInformation(80, ref cmd, 4);
        cmd = 4;
        int purge = NtSetSystemInformation(80, ref cmd, 4);
        return "cache " + cache + ", flush " + flush + ", purge " + purge;
    }
}
'@

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

// Loads a module the way the Windows loader will, resolving every static import; 127 is a missing function.
public static class M0Load
{
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] static extern IntPtr LoadLibraryExW(string path, IntPtr file, uint flags);
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] static extern IntPtr AddDllDirectory(string dir);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetDefaultDllDirectories(uint flags);
    [DllImport("kernel32.dll")] static extern bool FreeLibrary(IntPtr module);

    public static void Dirs(string[] dirs)
    {
        SetDefaultDllDirectories(0x1000);
        foreach (string d in dirs) AddDllDirectory(d);
    }

    public static int Load(string path)
    {
        IntPtr module = LoadLibraryExW(path, IntPtr.Zero, 0x1000 | 0x100);
        if (module == IntPtr.Zero) return Marshal.GetLastWin32Error();
        FreeLibrary(module);
        return 0;
    }
}
'@

function Say([string]$s) { $script:report.Add($s) }
function F([string]$fmt) { [string]::Format($inv, $fmt, [object[]]$args) }

function Decode([byte[]]$b) {
    try { return $strict.GetString($b) } catch { return '[cp1251] ' + $cp1251.GetString($b) }
}

function Run([string]$name, [string]$exe, [string[]]$a, [string[]]$envs = @(), [int]$timeout = 540000, [string]$cwd = $root) {
    $r = [M0Run]::Run($exe, $a, $cwd, $envs, $timeout)
    $text = New-Object Collections.Generic.List[string]
    $text.Add('cmd: ' + [M0Run]::Quote($exe) + ' ' + (($a | ForEach-Object { [M0Run]::Quote($_) }) -join ' '))
    foreach ($l in $r.Lines) {
        $p = $l.Split([char[]]'|', 3)
        $text.Add((F '{0,7} {1} {2}' $p[0] $p[1] (Decode ([Convert]::FromBase64String($p[2])))))
    }
    [IO.File]::WriteAllLines((Join-Path $res "$name.log"), $text.ToArray(), $plain)
    $to = ''
    if ($r.TimedOut) { $to = ' TIMEOUT' }
    Say (F '{0}: rc={1}, {2} ms, peak working set {3:0.0} MB, peak private {4:0.0} MB{5}' $name $r.ExitCode $r.ElapsedMs ($r.PeakWorkingSet / 1MB) ($r.PeakPaged / 1MB) $to)
    $r | Add-Member -NotePropertyName Text -NotePropertyValue $text -PassThru
}

# Lines are '{ms,7} {o|e} text', so the stream tag sits at index 8.
function Pick($r, [string]$re, [int]$max = 20) {
    $n = 0
    foreach ($t in ($r.Text | Select-Object -Skip 1)) {
        if ($t -match $re) { Say ('    ' + $t); $n++; if ($n -ge $max) { break } }
    }
}
function At($r, [string]$re, [switch]$Last) {
    $v = -1
    foreach ($t in ($r.Text | Select-Object -Skip 1)) {
        if ($t -match $re) { $v = [int]$t.Substring(0, 7).Trim(); if (-not $Last) { break } }
    }
    $v
}

function Pkg([string]$k, [string]$base = $root) {
    switch ($k) {
        'p1' { @{ exe = "$base\p1\yt-dlp.exe"; flags = @(); script = @() } }
        'p1x' { @{ exe = "$base\p1x\yt-dlp.exe"; flags = @(); script = @() } }
        'p2' { @{ exe = "$base\p2\python\python.exe"; flags = @('-I'); script = @("$base\p2\yt-dlp") } }
        'p3' { @{ exe = "$base\p3\python\python.exe"; flags = @('-I'); script = @("$base\p3\yt-dlp") } }
        'p2c' { @{ exe = "$base\p2\python\python.exe"; flags = @('-I'); script = @("$base\p2\ytdlp") } }
        'p3c' { @{ exe = "$base\p3\python\python.exe"; flags = @('-I'); script = @("$base\p3\ytdlp") } }
    }
}
function Y($p, [string[]]$a, [string[]]$extra = @()) {
    [string[]](@($p.flags) + @($extra) + @($p.script) + @('--ignore-config') + @($a))
}
function Js([string]$base = $root, [string]$j = 'qjs') { @('--no-js-runtimes', '--js-runtimes', "quickjs:$base\$j\qjs.exe") }
function Fresh([string]$path) {
    if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Recurse -Force }
    $null = New-Item -ItemType Directory -Force -Path $path
}

$packages = @('p1', 'p2', 'p3')
if ($Pkg) { $packages = $Pkg.Split(',') }
$yt = 'https://www.youtube.com/watch?v=jNQXAC9IVRw'
$ar = 'https://archive.org/details/Election_Ads/Commercial-Nixon1960ElectionAdToughonDefense.mpg'

try {
    switch ($Step) {
        'setup' {
            $cd = (Get-WmiObject Win32_LogicalDisk -Filter "VolumeName='PGM0'").DeviceID
            if (-not $cd) { throw 'the PGM0 disc is not inserted' }
            Add-Type -AssemblyName System.IO.Compression.FileSystem
            foreach ($z in 'p1', 'p2', 'p3') {
                $dst = Join-Path $root $z
                if (Test-Path $dst) { Remove-Item -Recurse -Force $dst }
                [IO.Compression.ZipFile]::ExtractToDirectory("$cd\$z.zip", $dst)
            }
            foreach ($d in 'p1x', 'qjs', 'qjs-ng', 'ffmpeg') {
                $dst = Join-Path $root $d
                if (Test-Path $dst) { Remove-Item -Recurse -Force $dst }
                Copy-Item -Recurse "$cd\$d" $dst
            }
            Copy-Item -Force "$cd\m0.ps1" (Join-Path $root 'm0.ps1')
            & attrib.exe -R "$root\*" /S /D | Out-Null
            $os = Get-WmiObject Win32_OperatingSystem
            $cpu = Get-WmiObject Win32_Processor | Select-Object -First 1
            $nls = Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Nls\CodePage'
            $ucrt = 'C:\Windows\System32\ucrtbase.dll'
            $uv = 'absent'
            if (Test-Path $ucrt) { $uv = (Get-Item $ucrt).VersionInfo.FileVersion }
            Say "os: $($os.Caption) $($os.Version) build $($os.BuildNumber), $($os.OSArchitecture), $($os.OSLanguage)"
            Say "cpu: $($cpu.Name.Trim()), $($cpu.NumberOfCores) cores, $($cpu.NumberOfLogicalProcessors) logical"
            Say (F 'ram: {0:0} MB visible' ($os.TotalVisibleMemorySize / 1KB))
            Say "code pages: ACP $($nls.ACP), OEMCP $($nls.OEMCP); ucrtbase $uv"
            Say "powershell $($PSVersionTable.PSVersion), clr $($PSVersionTable.CLRVersion)"
            Say ('copied: ' + ((Get-ChildItem $root | ForEach-Object { $_.Name }) -join ', '))
        }
        'smoke' {
            foreach ($k in $packages) {
                $p = Pkg $k
                $r = Run "smoke-$k-version" $p.exe (Y $p @('--version'))
                Pick $r '.' 3
                $r = Run "smoke-$k-verbose" $p.exe (Y $p ((Js) + @('-v', 'x')))
                Pick $r '\[debug\] (Encodings|Python|exe versions|Optional libraries|JS runtimes|Request Handlers)|ERROR|Error|Traceback|DLL' 14
                $r = Run "smoke-$k-impersonate" $p.exe (Y $p @('--list-impersonate-targets'))
                Pick $r 'Chrome|Safari|Edge|Firefox|ERROR|Error|DLL' 8
            }
            if (-not $Pkg) {
                $p = Pkg 'p1x'
                $r = Run 'smoke-p1x-version' $p.exe (Y $p @('--version'))
                Pick $r '.' 3
                foreach ($j in 'qjs', 'qjs-ng') {
                    $r = Run "smoke-$j" "$root\$j\qjs.exe" @('-e', 'print("qjs ok", 6 * 7, typeof BigInt)')
                    Pick $r '.' 3
                }
                $r = Run 'smoke-ffmpeg' "$root\ffmpeg\ffmpeg.exe" @('-hide_banner', '-version')
                Pick $r 'ffmpeg version' 1
                $r = Run 'smoke-ffprobe' "$root\ffmpeg\ffprobe.exe" @('-hide_banner', '-version')
                Pick $r 'ffprobe version' 1
            }
        }
        'youtube' {
            foreach ($k in $packages) {
                $p = Pkg $k
                foreach ($j in 'qjs', 'qjs-ng') {
                    $cache = "$root\cache\$k-$j"
                    Fresh $cache
                    foreach ($pass in 'cold', 'warm') {
                        $r = Run "yt-$k-$j-$pass" $p.exe (Y $p ((Js $root $j) + @('-v', '--cache-dir', $cache, '--extractor-args', 'youtube:jsc_trace=true', '--simulate', '--print', '%(title)s | %(formats.:.format_id)s', $yt)))
                        $a = At $r 'Running QuickJS'
                        $b = At $r 'Solved all' -Last
                        $solve = -1
                        if ($a -ge 0 -and $b -ge 0) { $solve = $b - $a }
                        Say (F '    js challenge {0} ms' $solve)
                        Pick $r '^.{8}o |WARNING|ERROR|Traceback' 6
                    }
                }
                $r = Run "yt-$k-impersonate" $p.exe (Y $p ((Js) + @('-v', '--impersonate', 'chrome', '--cache-dir', "$root\cache\$k-qjs", '--simulate', '--print', '%(title)s', $yt)))
                Pick $r '^.{8}o |impersonat|WARNING|ERROR|Traceback' 6
                $o = "$root\out\$k"
                Fresh $o
                $r = Run "yt-$k-merge" $p.exe (Y $p ((Js) + @('--cache-dir', "$root\cache\$k-qjs", '--ffmpeg-location', "$root\ffmpeg", '-f', '242+251', '--merge-output-format', 'mkv', '--newline', '-P', $o, '-o', '%(id)s.%(ext)s', $yt)))
                Pick $r 'Merging|ERROR|WARNING|Deleting' 4
                $r = Run "ar-$k" $p.exe (Y $p @('-f', '2', '--newline', '-P', $o, '-o', 'nixon.%(ext)s', $ar))
                Pick $r 'Destination|ERROR|WARNING' 3
                foreach ($f in Get-ChildItem -LiteralPath $o) { Say (F '    file: {0} ({1} bytes)' $f.Name $f.Length) }
            }
        }
        'unicode' {
            foreach ($k in $packages) {
                $p = Pkg $k
                $uni = @('--cache-dir', "$root\cache\$k-qjs", '--replace-in-metadata', 'title', '^', 'Тест 🏃 ')
                $r = Run "uni-$k-default" $p.exe (Y $p ((Js) + $uni + @('--simulate', '--print', '%(title)s', '--print', '%(title)j', $yt)))
                Pick $r '^.{8}o |ERROR' 4
                $r = Run "uni-$k-encoding" $p.exe (Y $p ((Js) + $uni + @('--encoding', 'utf-8', '--simulate', '--print', '%(title)s', $yt)))
                Pick $r '^.{8}o |ERROR' 4
                if ($k -ne 'p1') {
                    $r = Run "uni-$k-utf8mode" $p.exe (Y $p ((Js) + $uni + @('--simulate', '--print', '%(title)s', $yt)) @('-X', 'utf8'))
                    Pick $r '^.{8}o |ERROR' 4
                }
                $o = "$root\out\uni-$k"
                Fresh $o
                $r = Run "uni-$k-file" $p.exe (Y $p ((Js) + $uni + @('-f', '18', '-P', $o, '-o', '%(title)s [%(id)s].%(ext)s', $yt)))
                Pick $r 'Destination|ERROR|WARNING' 3
                foreach ($f in Get-ChildItem -LiteralPath $o) { Say (F '    file: {0} ({1} bytes)' $f.Name $f.Length) }
            }
        }
        'ffmpeg' {
            $ff = "$root\ffmpeg\ffmpeg.exe"
            $fp = "$root\ffmpeg\ffprobe.exe"
            $o = "$root\out\ff"
            Fresh $o
            $mkv = @(Get-ChildItem "$root\out\p*\jNQXAC9IVRw.mkv")[0].FullName
            $mpg = @(Get-ChildItem "$root\out\p*\nixon.mpg")[0].FullName
            Say "inputs: $mkv, $mpg"
            $r = Run 'ff-mf' $ff @('-hide_banner', '-f', 'lavfi', '-i', 'testsrc2=duration=5:size=1280x720:rate=30', '-c:v', 'h264_mf', '-b:v', '2M', '-y', "$o\mf.mp4")
            Pick $r 'rror|ailed|not ' 5
            $r = Run 'ff-vp9' $ff @('-hide_banner', '-i', $mkv, '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '23', '-c:a', 'aac', '-b:a', '128k', '-movflags', '+faststart', '-y', "$o\vp9-to-h264.mp4")
            Pick $r 'rror|ailed' 5
            $r = Run 'ff-mpeg1' $ff @('-hide_banner', '-i', $mpg, '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '23', '-c:a', 'aac', '-b:a', '128k', '-movflags', '+faststart', '-y', "$o\mpeg1-to-h264.mp4")
            Pick $r 'rror|ailed' 5
            $r = Run 'ff-bwdif' $ff @('-hide_banner', '-i', $mpg, '-vf', 'bwdif=mode=send_field', '-c:v', 'libx264', '-preset', 'veryfast', '-an', '-t', '10', '-y', "$o\bwdif.mp4")
            Pick $r 'rror|ailed' 5
            foreach ($f in @($mkv, $mpg) + @(Get-ChildItem "$o\*.mp4" | ForEach-Object { $_.FullName })) {
                $r = Run ('probe-' + [IO.Path]::GetFileName($f)) $fp @('-v', 'error', '-show_entries', 'stream=codec_name,width,height,r_frame_rate,field_order:format=duration,size', '-of', 'compact=p=0', $f)
                Pick $r '^.{8}o |rror' 4
            }
        }
        'cyrpath' {
            $cy = Join-Path $root 'Иван Петров'
            $tmp = Join-Path $cy 'Temp'
            Fresh $cy
            $null = New-Item -ItemType Directory -Force -Path $tmp
            foreach ($d in @($packages) + @('qjs')) { Copy-Item -Recurse (Join-Path $root $d) (Join-Path $cy $d) }
            foreach ($k in $packages) {
                $p = Pkg $k $cy
                $e = @('TEMP', $tmp, 'TMP', $tmp)
                $r = Run "cy-$k-version" $p.exe (Y $p @('--version')) $e
                Pick $r '.' 3
                $r = Run "cy-$k-yt" $p.exe (Y $p ((Js $cy) + @('-v', '--cache-dir', "$cy\cache-$k", '--simulate', '--print', '%(title)s | %(formats.:.format_id)s', $yt))) $e
                Pick $r '^.{8}o |Running QuickJS|Solved all|WARNING|ERROR|Traceback' 6
            }
        }
        'precompile' {
            foreach ($k in 'p2', 'p3') {
                $p = Pkg $k
                $dir = "$root\$k\ytdlp"
                Fresh $dir
                $r = Run "pc-$k-unzip" $p.exe @('-I', '-m', 'zipfile', '-e', "$root\$k\yt-dlp", $dir)
                Pick $r 'rror' 3
                $r = Run "pc-$k-compile" $p.exe @('-I', '-m', 'compileall', '-q', '-j', '0', '--invalidation-mode', 'unchecked-hash', $dir)
                Pick $r 'rror' 3
                $q = Pkg "${k}c"
                foreach ($i in 1..3) { $r = Run "pc-$k-version$i" $q.exe (Y $q @('--version')) }
                $r = Run "pc-$k-yt" $q.exe (Y $q ((Js) + @('--cache-dir', "$root\cache\$k-qjs", '--simulate', '--print', '%(title)s', $yt)))
                Pick $r '^.{8}o |ERROR|Traceback' 3
            }
        }
        'cold' {
            $list = @('p1x') + @($packages)
            if ($Pkg) { $list = $packages }
            foreach ($k in $list) {
                $p = Pkg $k
                foreach ($i in 1..3) {
                    Say ('purge: ' + [M0Mem]::Purge())
                    Start-Sleep -Seconds 20
                    $r = Run "start-$k-cold$i" $p.exe (Y $p @('--version'))
                    $r = Run "start-$k-warm$i" $p.exe (Y $p @('--version'))
                }
            }
        }
        'loadall' {
            # One package per call: modules that share a name must not meet in one process.
            $k = $packages[0]
            $base = Join-Path $root $k
            $mods = @(Get-ChildItem -Path $base -Recurse -Include *.dll, *.pyd)
            [M0Load]::Dirs([string[]]@($mods | ForEach-Object { $_.DirectoryName } | Sort-Object -Unique))
            $bad = 0
            foreach ($m in $mods) {
                $code = [M0Load]::Load($m.FullName)
                if ($code -ne 0) { $bad++; Say (F '    {0}: error {1}' $m.FullName.Substring($base.Length + 1) $code) }
            }
            Say (F '{0}: {1} modules, {2} failed to load' $k $mods.Count $bad)
        }
        'jsc' {
            foreach ($k in $packages) {
                $p = Pkg $k
                $cache = "$root\cache\jsc-$k"
                Fresh $cache
                $r = Run "jsc-$k" $p.exe (Y $p ((Js) + @('-v', '--cache-dir', $cache, '--simulate', '--print', '%(title)s', $yt)))
                $a = At $r 'Running QuickJS'
                $b = At $r 'Solved all' -Last
                Say (F '    js challenge {0} ms' ($b - $a))
                Pick $r 'ERROR|Traceback' 3
            }
        }
        default { throw "unknown step $Step" }
    }
}
catch {
    Say ('EXCEPTION: ' + $_.Exception.Message + ' at line ' + $_.InvocationInfo.ScriptLineNumber)
}

$file = Join-Path $res "$Step$Pkg.txt"
[IO.File]::WriteAllLines($file, $script:report.ToArray(), $plain)
'M0BEGIN'
[Convert]::ToBase64String([IO.File]::ReadAllBytes($file), 'InsertLineBreaks')
'M0END'
