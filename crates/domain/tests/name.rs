//! File names against what Windows 8 refuses, strips or can't fit.

// Fixture helpers outside #[test] fns fall outside clippy's test exemption.
#![allow(clippy::expect_used)]

use pastegrab_domain::name::output_name;
use proptest::prelude::*;

fn free(_: &str) -> bool {
    false
}

fn name(title: &str) -> String {
    output_name(title, "id123", "mp4", 200, free).expect("fits")
}

#[test]
fn forbidden_characters_become_readable() {
    assert_eq!(
        name("Слава Князовицкий: серебро | Атланта 1996?"),
        "Слава Князовицкий - серебро - Атланта 1996.mp4"
    );
    assert_eq!(name("a/b\\c <\"d\"> *"), "a - b - c 'd'.mp4");
}

#[test]
fn trailing_dots_and_spaces_go() {
    assert_eq!(name("Final...   "), "Final.mp4");
}

#[test]
fn reserved_device_names_get_an_underscore() {
    assert_eq!(name("CON"), "CON_.mp4");
    // Windows reads the device word before the first dot, spaces after it ignored.
    assert_eq!(name("nul.backup"), "nul_.backup.mp4");
    assert_eq!(name("nul .backup"), "nul_ .backup.mp4");
    assert_eq!(name("Lpt1"), "Lpt1_.mp4");
    assert_eq!(name("com¹"), "com¹_.mp4");
    assert_eq!(name("CONIN$"), "CONIN$_.mp4");
    assert_eq!(name("CONCERT"), "CONCERT.mp4");
    // A tight budget that cuts the underscore away must not bring the device back.
    assert_eq!(
        output_name("con.x", "id", "mp4", 7, free).as_deref(),
        Some("co.mp4")
    );
}

#[test]
fn an_empty_title_takes_the_fallback() {
    assert_eq!(name("???"), "id123.mp4");
    assert_eq!(
        output_name("", "", "m4a", 50, free).as_deref(),
        Some("video.m4a")
    );
}

#[test]
fn a_taken_name_gets_a_number() {
    let taken = |n: &str| n == "race.mp4" || n == "race (2).mp4";
    assert_eq!(
        output_name("race", "x", "mp4", 50, taken).as_deref(),
        Some("race (3).mp4")
    );
}

#[test]
fn emoji_count_twice_and_never_split() {
    // Each 🏃 is two UTF-16 units; ".mp4" takes four more.
    let got = output_name("🏃🏃🏃🏃", "x", "mp4", 9, free).expect("fits");
    assert_eq!(got, "🏃🏃.mp4");
}

#[test]
fn no_room_means_no_name() {
    assert_eq!(output_name("race", "x", "mp4", 4, free), None);
    assert_eq!(
        output_name("race", "x", "mp4", 5, free).as_deref(),
        Some("r.mp4")
    );
}

proptest! {
    #[test]
    fn names_fit_and_stay_legal(title in ".{0,300}", budget in 6_usize..260) {
        if let Some(n) = output_name(&title, "id", "mp4", budget, free) {
            prop_assert!(n.encode_utf16().count() <= budget);
            prop_assert!(!n.chars().any(|c| "<>:\"/\\|?*".contains(c) || c.is_control()));
            let stem = n.strip_suffix(".mp4").expect("extension");
            prop_assert!(!stem.is_empty());
            prop_assert!(!stem.ends_with(['.', ' ']));
        }
    }
}
