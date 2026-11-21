//! Checks the Windows APIs pastegrab relies on, one line per check; the exit code counts failures.
//! `--show` keeps the test window up for eight seconds, long enough for a screenshot.

use std::os::windows::io::AsRawHandle;
use std::os::windows::process::{ChildExt, CommandExt};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Foundation::{
    COLORREF, CloseHandle, E_FAIL, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::System::Threading::{CREATE_SUSPENDED, ResumeThread};
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetLayeredWindowAttributes, GetMessageW, IDC_ARROW, LWA_ALPHA, LoadCursorW, MSG,
    PostQuitMessage, RegisterClassW, SetLayeredWindowAttributes, SetTimer, TranslateMessage,
    WINDOW_EX_STYLE, WM_DESTROY, WM_TIMER, WNDCLASSW, WS_CHILD, WS_EX_LAYERED, WS_OVERLAPPEDWINDOW,
    WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

type Check = Result<String, String>;

fn main() {
    let show = std::env::args().any(|a| a == "--show");
    let checks = [
        ("os", os()),
        ("runtime", runtime()),
        ("process", process()),
        ("job", job()),
        ("https", https()),
        ("window", window(show)),
    ];
    let mut report = String::new();
    let mut failures = 0;
    for (name, result) in checks {
        let line = match result {
            Ok(detail) => format!("ok   {name}: {detail}"),
            Err(error) => {
                failures += 1;
                format!("FAIL {name}: {error}")
            }
        };
        println!("{line}");
        report.push_str(&line);
        report.push_str("\r\n");
    }
    // A run from the Task Scheduler has no console, so the log beside the exe is the only output.
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::write(exe.with_file_name("probe.log"), report);
    }
    std::process::exit(failures);
}

fn os() -> Check {
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a writable OSVERSIONINFOW whose size field is set, as RtlGetVersion requires.
    let status = unsafe { RtlGetVersion(&raw mut info) };
    if status.is_err() {
        return Err(format!("RtlGetVersion returned {status:?}"));
    }
    Ok(format!(
        "Windows {}.{} build {}",
        info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber
    ))
}

fn runtime() -> Check {
    let started = Instant::now();
    let since_epoch = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|e| e.to_string())?;
    let workers: Vec<_> = (0..4).map(|i| std::thread::spawn(move || i * 2)).collect();
    let sum: i32 = workers.into_iter().map(|t| t.join().unwrap_or(-100)).sum();
    // RandomState seeds itself from the OS random source on first use.
    let map: std::collections::HashMap<u32, u32> = (0..64).map(|i| (i, i)).collect();
    let cores = std::thread::available_parallelism().map_or(0, std::num::NonZero::get);
    let dir = std::env::temp_dir().join("pastegrab-probe");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join("тест 🏃.txt");
    std::fs::write(&file, "проверка").map_err(|e| e.to_string())?;
    let back = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(format!(
        "unix time {}, {cores} cores, threads {sum}, map {}, unicode file read back {back:?}, {:?}",
        since_epoch.as_secs(),
        map.len(),
        started.elapsed()
    ))
}

fn process() -> Check {
    let out = Command::new("cmd")
        .args(["/c", "echo", "child ok"])
        .output()
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "exit {}, stdout {:?}",
        out.status,
        String::from_utf8_lossy(&out.stdout).trim()
    ))
}

fn job() -> Check {
    // The child starts suspended, so nothing it spawns runs before the job owns it.
    let mut child = Command::new("ping")
        .args(["-n", "30", "127.0.0.1"])
        .stdout(Stdio::null())
        .creation_flags(CREATE_SUSPENDED.0)
        .spawn()
        .map_err(|e| e.to_string())?;
    let job = match contain(&child) {
        Ok(job) => job,
        Err(e) => {
            let _ = child.kill();
            return Err(format!("job setup: {e}"));
        }
    };
    std::thread::sleep(Duration::from_millis(300));
    let running = matches!(child.try_wait(), Ok(None));
    // SAFETY: `job` came from `contain` and is closed exactly once.
    let _ = unsafe { CloseHandle(job) };
    std::thread::sleep(Duration::from_millis(500));
    match child.try_wait() {
        Ok(Some(status)) if running => Ok(format!(
            "resumed inside the job, and closing the job ended it ({status})"
        )),
        Ok(Some(status)) => Err(format!("the child exited before the job closed ({status})")),
        Ok(None) => {
            let _ = child.kill();
            Err("the child outlived its kill-on-close job".into())
        }
        Err(e) => Err(e.to_string()),
    }
}

fn contain(child: &std::process::Child) -> windows::core::Result<HANDLE> {
    // SAFETY: the job handle is created here and handed to the caller to close; the child's
    // process and thread handles stay valid while `child` lives.
    unsafe {
        let job = CreateJobObjectW(None, PCWSTR::null())?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let setup = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size_of_val(&limits) as u32,
        )
        .and_then(|()| AssignProcessToJobObject(job, HANDLE(child.as_raw_handle())));
        if let Err(e) = setup {
            let _ = CloseHandle(job);
            return Err(e);
        }
        if ResumeThread(HANDLE(child.main_thread_handle().as_raw_handle())) == u32::MAX {
            let _ = CloseHandle(job);
            return Err(E_FAIL.into());
        }
        Ok(job)
    }
}

fn https() -> Check {
    let started = Instant::now();
    let mut response = ureq::get("https://github.com/")
        .call()
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let body = response
        .body_mut()
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "HTTP {status}, {} bytes in {:?}",
        body.len(),
        started.elapsed()
    ))
}

fn window(show: bool) -> Check {
    // SAFETY: standard window setup on this thread; each handle passed on comes from a call above it.
    unsafe {
        let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = w!("pastegrab-probe");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())?,
            lpszClassName: class,
            ..Default::default()
        };
        if RegisterClassW(&raw const wc) == 0 {
            return Err("RegisterClassW failed".into());
        }
        let main = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!("pastegrab probe"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            480,
            200,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .map_err(|e| format!("main window: {e}"))?;
        // Windows 8 is the first version that allows a layered child, which the UI fades rely on.
        let label = CreateWindowExW(
            WS_EX_LAYERED,
            w!("STATIC"),
            w!("Полупрозрачная надпись"),
            WS_CHILD | WS_VISIBLE,
            20,
            20,
            400,
            60,
            Some(main),
            None,
            Some(instance.into()),
            None,
        )
        .map_err(|e| format!("layered child: {e}"))?;
        SetLayeredWindowAttributes(label, COLORREF(0), 128, LWA_ALPHA)
            .map_err(|e| format!("child alpha: {e}"))?;
        let mut alpha = 0u8;
        GetLayeredWindowAttributes(label, None, Some(&raw mut alpha), None)
            .map_err(|e| format!("reading the alpha back: {e}"))?;
        SetTimer(Some(main), 1, if show { 8000 } else { 1000 }, None);
        let started = Instant::now();
        let mut msg = MSG::default();
        while GetMessageW(&raw mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&raw const msg);
            DispatchMessageW(&raw const msg);
        }
        Ok(format!(
            "window with a layered child at alpha {alpha}, message loop ran {:?}",
            started.elapsed()
        ))
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: Windows calls this on the window's own thread with the window's own handle.
    unsafe {
        match msg {
            WM_TIMER => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
