# Техническое задание: `auto-tzim`

- **Статус:** проектное ТЗ для нового отдельного репозитория
- **Дата:** 2026-10-29
- **Версия ТЗ:** 0.2
- **Ревизия:** учтены findings P-01 (capabilities), P-02 (`tzim --dry-run` пишет temporary candidate) и P-03 (exit codes)
- **Связь с `tzim`:** companion/orchestration tool, не fork и не часть текущего PR #84

## 1. Цель проекта

Создать кроссплатформенный инструмент `auto-tzim`, который позволяет пользователю:

1. явно выбрать отдельные папки и правила обработки;
2. предварительно увидеть read-only план кандидатов и точные параметры возможного запуска, не принимая этот план за гарантию eligibility или gain;
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
- До первого apply для новой папки обязателен read-only scan/plan. `auto-tzim` dry-run не вызывает `tzim --dry-run` и не создаёт файлы внутри target roots.
- Отсутствие machine-readable capabilities не компенсируется парсингом `--info`, `--list-formats` или свободного `note`.
- Неизвестные, unsupported, refused и verification-failed файлы никогда не направляются в альтернативный handler.
- Телеметрия и отправка путей/файлов на внешние серверы отсутствуют.

### 2.2 Что не входит в проект

- Изменение алгоритмов и accepted subsets `tzim`.
- `--force`, обход отказов, проверок, signature/resource limits.
- Обещание undo после успешной оптимизации.
- Собственный backup/синхронизация/облачное хранилище в первых версиях.
- Одновременная реализация всех UI и всех способов обновления в первом релизе.
- Confirmation-required transformations в MVP: `transforms` обязан быть пустым. Их поддержка возможна позже только через отдельный явный opt-in и доказанный noninteractive контракт.
- Вызов `tzim --dry-run` в MVP: он создаёт/верифицирует temporary candidate в target folder и не является read-only preview.
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
formats = []                    # [] = делегировать default policy tzim, не перечисляя её
transforms = []                 # MVP: любое другое значение — config error
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

- `formats=[]` означает запуск совместимого `tzim` **без флага `--formats`**: default policy делегируется самому бинарнику. `auto-tzim` не пытается перечислить или восстановить default-enabled set.
- В UI такой режим называется «default policy выбранного `tzim`; состав не перечислен», а не показывается выдуманный список форматов.
- Непустой `formats` остаётся локальным allowlist и разрешён только для версии/флагов, явно поддерживаемых adapter. Он не становится consent к структурной трансформации.
- Нельзя выводить capabilities из человекочитаемых `--info`/`--list-formats`, расширений файлов, прошлых результатов или свободного `note`.
- В MVP `transforms=[]` обязателен; непустое значение — config error. Confirmation-required transformations не запускаются и не предлагаются как доступные.
- Scheduler/non-TTY никогда не запрашивает интерактивное подтверждение; subprocess не получает PTY.
- Planner возвращает `planned_candidate`, а не утверждение `eligible`. Фактические eligibility, refusal и gain становятся известны только из результата реального apply.
- При смене `tzim`, binary identity, известных capabilities или profile policy план и skip-cache инвалидируются.

## 7. Взаимодействие с `tzim`

### 7.1 Binary identity и ограниченные capabilities

`doctor` и каждый новый run записывают:

- configured path и resolved realpath;
- SHA-256 и размер бинарника;
- machine-readable `tzim --version`;
- OS/architecture;
- JSON result contract, поддержанный конкретным adapter;
- capabilities только если они получены machine-readable способом; иначе явное `capabilities_unavailable`.

Текущее отсутствие machine-readable `--info`/`--list-formats` и structured refusal reasons — ограничение интеграции, а не повод парсить текст. MVP использует version-specific adapter с минимальным документированным контрактом:

1. поддержанные semver/JSON результат явно перечислены в adapter;
2. default mode вызывается без `--formats` и без handler-specific opt-ins;
3. неизвестная версия или несовпавший JSON contract даёт `incompatible_tzim`;
4. смена binary hash инвалидирует cache и требует повторного `doctor`, даже если semver не изменился;
5. свободный `note` сохраняется только как diagnostic text и не переводится в машинные reason codes.

Отсутствие полного capabilities API не блокирует этот restricted MVP. Для будущих explicit formats/transforms менеджер создаёт отдельный upstream issue `tzim`: machine-readable schema version, build features, default/all formats, opt-ins, prompt/noninteractive semantics и структурированные result reason codes. До появления такого API нельзя расширять adapter догадками.

### 7.2 Запуск

- Только argv array, без `shell=True` и без PTY.
- `stdin=DEVNULL`; prompts невозможны и не обслуживаются. Попытка процесса читать подтверждение считается integration failure, а не приглашением эмулировать ответ.
- MVP вызывает обработку `tzim` только для реального, заранее разрешённого apply. Во время scan/dry-run запрещены file-processing invocations и `tzim --dry-run`; единственный допустимый вызов бинарника — `tzim --version` без target paths, если он нужен adapter.
- JSON stdout проверяется по schema; stderr и malformed JSON сохраняются как diagnostic failure.
- Первый MVP обрабатывает по одному файлу на process и по умолчанию использует `jobs=1`.
- Путь с пробелами, Unicode, начальным `-` и shell-metacharacters не влияет на команду.
- Поддерживаются timeout, graceful SIGINT/SIGTERM и запрет новых запусков после cancellation.
- Параллельные процессы для одного файла запрещены lock/state механизмом.
- `auto-tzim` не выполняет собственную установку candidate поверх оригинала: установку и её гарантии выполняет `tzim`.

### 7.3 Статусы

Минимальный нормализованный набор:

```text
planned_candidate
optimized
already_optimal
skipped_by_policy       # решение самого auto-tzim
skipped_by_tzim         # generic, без классификации по note
skipped_unstable
hardlink_skip           # решение самого auto-tzim
failed
interrupted
incompatible_tzim
update_required
```

Нормализованный статус всегда имеет поле `origin = auto_tzim | tzim`. Сырые `status`, `note`, `error`, exit code и версия `tzim` сохраняются отдельно. Нельзя превращать `skipped_by_tzim` в `skipped_refused`, `unsupported` или другой точный reason путём анализа свободного текста. Более детальная классификация возможна только после появления structured reason codes.

### 7.4 Retry и история

- `already_optimal`, `skipped_by_tzim` и `skipped_by_policy` не повторяются при неизменной file identity, binary identity, `tzim` version и policy.
- `failed` не повторяется автоматически при каждом scheduler run; повтор — manual или после изменения identity/policy.
- Запись history содержит root, relative path, device/inode, size, mtime_ns, SHA-256 до/после (если вычислены), status/reason, timestamps, argv без секретов и binary identity.
- SQLite transactions не оставляют файл одновременно `running` после завершения процесса; stale runs помечаются `interrupted`, а не `optimized`.

### 7.5 Read-only plan и отличие от `tzim --dry-run`

`auto-tzim scan` и `auto-tzim run --once --dry-run` выполняют только discovery, policy checks и построение candidate plan/argv:

- file-processing процесс `tzim` не запускается и target paths ему не передаются; допускается только отдельный `tzim --version` для adapter;
- `tzim --dry-run` не вызывается; candidate, temporary files и другие writes внутри target roots не создаются;
- запись собственного plan/state/log вне target roots допускается и документируется;
- read-only target filesystem достаточен для preview;
- результат означает «кандидат по локальной политике», а не предсказание eligibility, refusal или gain.

Перед первым apply сохраняется reviewed plan ID/hash вместе с root, policy и binary identity. Apply без подходящего предварительного плана отклоняется кодом 6. Изменение root/policy/binary требует нового plan.

Реальный `tzim --dry-run` нельзя называть read-only: он выполняет pipeline, создаёт и верифицирует temporary candidate в target folder, требует write/free space и после SIGKILL/panic может оставить orphan temporary file. MVP его не использует. Если позже появится отдельная команда полной проверки, она должна называться не `dry-run`, требовать отдельного consent и явно документировать temporary writes и crash cleanup.

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
auto-tzim run --once [--folder ID] (--dry-run | --apply)
auto-tzim status
auto-tzim history [--folder ID] [--status STATUS]
auto-tzim logs [--folder ID]
auto-tzim pause
auto-tzim resume
auto-tzim update-check
```

`folder add` не запускает обработку. `run --once` без режима — usage error (2) и ничего не обрабатывает. `--dry-run` является read-only planner alias; `--apply` обязателен для любого ручного изменения файлов. Для новой папки apply без ранее сохранённого подходящего plan ID/hash отклоняется кодом 6.

Все команды поддерживают `--json`; JSON идёт только в stdout, diagnostics — в stderr. В non-TTY нет prompts или ожидания stdin.

### 8.2 Exit codes и precedence

`auto-tzim` имеет собственный контракт exit codes и **никогда не возвращает raw exit code `tzim` как свой**. Код результата `tzim` сохраняется в history. Code 4 намеренно не используется `auto-tzim`, чтобы не смешивать его с verification failure `tzim`.

| Code | Значение |
| --- | --- |
| 0 | Команда выполнена; optimized/already_optimal/документированные skips и пустой candidate plan допустимы |
| 1 | Run завершён, но один или несколько файлов имеют `failed` |
| 2 | Usage/config error, включая отсутствующий `--dry-run`/`--apply` и непустой `transforms` в MVP |
| 3 | Несовместимый/недоступный `tzim` или unsupported version/JSON contract |
| 5 | Ошибка update check/update; доступность новой версии сама по себе не ошибка |
| 6 | Preflight/policy refusal до обработки: first-run plan gate, pause, insufficient free space, apply не разрешён |
| 7 | Внутренняя ошибка `auto-tzim`, включая неустранимый state/storage failure |
| 8 | Уже выполняется другой run или получение lock невозможно |
| 130 | Выполнение прервано SIGINT |
| 143 | Выполнение завершено обработанным SIGTERM |

Preflight выполняется в фиксированном порядке: usage/config (2) → lock (8) → `tzim` compatibility (3) → policy/resources (6) → выполнение. Update commands используют 5 для своей операционной ошибки. Если после начала выполнения записано несколько итогов, process exit выбирается по precedence:

```text
130/143 > 7 > 2 > 8 > 3 > 6 > 5 > 1 > 0
```

Первый preflight failure завершает команду и не требует вычислять остальные стадии; все дополнительные diagnostics остаются в log/history. Dry-run возвращает 0 при успешном построении плана, не запускает file-processing `tzim`, не пишет в target roots и не записывает history как будто apply произошёл.

## 9. Планировщик — v0.2

- Используются launchd user agent и systemd user timer, без system daemon.
- По умолчанию schedule выключен. Включение требует предварительного read-only plan и отдельного apply consent; смена root/policy/binary identity инвалидирует consent и ставит schedule на pause до нового review.
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
- просматривать read-only candidate plan и точные argv/policy до apply, с явной пометкой, что eligibility/gain ещё не известны;
- запускать/останавливать run и ставить scheduler на pause;
- видеть текущий файл, счётчики статусов, ошибки и историю;
- просматривать update notification, не применяя обновление неявно.

Опасные изменения (cloud-sync, scheduled update, а в будущем — `transforms`) требуют отдельного экрана подтверждения с default **No**. В MVP `transforms` не предлагается вообще.

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
- После обновления: повторные version/adapter checks, binary hash, `doctor` и read-only `scan` smoke test; `tzim --dry-run` для этого не используется.
- Ошибка update не запускает compression и оставляет понятный log.
- Automatic rollback не заявляется, пока он реально не реализован и проверен.
- `update_policy=scheduled` включается только отдельным пользовательским opt-in после успешного ручного update.
- Обновление `auto-tzim` этим механизмом не выполняется.

### 11.3 Acceptance update test

В изолированном тестовом окружении устанавливается предыдущая Release-версия, затем выполняется обновление тем же способом, который получит пользователь. Проверяются фактическая смена версии, запуск binary и совместимость. Проверки только formula на GitHub недостаточно.

## 12. Qt GUI — v0.4

- Нативный выбор папок и профилей.
- Read-only candidate plan до apply, без обещания фактической eligibility/gain.
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
- argv construction, generic status mapping без анализа `note` и запрет непустого `transforms` в MVP.
- SQLite recovery, lock и signal transitions.
- Все exit codes, fixed preflight order и signal/error precedence.
- Update version comparison и несовместимые schemas.

### 14.2 Integration с fake `tzim`

Fake binary должен моделировать:

- все JSON statuses и malformed/partial output;
- nonzero exits, timeout, hang и попытку чтения stdin/prompt;
- version/capabilities mismatch;
- unexpected file mutation и ошибки создания temporary output;
- файлы с любыми допустимыми именами.

Проверяется, что `auto-tzim` не использует shell/PTY, не отвечает на prompt и не объявляет success без валидного JSON/exit contract. Отдельный fake-binary invocation log доказывает, что scan/dry-run на read-only target root не вызывает ничего, кроме необязательного `tzim --version`: target paths и `--dry-run` никогда не передаются. Тесты также доказывают отсутствие passthrough raw `tzim` exit code и корректный код 6 для каждого preflight refusal.

### 14.3 Integration с реальным `tzim`

- Только temporary test roots и маленькие разрешённые fixtures.
- Default mode без `--formats`: фактические optimized/already_optimal/skipped_by_tzim cases без предположения о полном default set.
- Confirmation-required transformations: непустой `transforms` отклоняется в MVP, а никакой запуск не ожидает и не эмулирует prompt.
- Scan/dry-run на read-only target root успешно строит candidate plan без file-processing вызова `tzim`, без передачи `--dry-run` и без temporary files.
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

1. Пользователь может добавить узкую папку, выполнить read-only scan/dry-run и увидеть точный детерминированный candidate plan/argv без утверждения о будущем eligibility/gain.
2. Scan/dry-run не запускает file-processing `tzim` и не передаёт `--dry-run`; допустим только `tzim --version`. План работает на read-only target root и не создаёт там temporary files; собственные state/log writes вне root документированы.
3. Первый `--apply` без подходящего предварительного plan ID/hash возвращает 6 и не изменяет target files.
4. `run --once --apply` вызывает официальный `tzim` только с вычисленными разрешёнными параметрами; bare `run --once` возвращает 2.
5. Ни один unknown/unsupported/refused/failed/unstable/hard-linked/symlink файл не изменяется.
6. Нет shell/PTY invocation, prompts, root requirement или скрытой фоновой активности.
7. MVP не перечисляет недоступный default set, не парсит human capabilities/note и отклоняет непустой `transforms`.
8. Повторный запуск не создаёт бесконечный retry loop.
9. Config/history/log recovery работает после отмены и crash simulation.
10. JSON/CLI exit codes, включая 6/7/8/130/143 и precedence, проверены реальными командами; raw `tzim` codes не пробрасываются.
11. Update check только уведомляет и ничего не заменяет.
12. Документация содержит установку, первый read-only plan, отличие от пишущего `tzim --dry-run`, temporary writes при реальном apply, риски, отсутствие undo, способы отключить обработку и полный список ограничений.

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
3. Реализовать безопасный discovery, read-only candidate planner без вызова `tzim --dry-run` и state/history.
4. Реализовать restricted version adapter и per-file JSON subprocess runner только для явно разрешённого apply.
5. Добавить CLI из §8.1 и update-check notification.
6. Покрыть fake/real `tzim` integration tests на macOS/Linux.
7. Подготовить README с первым read-only plan, отличием от `tzim --dry-run` и ограничениями.

Не реализовывать в этом задании scheduler daemon, TUI, Qt, automatic update, backup, cloud sync, собственные codecs или изменения в репозитории `tzim`.
