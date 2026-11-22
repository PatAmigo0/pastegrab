//! Version info and a manifest for pastegrab-app.exe.

fn main() {
    pastegrab_winres::embed(
        pastegrab_winres::Kind::Window,
        "pastegrab-app.exe",
        "pastegrab",
    );
}
