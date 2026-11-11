# mde

Нативный редактор Markdown на Rust для macOS, Linux и Windows: вкладки, подсветка синтаксиса
и живой предпросмотр.

## Установка

Скачайте архив из [Releases](https://github.com/jarymor-ux/mde/releases)
(автоматические сборки помечены **Pre-release**):

| Архив | Платформа |
| --- | --- |
| `mde-macos-arm64.tar.gz` | Mac с Apple Silicon — M1 и новее |
| `mde-macos-x86_64.tar.gz` | Mac с Intel |
| `mde-linux-x86_64.tar.gz` | Linux x86_64, Ubuntu 24.04 / glibc 2.39+ |
| `mde-windows-x86_64-setup.exe` | Windows 10/11 x64, установщик с ярлыком на рабочем столе |
| `mde-windows-x86_64.zip` | Windows 10/11 x64, portable (без установки) |

### macOS

Распакуйте архив и перенесите `mde.app` в `/Applications`.
Приложение подписано ad-hoc, без Apple notarization. Если первый запуск
заблокирован, после попытки открытия разрешите его в **Системные настройки →
Конфиденциальность и безопасность → Всё равно открыть**.

Для терминала в архиве также есть отдельный исполняемый файл `mde`.

### Windows

Запустите `mde-windows-x86_64-setup.exe`: установщик добавляет MDE в меню «Пуск»
и по умолчанию создаёт ярлык на рабочем столе. Для установки не нужны права
администратора. Ярлык запускает графическое приложение без окна консоли.

Portable ZIP: распакуйте архив и дважды нажмите `mde-desktop.exe`.
Для использования из терминала предусмотрен отдельный `mde.exe` (CLI).
Не разделяйте два EXE: графический launcher запускает `mde.exe` из той же папки.
Установщик позволяет удалить программу через список установленных приложений.

### Linux

Нужны графическая сессия X11/Wayland и OpenGL. На Ubuntu 24.04 установите:

```sh
sudo apt-get update
sudo apt-get install -y libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libegl1 libgl1 \
  libxcb-render0 libxcb-shape0 libxcb-xfixes0 libdbus-1-3 \
  xdg-desktop-portal xdg-desktop-portal-gtk zenity
```

Для KDE замените `xdg-desktop-portal-gtk` на `xdg-desktop-portal-kde`.
На других дистрибутивах нужны эквивалентные пакеты; для старой glibc или
Alpine/musl собирайте из исходников.

```sh
tar -xzf mde-linux-x86_64.tar.gz
cd mde-linux-x86_64
./mde examples/demo.md
```

Чтобы установить MDE для текущего пользователя с иконкой в меню приложений и
ярлыком на рабочем столе (если окружение рабочего стола поддерживает их), выполните
в распакованной папке:

```sh
./install.sh
```

Скрипт копирует исполняемый файл в `~/.local/bin/mde` и создаёт
`~/.local/share/applications/MDE.desktop` (с учётом `XDG_DATA_HOME`).
На GNOME может понадобиться выбрать «Разрешить запуск» для ярлыка на рабочем столе.

### Команда `mde` в терминале

На macOS и Linux можно также вручную выполнить внутри распакованной папки:

```sh
mkdir -p "$HOME/.local/bin"
install -m 755 mde "$HOME/.local/bin/mde"
export PATH="$HOME/.local/bin:$PATH"
```

Если `~/.local/bin` ещё нет в PATH, добавьте последнюю строку в конфигурацию
своей оболочки, например `~/.zshrc` или `~/.bashrc`.

## Запуск

```sh
mde                      # Стартовый экран
mde notes.md             # Файл
mde one.md two.md         # Несколько вкладок
mde .                    # Папка с документами
mde --foreground notes.md # Запуск с выводом ошибок в терминал
mde --help               # Справка
```

Обычный запуск сразу освобождает терминал — добавлять `&` не требуется.

## Возможности

- Редактор, предпросмотр рядом и режим чтения; редактирование абзацев прямо в предпросмотре.
- Подсветка Markdown и блоков Rust, Python, JavaScript/JSX, JSON и Bash.
- Таблицы, списки задач, сноски и локальные PNG/JPEG.
- Вкладки, дерево файлов, недавние документы, поиск и замена, undo/redo.
- Автосохранение через секунду после правки; новому документу сначала нужно выбрать имя.
- Светлые и тёмные темы: Catppuccin, Dracula, Nord, Gruvbox, Tokyo Night и Solarized.

По умолчанию элементы управления скрыты. Откройте их правой кнопкой мыши
или сочетанием ⌘/Ctrl Shift P.

## Клавиши

На macOS используется ⌘, на Linux — Ctrl.

| Клавиша | Действие |
| --- | --- |
| ⌘/Ctrl N / O / S | Новый документ / открыть / сохранить |
| ⌘/Ctrl Shift O / S | Открыть папку / сохранить как |
| ⌘/Ctrl W / Q | Закрыть вкладку / приложение |
| ⌘/Ctrl F | Поиск и замена |
| ⌘/Ctrl Shift P | Управление, темы и вкладки |
| ⌘/Ctrl Shift H | Показать или скрыть весь интерфейс |
| ⌘/Ctrl B | Показать или скрыть дерево файлов |
| Ctrl Tab / Ctrl Shift Tab | Следующая / предыдущая вкладка |
| ⌘/Ctrl 1 / 2 / 3 | Редактор / вместе / просмотр |
| Escape | Завершить редактирование блока или скрыть поиск |

## Сборка из исходников

Нужны [Rust 1.95+ и Cargo](https://rustup.rs/). На macOS также установите
Command Line Tools: `xcode-select --install`.

На Ubuntu 24.04 установите зависимости сборки:

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libssl-dev libudev-dev \
  libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libegl1-mesa-dev libgl1-mesa-dev
```

На обеих платформах:

```sh
git clone https://github.com/jarymor-ux/mde.git
cd mde
cargo install --path . --locked
mde examples/demo.md
```

Для macOS-приложения: `./scripts/package-macos.sh`, результат — `target/mde.app`.

[GitHub Actions](https://github.com/jarymor-ux/mde/actions/workflows/release.yml)
при push в `main` или `master` проверяет код и публикует prerelease с
сборками macOS, Linux и Windows (включая Windows-установщик) и `SHA256SUMS`. Pull request проходят проверки без публикации;
ручной запуск доступен в Actions. На Linux дополнительно проверяется запуск GUI.

## Ограничения

HTML, Mermaid, LaTeX и удалённые изображения не поддерживаются.
Нет слежения за изменениями файлов на диске и синхронной прокрутки панелей.
Максимальный размер UTF-8 файла — 32 MiB. Для файлов больше 512 KiB предпросмотр
включается вручную, больше 1 MiB — отключается подсветка. При конфликте сохранения
используйте «Сохранить как» или заново откройте файл.

## Лицензия

[mde Source-Available License 1.0 — No Monetization](LICENSE): можно бесплатно
использовать, изменять, форкать и распространять с сохранением условий лицензии.
Использование в коммерческих компаниях и создание документов для оплачиваемой
работы разрешены.
Монетизация самой программы и любых изменённых версий запрещена, включая
продажу, подписки, рекламу, пожертвования и платные услуги для третьих лиц.

Лицензии зависимостей сохранены в [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt).
Первый коммит `cb3ed64` был опубликован под MIT; новые условия не отзывают ранее
выданные права на ту версию.
