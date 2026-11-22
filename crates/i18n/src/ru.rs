use crate::{Columns, Failures, NoPlan, States, Strings};

const KB: u64 = 1000;
const MB: u64 = 1000 * KB;
const GB: u64 = 1000 * MB;

/// Russian.
pub const RU: Strings = Strings {
    paste_hint: "Вставьте ссылку (Ctrl+V)",
    analyze: "OK",
    download: "Скачать",
    save_to: "Сохранять в:",
    change_folder: "Изменить",
    columns: Columns {
        title: "Название",
        state: "Состояние",
    },
    no_links: "В тексте нет ссылки",
    profiles: [
        "Видео",
        "Видео, лучшее качество",
        "Видео, малый размер",
        "Только звук",
        "Только звук, MP3",
        "Для анализа",
    ],
    profile_details: [
        "MP4, качество по размеру экрана",
        "MKV в наилучшем качестве, нужен VLC или MPC-HC",
        "MP4 не выше 480p, файл меньше",
        "Файл M4A",
        "Файл MP3",
        "MP4 с наибольшей частотой кадров, до 1080p",
    ],
    states: States {
        queued: "В очереди",
        probing: "Узнаю о видео",
        ready: "Готово к загрузке",
        downloading: "Загрузка",
        merging: "Сборка файла",
        converting: "Конвертация",
        done: "Готово",
        paused: "Пауза",
        cancelled: "Отменено",
    },
    failures: Failures {
        engine_outdated: "Сайт изменился, нужна новая версия загрузчика",
        bot_wall: "Сайт не пускает программу",
        bot_check: "Сайт просит подтвердить, что вы не робот. Попробуйте позже",
        needs_login: "Сайт требует вход в аккаунт",
        unavailable: "Видео удалено или закрыто для вашей страны",
        not_started: "Трансляция ещё не началась",
        unsupported: "На странице нет видео. Скопируйте ссылку на само видео",
        expired: "Ссылка устарела. Нажмите «Повторить»",
        network: "Нет связи с интернетом",
        disk_full: "На диске закончилось место",
        tool_broken: "Часть программы не запускается. Установите pastegrab заново",
        no_format: "Нет подходящего формата",
        internal: "Непредвиденная ошибка. Отправьте отчёт разработчику",
    },
    no_plan: NoPlan {
        no_usable_format: "У видео нет форматов, которые можно скачать",
        no_video: "Здесь только звук, без видео",
        no_audio: "В этом видео нет звука",
    },
    size,
    speed,
    duration,
};

fn size(bytes: u64) -> String {
    if bytes >= GB {
        let tenths = bytes.saturating_add(GB / 20) / (GB / 10);
        format!("около {},{} ГБ", tenths / 10, tenths % 10)
    } else if bytes >= MB {
        format!("около {} МБ", bytes.saturating_add(MB / 2) / MB)
    } else {
        format!("около {} КБ", (bytes.saturating_add(KB / 2) / KB).max(1))
    }
}

fn speed(per_second: u64) -> String {
    if per_second >= MB {
        let tenths = per_second / (MB / 10);
        format!("{},{} МБ/с", tenths / 10, tenths % 10)
    } else {
        format!("{} КБ/с", per_second / KB)
    }
}

fn duration(seconds: u32) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::{duration, size, speed};

    #[test]
    fn numbers_read_the_russian_way() {
        assert_eq!(size(1_830_000_000), "около 1,8 ГБ");
        assert_eq!(size(631_667), "около 632 КБ");
        assert_eq!(size(20), "около 1 КБ");
        assert_eq!(size(640_400_000), "около 640 МБ");
        assert_eq!(speed(2_150_000), "2,1 МБ/с");
        assert_eq!(speed(640_000), "640 КБ/с");
        assert_eq!(duration(19), "0:19");
        assert_eq!(duration(9664), "2:41:04");
    }
}
