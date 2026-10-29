//! The Russian catalog: English source text on the left, translation on the
//! right. Adding a language means copying this file and registering it in
//! `catalog` — nothing else in the project knows about languages.
//!
//! The numbered holes are part of the key: a translation has to keep every one
//! of them, though it may put them in another order.

pub const CATALOG: &[(&str, &str)] = &[
    // help and arguments
    (
        "clistatus — Claude and Codex limits in your terminal.

Usage:
  clistatus            status line: session JSON on stdin, line on stdout
  clistatus install    register in ~/.claude/settings.json
  clistatus install codex  install the Codex footer bridge for our renderer
  clistatus codex [ARGS]   launch Codex with our colored status line
  clistatus render codex   render Codex session JSON from stdin
  clistatus check      check whether a new version is out
  clistatus update     download the latest version and replace itself
  clistatus uninstall  remove the status line, the cache and the binary
  clistatus divoom     limits panel on a Divoom Times Gate (divoom help)
  clistatus version    print the version
  clistatus help       this help

Environment:
  CLISTATUS_LANG=ru|en         force the interface language
  CLISTATUS_NO_AUTO_UPDATE=1   do not check for updates in the background
",
        "clistatus — лимиты Claude и Codex в терминале.

Использование:
  clistatus            строка статуса: JSON сессии на stdin, строка на stdout
  clistatus install    прописать себя в ~/.claude/settings.json
  clistatus install codex  установить подключение нашего рендера к Codex
  clistatus codex [ARGS]   запустить Codex с нашими цветными полосками
  clistatus render codex   отрисовать JSON сессии Codex из stdin
  clistatus check      проверить, вышла ли новая версия
  clistatus update     скачать последнюю версию и заменить себя ею
  clistatus uninstall  убрать строку статуса, кэш и сам бинарь
  clistatus divoom     панель лимитов на Divoom Times Gate (divoom help)
  clistatus version    показать версию
  clistatus help       эта справка

Переменные окружения:
  CLISTATUS_LANG=ru|en         задать язык интерфейса
  CLISTATUS_NO_AUTO_UPDATE=1   не проверять обновления в фоне
",
    ),
    (
        "Unknown command: {0}\n\n{1}",
        "Неизвестная команда: {0}\n\n{1}",
    ),
    ("built from source", "сборка из исходников"),
    // status line
    ("reset", "сброс"),
    ("{0}h {1}m", "{0}ч {1}м"),
    // install and uninstall
    (
        "Codex footer connected. Start a new session with: codex",
        "Строка статуса подключена. Новые сессии запускай обычной командой: codex",
    ),
    (
        "Original Codex launcher restored. The transport remains cached.",
        "Прежний запускатель Codex восстановлен. Подключение к футеру осталось в кэше.",
    ),
    (
        "Codex transport is installed. Run: clistatus codex",
        "Подключение к футеру Codex установлено. Запуск: clistatus codex",
    ),
    (
        "Downloading Codex footer transport ({0} MB)...",
        "Скачиваю подключение к футеру Codex ({0} МБ)...",
    ),
    (
        "Codex transport is unavailable: {0}",
        "Подключение к футеру Codex недоступно: {0}",
    ),
    (
        "Could not launch Codex: {0}",
        "Не удалось запустить Codex: {0}",
    ),
    (
        "Unsupported Codex platform: {0}/{1}",
        "Нет сборки подключения к Codex для {0}/{1}",
    ),
    (
        "could not migrate {0}: {1}",
        "не удалось перенести {0}: {1}",
    ),
    (
        "Usage: clistatus install [claude|codex]",
        "Использование: clistatus install [claude|codex]",
    ),
    (
        "Codex limit indicators enabled in {0}",
        "Индикаторы лимитов Codex включены в {0}",
    ),
    (
        "Codex limit indicators are already enabled in {0}",
        "Индикаторы лимитов Codex уже включены в {0}",
    ),
    (
        "Restart Codex CLI to apply. These are native Codex limits; custom Claude bars and Divoom snapshots are not available through this integration.",
        "Перезапусти Codex CLI для применения. Это встроенные лимиты Codex; собственные полоски Claude и снимки для Divoom через эту интеграцию недоступны.",
    ),
    ("could not read {0}: {1}", "не удалось прочитать {0}: {1}"),
    (
        "could not back up {0}: {1}",
        "не удалось сделать бэкап {0}: {1}",
    ),
    (
        "{0}: tui must be a table and tui.status_line must be an array of strings",
        "{0}: tui должен быть таблицей, а tui.status_line — массивом строк",
    ),
    (
        "{0} does not parse — fix it by hand",
        "{0} не разбирается — поправьте его вручную",
    ),
    (
        "could not back up settings.json: {0}",
        "не удалось сделать бэкап settings.json: {0}",
    ),
    (
        "could not read settings.json: {0}",
        "не удалось прочитать settings.json: {0}",
    ),
    (
        "Replacing the previous status line: {0}",
        "Заменяю прежнюю строку статуса: {0}",
    ),
    (
        "The previous settings are in {0}.bak",
        "Прежние настройки — в {0}.bak",
    ),
    ("could not create {0}: {1}", "не удалось создать {0}: {1}"),
    (
        "Status line registered in {0}",
        "Строка статуса прописана в {0}",
    ),
    (
        "\n{0} is not in PATH — the clistatus command will not be found.",
        "\nКаталог {0} не в PATH — команда clistatus не найдётся.",
    ),
    (
        "Line for ~/.zshrc:  export PATH=\"{0}:$PATH\"",
        "Строка для ~/.zshrc:  export PATH=\"{0}:$PATH\"",
    ),
    (
        "Command for PowerShell:  [Environment]::SetEnvironmentVariable(\"Path\", [Environment]::GetEnvironmentVariable(\"Path\", \"User\") + \";{0}\", \"User\")",
        "Команда для PowerShell:  [Environment]::SetEnvironmentVariable(\"Path\", [Environment]::GetEnvironmentVariable(\"Path\", \"User\") + \";{0}\", \"User\")",
    ),
    ("Removed {0}", "Удалён {0}"),
    ("Removed the cache {0}", "Удалён кэш {0}"),
    (
        "The binary is still there — remove it by hand: {0}",
        "Бинарь остался — удалите вручную: {0}",
    ),
    ("Removed the binary {0}", "Удалён бинарь {0}"),
    (
        "There is no {0} — nothing to clean in the settings",
        "{0} нет — настройки чистить не нужно",
    ),
    ("There is no status line in {0}", "В {0} строки статуса нет"),
    (
        "{0} holds a status line that is not ours — leaving it as is:\n  {1}",
        "В {0} прописана не наша строка статуса — оставляю как есть:\n  {1}",
    ),
    (
        "Status line removed from {0} (the previous settings are in {0}.bak)",
        "Строка статуса убрана из {0} (прежние настройки — в {0}.bak)",
    ),
    (
        "could not determine our own path: {0}",
        "не удалось определить свой путь: {0}",
    ),
    // version check and update
    (
        "{0} installed, {1} is out — update with: clistatus update",
        "Установлено {0}, вышло {1} — обновиться: clistatus update",
    ),
    (
        "Not a release build, the latest version is {0}",
        "Сборка не из релиза, последняя версия — {0}",
    ),
    (
        "The latest version is installed: {0}",
        "Установлена последняя версия: {0}",
    ),
    (
        "Already the latest version: {0}",
        "Уже последняя версия: {0}",
    ),
    ("==> Installing {0}", "==> Установка {0}"),
    ("==> Updating {0} → {1}", "==> Обновление {0} → {1}"),
    (
        "Done: {0}. The status line picks it up by itself, no need to restart Claude Code.",
        "Готово: {0}. Строка статуса обновится сама, перезапускать Claude Code не нужно.",
    ),
    (
        "could not download the checksums: {0}",
        "не удалось скачать контрольные суммы: {0}",
    ),
    (
        "release {0} has no binary for {1}",
        "в релизе {0} нет бинаря для {1}",
    ),
    ("could not download {0}: {1}", "не удалось скачать {0}: {1}"),
    (
        "the checksum of {0} does not match — the file is broken or tampered with",
        "контрольная сумма {0} не сошлась — файл побился или подменён",
    ),
    ("could not write {0}: {1}", "не удалось записать {0}: {1}"),
    (
        "could not move the previous binary away: {0}",
        "не удалось убрать прежний бинарь: {0}",
    ),
    (
        "could not put the new binary in place: {0}",
        "не удалось поставить новый бинарь: {0}",
    ),
    (
        "could not find out the latest version: {0}",
        "не удалось узнать последнюю версию: {0}",
    ),
    (
        "could not parse the GitHub response: {0}",
        "не удалось разобрать ответ GitHub: {0}",
    ),
    ("{0} has no releases yet", "у {0} ещё нет релизов"),
    ("{0} answered {1}", "{0} ответил {1}"),
    // divoom: help and arguments
    (
        "clistatus divoom — limits panel on the screen of a Divoom Times Gate.

Usage:
  clistatus divoom on [N]       find the Divoom devices and turn the panel on device N
  clistatus divoom off          turn the panel off and give the screen its clock face back
  clistatus divoom              keep the panel updated (works while running)
  clistatus divoom once         send the panel once and exit
  clistatus divoom screen [N]   show or take over screen 1–5
  clistatus divoom preview FILE save a frame to a file without touching the device

The settings are divoom.json in the application directory, created by on.
",
        "clistatus divoom — панель лимитов на экране Divoom Times Gate.

Использование:
  clistatus divoom on [N]       найти устройства Divoom и включить панель на устройстве N
  clistatus divoom off          выключить панель и вернуть экрану циферблат
  clistatus divoom              держать панель обновлённой (работает, пока запущен)
  clistatus divoom once         отправить панель один раз и выйти
  clistatus divoom screen [N]   показать или занять экран 1–5
  clistatus divoom preview FILE сохранить кадр в файл, не трогая устройство

Настройки — divoom.json в каталоге приложения, создаёт on.
",
    ),
    (
        "name a file: clistatus divoom preview panel.gif",
        "укажите файл: clistatus divoom preview panel.gif",
    ),
    (
        "unknown command: {0}\n\n{1}",
        "неизвестная команда: {0}\n\n{1}",
    ),
    // divoom: device and bridge
    ("Device {0}: {1}", "Устройство {0}: {1}"),
    (
        "Divoom devices on the network:",
        "Устройства Divoom в сети:",
    ),
    ("Divoom device", "устройство Divoom"),
    (
        "Which one gets the panel? 1–{0}: ",
        "На какое повесить панель? 1–{0}: ",
    ),
    (
        "the device is a number from 1 to {0}, not {1}",
        "устройство — число от 1 до {0}, а не {1}",
    ),
    (
        "there is more than one device — name the number: clistatus divoom on N",
        "устройств больше одного — укажите номер: clistatus divoom on N",
    ),
    (
        "the chosen device is not on the network — choose again: clistatus divoom on",
        "выбранного устройства нет в сети — выберите заново: clistatus divoom on",
    ),
    (
        "the device did not take the frame",
        "устройство не забрало кадр",
    ),
    (
        "Panel on screen {0} of device {1}, updated every {2}",
        "Панель на экране {0} устройства {1}, обновление каждые {2}",
    ),
    (
        "the device has been unreachable for too long, leaving",
        "устройство недоступно слишком долго, выхожу",
    ),
    (
        "the panel is not turned on — clistatus divoom on",
        "панель не включена — clistatus divoom on",
    ),
    ("{0} does not parse: {1}", "{0} не разбирается: {1}"),
    (
        "the device is unreachable: {0}",
        "устройство недоступно: {0}",
    ),
    (
        "the device answered with something unexpected: {0}",
        "устройство ответило неожиданным: {0}",
    ),
    (
        "the device rejected the command: {0}",
        "устройство отклонило команду: {0}",
    ),
    (
        "the device has no screen {0}",
        "у устройства нет экрана {0}",
    ),
    (
        "no Divoom devices are visible on this network",
        "устройств Divoom в этой сети не видно",
    ),
    ("the bridge is already running", "мост уже запущен"),
    ("The Divoom bridge is stopped", "Остановлен мост Divoom"),
    (
        "The panel is on screen {0} of {1}",
        "Панель на экране {0} из {1}",
    ),
    (
        "the screen is a number from 1 to {0}, not {1}",
        "экран — число от 1 до {0}, а не {1}",
    ),
    (
        "The panel is on screen {0} already",
        "Панель и так на экране {0}",
    ),
    (
        "The panel moved to screen {0}",
        "Панель переехала на экран {0}",
    ),
    (
        "The panel is on, screen {0}",
        "Панель включена на экране {0}",
    ),
    (
        "The panel is off, the screen got its clock face back",
        "Панель выключена, экрану возвращён его циферблат",
    ),
    ("no snapshot found", "снапшот не найден"),
    ("the snapshot is damaged", "снапшот повреждён"),
    ("the snapshot holds no limits", "лимитов в снапшоте нет"),
    (
        "could not draw the panel: {0}",
        "не удалось нарисовать панель: {0}",
    ),
    // divoom: labels drawn on the panel itself. Every character here must have a
    // glyph in the font of divoomkit, and the string has to fit its bar.
    ("RESET", "СБРОС"),
    ("NO", "НЕТ"),
    ("DATA", "ДАННЫХ"),
];
