# Техническое задание: `auto-tzim`

**Статус:** проектное ТЗ для нового отдельного репозитория  
**Дата:** 2026-10-29  
**Версия ТЗ:** 0.1  
**Связь с `tzim`:** companion/orchestration tool, не fork и не часть текущего PR #84

## 1. Цель проекта

Создать кроссплатформенный инструмент `auto-tzim`, который позволяет пользователю:

1. явно выбрать отдельные папки и правила обработки;
2. предварительно увидеть, какие файлы и с какими параметрами будут обработаны;
3. запускать сжатие вручную или по расписанию через официальный бинарник `tzim`;
4. видеть состояние, историю и причины пропусков/ошибок;
5. получать уведомления об обновлениях `tzim`, а позже — выполнять контролируемое обновление;
6. использовать один движок через CLI, затем TUI и Qt GUI.

Главная ценность проекта — **безопасная оркестрация**, а не реализация алгоритмов сжатия.

## 2. Основные решения и ограничения

### 2.1 Обязательные решения

- `auto-tzim` вызывает внешний установленный `tzim` как subprocess.
- Все решения о формате, eligibility, оптимизации, verification и атомарной замене принимает только `tzim`.
- `auto-tzim` не содержит кодеков, перепаковщиков и собственных реализаций форматов.
- Взаимодействие с `tzim` — через аргументы argv и документированный machine-readable JSON, без shell и без парсинга человекочитаемого вывода.
- Обработка выполняется только от имени текущего пользователя, без root/sudo.
- По умолчанию ничего не сканируется и не изменяется: папку и правила сначала явно создаёт пользователь.
- Первый запуск для новой папки — только dry-run.
- Неизвестные, unsupported, refused и verification-failed файлы никогда не направляются в альтернативный handler.
- Телеметрия и отправка путей/файлов на внешние серверы отсутствуют.

### 2.2 Что не входит в проект

- Изменение алгоритмов и accepted subsets `tzim`.
- `--force`, обход отказов, проверок, signature/resource limits.
- Обещание undo после успешной оптимизации.
- Собственный backup/синхронизация/облачное хранилище в первых версиях.
- Одновременная реализация всех UI и всех способов обновления в первом релизе.
- Автоматическая обработка confirmation-required transformations без заранее явного opt-in пользователя.
- Самообновление `auto-tzim` в MVP.
- Замена бинарника `tzim` во время активных заданий.

## 3. Этапы поставки

| Этап | Содержание | Критерий перехода |
| --- | --- | --- |
| **MVP / v0.1** | Core, CLI, ручной `run --once`, dry-run, config/state/history, проверка версии, update notification | Все safety/integration тесты пройдены на macOS и Linux |
| **v0.2** | Планировщик через launchd/systemd user timers и TUI | Нет overlapping runs, scheduler использует тот же core |
| **v0.3** | Контролируемое обновление `tzim` через Homebrew | Проверен реальный upgrade с предыдущей установленной версии в изолированном окружении |
| **v0.4** | Qt GUI | Qt является тонкой оболочкой над тем же core, без отдельной логики обработки |

Каждый этап — отдельный PR/серия PR в репозитории `auto-tzim`. Нельзя начинать следующий этап, если предыдущий имеет unresolved data-safety findings.

## 4. Рекомендуемая архитектура

### 4.1 Технологии

- **Core и CLI:** Python 3.12+.
- **Конфигурация:** TOML; чтение `tomllib`, атомарная запись через закреплённую небольшую зависимость.
- **State/history:** SQLite из стандартной библиотеки.
- **TUI:** Textual, optional dependency начиная с v0.2.
- **Qt:** PySide6, optional dependency начиная с v0.4.
- **Планировщик:** launchd на macOS и systemd user timers на Linux; отдельный постоянный daemon не требуется.

Core должен иметь минимальные зависимости. Textual/PySide6 нельзя делать обязательными для CLI.

### 4.2 Модули

```text
auto_tzim/
  core/
    config          # чтение, validation, атомарная запись
    capabilities    # версия и поддерживаемый CLI/JSON контракт tzim
    discovery       # безопасный обход и фильтрация файлов
    stability       # regular file, symlink/hard-link и change detection
    planner         # детерминированный список файлов и argv
    runner          # subprocess, JSON, timeout, signals
    policy          # форматы, opt-ins, refusal и retry policy
    state           # SQLite, history, invalidation
    scheduler       # run-once и позже OS scheduler adapters
    updater         # update check и позже controlled update
    logging         # локальные structured logs и retention
  cli/
  tui/              # начиная с v0.2
  qt/               # начиная с v0.4
```

UI не обращается к файлам или `tzim` напрямую: только через public core API.

## 5. Конфигурация и данные

### 5.1 Расположение

- macOS: `~/Library/Application Support/auto-tzim/`
- Linux: `${XDG_CONFIG_HOME:-~/.config}/auto-tzim/` для config и `${XDG_STATE_HOME:-~/.local/state}/auto-tzim/` для state/logs.
- Для тестов: `AUTO_TZIM_HOME` полностью переопределяет расположение.
- Config, SQLite и логи создаются с permissions `0600`; директории — `0700` где поддерживается.
- Изменения config выполняются через temporary file + flush/fsync + atomic rename. Повреждённый config не перезаписывается автоматически.

### 5.2 Пример профиля папки

```toml
schema = 1

[installation]
tzim_path = ""
update_policy = "notify"       # notify | manual | scheduled (scheduled позже)
allowed_major_versions = [0]

[[folder]]
id = "01JEXAMPLE000000000000000000"
name = "Project images"
root = "/Users/example/Pictures/project"
enabled = true
recursive = false               # default для новой папки
include_hidden = false
cross_filesystems = false
follow_symlinks = false         # неизменяемо в MVP
allow_hard_links = false        # default: такие файлы пропускаются
allow_cloud_sync = false
formats = []                    # [] = только default-enabled formats tzim
transforms = []                 # например, отдельный явный "fits-tile" opt-in
exclude_globs = [".git/**", ".hg/**", ".svn/**"]
min_file_age_seconds = 60
jobs = 1
min_free_bytes = 268435456

[folder.schedule]
enabled = false                 # manual-only default
interval_minutes = 60
```

Незнакомые поля — ошибка validation, а не молчаливая опечатка. Пустые/относительные `root`, NUL и недопустимые значения отклоняются.

### 5.3 Ограничения на корни

- Нельзя добавлять filesystem root, `$HOME` целиком и системные каталоги (`/System`, `/usr`, `/etc`, `/var`, `/bin`, `/sbin`, аналоги на поддерживаемых ОС).
- Новый root должен быть существующим каталогом и canonicalized.
- Известные cloud-sync roots требуют отдельного `allow_cloud_sync=true`; предупреждение не считается исчерпывающей проверкой облачных провайдеров.
- Удаление папки из config не удаляет пользовательские файлы и не меняет уже обработанные файлы.

## 6. Discovery и eligibility

### 6.1 Обход

- Обрабатываются только regular files.
- Symlinks не следуются и не обрабатываются как цели.
- Socket/FIFO/device/directory junction и другие специальные объекты пропускаются с причиной.
- По умолчанию не пересекаются mount points и не входят в hidden directories.
- Порядок планирования детерминирован: сортировка по relative path, без отбора по потенциальному gain.
- Перед запуском повторно проверяется, что canonical path остаётся внутри разрешённого root.

### 6.2 Стабильность файла

Файл включается в план только если:

1. `lstat`/metadata совпадают между discovery и pre-run check;
2. размер и `mtime_ns` не меняются в течение `min_file_age_seconds`;
3. файл остаётся regular file с тем же device/inode на поддерживаемых ОС;
4. он не изменился между построением плана и запуском;
5. после завершения процесса фиксируется результат и проверяется отсутствие внешней гонки.

Изменившийся файл получает `skipped_unstable`, не перезаписывается намеренно и переносится в следующий анализ только после новой стабильной идентичности.

Файлы с `st_nlink > 1` по умолчанию пропускаются как `hardlink_skip`. Их поддержка возможна только после отдельного доказанного контракта сохранности всех siblings.

### 6.3 Форматы и transformations

- `formats=[]` означает только default-enabled обработку текущего совместимого `tzim`.
- `formats` остаётся allowlist и не становится consent к структурной трансформации.
- Confirmation-required opt-in включается только отдельным полем `transforms` после явного действия пользователя.
- Scheduler/non-TTY никогда не запрашивает интерактивное подтверждение.
- Если opt-in не задан заранее, соответствующий файл пропускается.
- При смене `tzim`, capabilities или profile policy список eligible файлов пересчитывается, а старые skip-cache entries инвалидируются.

## 7. Взаимодействие с `tzim`

### 7.1 Binary identity и capabilities

`doctor` и каждый новый run записывают:

- configured path и resolved realpath;
- SHA-256 и размер бинарника;
- `tzim --version`;
- поддерживаемые форматы/opt-ins и JSON schema версии, если доступны;
- OS/architecture.

Если machine-readable capabilities отсутствуют, разрешён только version-specific adapter с явно документированным фиксированным контрактом. Нельзя угадывать флаги или парсить human output. Для недостающего machine interface создаётся отдельный issue в `tzim`.

### 7.2 Запуск

- Только argv array, без `shell=True`.
- `stdin=DEVNULL`; prompts невозможны и не обслуживаются.
- JSON stdout проверяется по schema; stderr и malformed JSON сохраняются как diagnostic failure.
- Первый MVP обрабатывает по одному файлу на process и по умолчанию использует `jobs=1`.
- Путь с пробелами, Unicode, начальным `-` и shell-metacharacters не влияет на команду.
- Поддерживаются timeout, graceful SIGINT/SIGTERM и запрет новых запусков после cancellation.
- Параллельные процессы для одного файла запрещены lock/state механизмом.
- `auto-tzim` не выполняет собственную установку candidate поверх оригинала: установку и её гарантии выполняет `tzim`.

### 7.3 Статусы

Минимальный нормализованный набор:

```text
planned
optimized
already_optimal
skipped_unsupported
skipped_policy
skipped_refused
skipped_unstable
hardlink_skip
failed
interrupted
incompatible_tzim
update_required
```

Сырые status/note/error и версия `tzim` сохраняются рядом с нормализованным статусом.

### 7.4 Retry и история

- `already_optimal`, unsupported и policy/refusal skips не повторяются при неизменной file identity, `tzim` version и policy.
- `failed` не повторяется автоматически при каждом scheduler run; повтор — manual или после изменения identity/policy.
- Запись history содержит root, relative path, device/inode, size, mtime_ns, SHA-256 до/после (если вычислены), status/reason, timestamps, argv без секретов и binary identity.
- SQLite transactions не оставляют файл одновременно `running` после завершения процесса; stale runs помечаются `interrupted`, а не `optimized`.

## 8. CLI

### 8.1 Команды MVP

```text
auto-tzim doctor
auto-tzim config validate
auto-tzim folder add PATH [--name NAME] [--recursive] [--formats ...]
auto-tzim folder list
auto-tzim folder show ID
auto-tzim folder edit ID ...
auto-tzim folder enable ID
auto-tzim folder disable ID
auto-tzim folder remove ID
auto-tzim scan [--folder ID]
auto-tzim run --once [--folder ID] [--dry-run]
auto-tzim status
auto-tzim history [--folder ID] [--status STATUS]
auto-tzim logs [--folder ID]
auto-tzim pause
auto-tzim resume
auto-tzim update-check
```

`folder add` не запускает обработку. Для новой папки первый `run --once` требует либо предшествующий просмотр scan/dry-run, либо явный `--dry-run` в том же вызове.

Все команды поддерживают `--json`; JSON идёт только в stdout, diagnostics — в stderr. В non-TTY нет prompts или ожидания stdin.

### 8.2 Exit codes

| Code | Значение |
| --- | --- |
| 0 | Команда выполнена; optimized/already_optimal/документированные skips допустимы |
| 1 | Один или несколько файлов завершились `failed` |
| 2 | Ошибка usage/config |
| 3 | Несовместимый/недоступный `tzim` |
| 4 | Уже выполняется другой run или получение lock невозможно |
| 5 | Ошибка update check/update |

Dry-run возвращает 0 при успешном построении плана и не изменяет файлы или history как будто обработка произошла.

## 9. Планировщик — v0.2

- Используются launchd user agent и systemd user timer, без system daemon.
- По умолчанию schedule выключен.
- Один profile не может иметь overlapping runs; глобальный lock запрещает конкурентные изменения одного файла.
- Пропущенный запуск не вызывает серию catch-up запусков.
- Scheduler вызывает тот же `run --once` core API и не имеет отдельных правил eligibility.
- Пользователь может `pause`, `resume`, отключить schedule и удалить OS unit.
- При logout/отсутствии user session поведение документируется отдельно для каждой ОС.
- Battery/nice/IONice/network-aware policy допускается только позже и не меняет safety semantics.

## 10. TUI — v0.2

TUI должен позволять:

- добавить папку через path input и увидеть canonical root;
- редактировать форматы/excludes/schedule/limits;
- просматривать dry-run список и точные argv/policy до запуска;
- запускать/останавливать run и ставить scheduler на pause;
- видеть текущий файл, счётчики статусов, ошибки и историю;
- просматривать update notification, не применяя обновление неявно.

Опасные изменения (`transforms`, cloud-sync, scheduled update) требуют отдельного экрана подтверждения с default **No**.

## 11. Обновления

### 11.1 MVP — только проверка и уведомление

- Проверка использует только официально документированный источник релизов/package manager.
- Пользователь видит текущую и доступную версии и команду/действие для обновления.
- Нет `curl | sh`, скачивания `main`, самозамены binary или обновления без подтверждения.
- Несовместимая major/JSON schema версия не активируется автоматически.

### 11.2 v0.3 — controlled Homebrew update

- Поддерживается только явно настроенный official Homebrew tap/formula; имя не угадывается.
- Перед обновлением: нет активных runs, сохранены old version/binary identity и lock получен.
- Применяется документированный fully-qualified tap/formula flow, учитывающий реальное поведение уже установленного пакета.
- После обновления: повторные version/capabilities checks, binary hash и read-only/dry-run smoke test.
- Ошибка update не запускает compression и оставляет понятный log.
- Automatic rollback не заявляется, пока он реально не реализован и проверен.
- `update_policy=scheduled` включается только отдельным пользовательским opt-in после успешного ручного update.
- Обновление `auto-tzim` этим механизмом не выполняется.

### 11.3 Acceptance update test

В изолированном тестовом окружении устанавливается предыдущая Release-версия, затем выполняется обновление тем же способом, который получит пользователь. Проверяются фактическая смена версии, запуск binary и совместимость. Проверки только formula на GitHub недостаточно.

## 12. Qt GUI — v0.4

- Нативный выбор папок и профилей.
- Dry-run preview до запуска.
- Таблица статусов/истории и фильтры.
- Pause/resume и scheduler controls.
- Update notifications и controlled update через тот же core.
- Нет отдельного file discovery, subprocess или updater кода.
- GUI не запускается автоматически после установки и не требует admin rights.
- Требования packaging/signing/notarization и sandbox entitlements документируются отдельно до Release.

## 13. Логи, приватность и восстановление

- Логи локальные, structured JSONL + читаемый UI view.
- Секреты и environment variables не записываются; пути считаются чувствительными данными.
- Rotation/retention настраиваются; default — ограниченный размер и 30 дней.
- После crash config/history должны оставаться читаемыми; stale running entries восстанавливаются как interrupted.
- Нет сетевых запросов, кроме явного update-check/update и скачивания официальных package artifacts.
- Backup — отдельная будущая функция. До её появления UI прямо сообщает, что успешная оптимизация изменяет файл и undo не предоставляется.

## 14. Тестирование

### 14.1 Unit tests

- Config schema, unknown fields, atomic write и повреждённые файлы.
- Canonical roots, запрещённые roots, Unicode/пробелы/metacharacters.
- Symlink/special files/mount boundaries/hard links.
- Stable/unstable file transitions и изменение между discovery/pre-run/post-run.
- Profile/version invalidation и retry rules.
- argv construction и status mapping.
- SQLite recovery, lock и signal transitions.
- Update version comparison и несовместимые schemas.

### 14.2 Integration с fake `tzim`

Fake binary должен моделировать:

- все JSON statuses и malformed/partial output;
- nonzero exits, timeout, hang и попытку чтения stdin/prompt;
- version/capabilities mismatch;
- unexpected file mutation и ошибки создания temporary output;
- файлы с любыми допустимыми именами.

Проверяется, что `auto-tzim` не использует shell, не отвечает на prompt и не объявляет success без валидного JSON/exit contract.

### 14.3 Integration с реальным `tzim`

- Только temporary test roots и маленькие разрешённые fixtures.
- Default-enabled formats: фактические optimized/already_optimal/refused/unsupported cases.
- Confirmation-required transformation: пропуск без opt-in и обработка только после отдельного explicit opt-in.
- Byte identity исходников для всех skips/failures.
- Повторный run не создаёт цикл повторной обработки.
- macOS и Linux CI; Qt/TUI тестируются дополнительно к core, не вместо него.

### 14.4 Scheduler/updater tests

- Нет overlapping timers и catch-up storm.
- Pause/disable/remove действительно меняют OS scheduler state.
- Реальный isolated Homebrew upgrade из предыдущей установки.
- Нет compression во время update и нет update во время run.

## 15. Acceptance criteria MVP

MVP принимается только если всё ниже выполнено на macOS и Linux:

1. Пользователь может добавить узкую папку, выполнить dry-run и увидеть точный детерминированный план.
2. Первый запуск без dry-run не изменяет файлы.
3. `run --once` вызывает официальный `tzim` только с вычисленными разрешёнными параметрами.
4. Ни один unknown/unsupported/refused/failed/unstable/hard-linked/symlink файл не изменяется.
5. Нет shell invocation, prompts, root requirement или скрытой фоновой активности.
6. Повторный запуск не создаёт бесконечный retry loop.
7. Config/history/log recovery работает после отмены и crash simulation.
8. JSON/CLI exit codes соответствуют документации.
9. Update check только уведомляет и ничего не заменяет.
10. Документация содержит установку, первый dry-run, риски, отсутствие undo, способы отключить обработку и полный список ограничений.

## 16. Definition of Done для каждого Release

- Все acceptance criteria соответствующего этапа выполнены на конечном SHA.
- CI и raw test evidence опубликованы.
- README/CLI help/config reference/safety documentation совпадают с реальным поведением.
- Нет unresolved data-loss, symlink/race, silent-update или prompt/deadlock findings.
- Release artifacts имеют SHA-256; версии и changelog не переписывают историю.
- Homebrew tap/formula для `auto-tzim`, когда распространение добавляется, обновляется вместе с Release и проверяется реальным upgrade.
- Отсутствие telemetry и список сетевых запросов явно задокументированы.

## 17. Первый implementation assignment после запуска репозитория

Реализовать только **MVP / v0.1**:

1. Инициализировать Python package и core/CLI structure.
2. Добавить config/profile validation и atomic storage.
3. Реализовать безопасный discovery, dry-run planner и state/history.
4. Реализовать version adapter и per-file JSON subprocess runner.
5. Добавить CLI из §8.1 и update-check notification.
6. Покрыть fake/real `tzim` integration tests на macOS/Linux.
7. Подготовить README с первым dry-run и ограничениями.

Не реализовывать в этом задании scheduler daemon, TUI, Qt, automatic update, backup, cloud sync, собственные codecs или изменения в репозитории `tzim`.
