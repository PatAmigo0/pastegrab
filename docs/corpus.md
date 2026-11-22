# Corpus

**`pgcli` on Linux with yt-dlp 2026.10.26 and ffmpeg 9.0.2, on 2026-11-22.** The links come from yt-dlp's `_TESTS` for each site, plus one direct file. olympics.com waits for the check in section 15 of the spec.

| Site | Link | What happened |
|---|---|---|
| YouTube | `jNQXAC9IVRw` | 134+140 merged into MP4 and kept as is. Sound only, MP3, «Для анализа» and the MKV profile passed too |
| VK | `video205387401_165548505` | HLS fMP4 video and DASH sound merged into MP4 |
| Rutube | `3eac3b4561676c17df9132a9a1e62e3e` | one muxed HLS stream, Cyrillic title intact |
| OK.ru | `1484130554189` | formats without heights, the bigger one picked, MP3 sound encoded to AAC |
| archive.org | `Cops1922` | formats without codecs; the planner takes 480p and ffprobe decides after the download |
| archive.org | `Election_Ads` | ten videos on one page, refused until playlists arrive |
| archive.org | a `_512kb.mp4` from `Election_Ads` | downloaded directly, without yt-dlp |
| Dailymotion | `x2iuewm` | «Not found.», read as unavailable |
| smotrim | `1539617` | yt-dlp couldn't parse the site's JSON, read as an outdated engine |
| Vimeo | `56015672` | needs a login, read as such |
