//! Version info and a manifest for every Windows exe pastegrab builds. Unknown exes without
//! them look worse to antivirus heuristics, and Windows 8 needs the manifest for layered child
//! windows. Elsewhere the build script call does nothing.

#![forbid(unsafe_code)]

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

/// What the exe asks of Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A console tool.
    Console,
    /// A window: common controls 6 and the screen's DPI.
    Window,
    /// The installer, a window that also starts on Windows 7 to say it needs 8.
    Setup,
}

/// Links version info and a manifest into `file`, such as `pgcli.exe`, from a build script.
/// A failure stops the build with a `cargo::error` line.
#[expect(
    clippy::print_stdout,
    reason = "cargo reads build script directives from stdout"
)]
pub fn embed(kind: Kind, file: &str, description: &str) {
    if let Err(e) = try_embed(kind, file, description) {
        println!("cargo::error={e}");
    }
}

fn try_embed(kind: Kind, file: &str, description: &str) -> Result<(), String> {
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is unset")?);
    let manifest = out.join("app.manifest");
    fs::write(&manifest, manifest_xml(kind)).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let rc = out.join("app.rc");
    let path = manifest.to_string_lossy().replace('\\', "\\\\");
    fs::write(&rc, resource_script(file, description, &path))
        .map_err(|e| format!("{}: {e}", rc.display()))?;
    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_required()
        .map_err(|e| format!("{file}: {e}"))
}

fn resource_script(file: &str, description: &str, manifest: &str) -> String {
    let part = |name: &str| env::var(name).unwrap_or_else(|_| "0".into());
    let (major, minor, patch) = (
        part("CARGO_PKG_VERSION_MAJOR"),
        part("CARGO_PKG_VERSION_MINOR"),
        part("CARGO_PKG_VERSION_PATCH"),
    );
    let version = format!("{major}.{minor}.{patch}");
    let stem = file.strip_suffix(".exe").unwrap_or(file);
    let mut rc = String::new();
    let _ = write!(
        rc,
        r#"1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3f
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "pastegrab"
      VALUE "FileDescription", "{description}"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "{stem}"
      VALUE "LegalCopyright", "Apache License 2.0"
      VALUE "OriginalFilename", "{file}"
      VALUE "ProductName", "pastegrab"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
1 24 "{manifest}"
"#
    );
    rc
}

fn manifest_xml(kind: Kind) -> String {
    let windows7 = if kind == Kind::Setup {
        r#"<supportedOS Id="{35138b9a-5d96-4fbd-8e2d-a2440225f93a}"/>"#
    } else {
        ""
    };
    let window = if kind == Kind::Console {
        ""
    } else {
        r#"<dependency><dependentAssembly><assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/></dependentAssembly></dependency>
  <application xmlns="urn:schemas-microsoft-com:asm.v3"><windowsSettings><dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true</dpiAware></windowsSettings></application>"#
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1"><application>
    {windows7}
    <supportedOS Id="{{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}}"/>
    <supportedOS Id="{{1f676c76-80e1-4239-95bb-83d0f6d0da78}}"/>
    <supportedOS Id="{{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}}"/>
  </application></compatibility>
  {window}
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3"><security><requestedPrivileges>
    <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
  </requestedPrivileges></security></trustInfo>
</assembly>
"#
    )
}
