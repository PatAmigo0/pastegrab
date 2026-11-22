//! Version info and a manifest for pastegrab.exe.

fn main() {
    pastegrab_winres::embed(pastegrab_winres::Kind::Window, "pastegrab.exe", "pastegrab");
}
