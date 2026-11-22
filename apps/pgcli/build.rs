//! Version info and a manifest for pgcli.exe.

fn main() {
    pastegrab_winres::embed(
        pastegrab_winres::Kind::Console,
        "pgcli.exe",
        "pastegrab command line",
    );
}
