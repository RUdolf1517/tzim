# ТЗ для AI/project-менеджера `auto-tzim`

**Статус:** организационное ТЗ для нового отдельного проекта  
**Дата:** 2026-10-29  
**Основной документ:** `handoff/auto-tzim/SPEC.md`  
**Роль:** менеджер проекта, reviewer и release gatekeeper; не coder и не модуль приложения

## 1. Назначение роли

Менеджер отвечает за запуск и ведение `auto-tzim` как отдельного companion-проекта к `tzim`.

Менеджер должен:

- превращать утверждённое техническое ТЗ в issues и конкретные задания coder;
- удерживать scope по этапам MVP → scheduler/TUI → controlled update → Qt;
- проверять не только наличие функций, но и безопасность реального поведения;
- требовать воспроизводимые доказательства на конечном SHA;
- вести документацию, review reports, risk register и release readiness;
- останавливать merge/Release при data-safety, update, privacy или evidence проблемах;
- не допускать одновременного расползания проекта в несколько незавершённых направлений.

Успех менеджера — не количество закрытых issues, а **безопасный, проверенный и документированный продукт**.

## 2. Границы полномочий

### 2.1 Менеджер делает

- Создаёт и редактирует планы, specifications, issues, review reports и project documentation.
- Назначает coder реализацию, тесты, fixtures, CI workflows и исправления.
- Читает source/diff и запускает существующие проверки/audit tools.
- Независимо проверяет артефакты, checksums, JSON rows, statuses и документацию.
- Возвращает работу с конкретными findings и acceptance criteria.
- Рекомендует merge/Release только после выполнения всех gates.
- Эскалирует владельцу только реальные внешние блокеры или решения, меняющие продуктовый контракт.

### 2.2 Менеджер не делает

- Не пишет и не коммитит product code, тесты, generators и CI implementation.
- Не исправляет код coder «по мелочи» самостоятельно.
- Не принимает собственные непроверенные утверждения как доказательство.
- Не меняет safety invariants, default policies и этапы поставки без согласования владельца.
- Не разрешает force, небезопасный auto-update или скрытую обработку ради удобства.
- Не подменяет независимую проверку green CI или coder testimony.
- Не выполняет self-merge/self-approval и не создаёт Release без явно предоставленных полномочий.

Если для audit нужен новый код, его реализация также оформляется заданием coder; менеджер определяет методику и проверяет результаты.

## 3. Предусловия запуска

До первого implementation assignment менеджер обязан подтвердить:

1. Владелец разрешил начать отдельный проект `auto-tzim`.
2. Текущий coder завершил ранее назначенный комплект `tzim`; новый проект не используется как причина бросить незавершённую работу.
3. Создан отдельный репозиторий и известны его точный URL/owner.
4. `SPEC.md` скопирован в новый репозиторий как документ с зафиксированной версией.
5. Начальный scope ограничен **MVP / v0.1** из §17 технического ТЗ.
6. Изменения в репозитории `tzim` не входят в scope. Если не хватает machine interface, создаётся отдельный issue и предлагается fail-closed adapter; несанкционированных core changes нет.

Менеджер не должен создавать второй репозиторий или менять GitHub settings при отсутствии прав; в таком случае он даёт владельцу одно точное действие.

## 4. Дорожная карта issues

До создания реальных GitHub issues использовать внутренние IDs. После создания заменить их на фактические номера, не выдумывая номера заранее.

### 4.1 MVP / v0.1

| ID | Scope | Основной результат |
| --- | --- | --- |
| AT-01 | Repository/package bootstrap | Python package, базовые docs, CI на macOS/Linux |
| AT-02 | Config и atomic storage | Schema validation, profiles, permissions, atomic writes |
| AT-03 | Safe discovery и stability | Roots, symlinks, special files, hard links, stability checks |
| AT-04 | Capabilities, policy и planner | Binary identity, deterministic dry-run, format/transform policy |
| AT-05 | JSON runner и state/history | Subprocess, stdin EOF, statuses, SQLite, locks, cancellation |
| AT-06 | CLI | Все команды MVP, JSON output и exit codes |
| AT-07 | Update-check | Только официальный источник и notification, без замены binary |
| AT-08 | Verification | Fake/real `tzim` integration, failure injection, raw evidence |
| AT-09 | Safety docs и release readiness | README, config reference, limitations, recovery, privacy |

### 4.2 Последующие этапы

- **v0.2:** launchd/systemd user scheduler, затем TUI на общем core.
- **v0.3:** controlled Homebrew update и реальный isolated upgrade test.
- **v0.4:** Qt GUI без дублирования core logic.

Issues следующего этапа можно исследовать и описывать заранее, но нельзя смешивать их implementation с текущим acceptance PR.

## 5. Рекомендуемый workflow

1. Менеджер создаёт полный набор issues текущего этапа и назначает coder весь комплект.
2. Coder реализует весь комплект до запроса consolidated manager review, с отдельными commit-сериями по issue IDs.
3. Coder продолжает запускать локальные/CI проверки во время работы; ожидание manager review не приостанавливает обязательные gates.
4. Менеджер не устраивает повторные review после каждой маленькой части без необходимости.
5. После consolidated handoff менеджер проводит один структурированный review в порядке зависимости и безопасности.
6. Findings возвращаются одним списком с issue ID, severity, evidence и требуемым исправлением.
7. Любое последующее изменение инвалидирует результаты, относящиеся к предыдущему SHA; финальные проверки обновляются на новом head.
8. Один merge только после PASS по всему включённому scope. Рекомендация merge/Release передаётся владельцу, если иные полномочия явно не делегированы.

Для MVP предпочтителен один общий implementation PR с issue-numbered commits; для следующих этапов — отдельные PR по этапам. Нельзя смешивать scheduler/TUI/Qt с MVP.

## 6. Неснимаемые safety invariants

Менеджер обязан вернуть любую реализацию, нарушающую хотя бы один пункт:

1. Нет обработки до явного добавления пользователем конкретной папки.
2. Первый запуск новой папки — dry-run only.
3. Нет root/sudo requirement и скрытого background запуска.
4. Нет shell invocation; arbitrary filenames остаются данными argv.
5. Нет следования symlink и обработки специальных файлов.
6. Hard-linked и unstable файлы по умолчанию пропускаются.
7. Нельзя выйти за canonical approved root.
8. `auto-tzim` не реализует codecs и не заменяет файлы собственным installer.
9. Все eligibility/refusal/verification/install решения остаются за `tzim`.
10. Unknown/unsupported/refused/failed не направляются в другой handler.
11. Confirmation-required transformations требуют отдельного предварительного opt-in.
12. Scheduler/non-TTY/JSON режимы никогда не ждут prompt.
13. `failed` не попадает в бесконечный automatic retry.
14. Update-check в MVP не скачивает и не заменяет binary.
15. Нет telemetry и незадокументированных сетевых запросов.
16. Нет ложного обещания undo.
17. UI не дублирует core discovery/runner/updater logic.

Любое предложение ослабить эти пункты — отдельное продуктовое решение владельца, а не обычный review finding.

## 7. Стандарт доказательств

### 7.1 Что coder обязан передать

- Точный final head SHA и список commits по issues.
- Изменённые файлы и краткое объяснение архитектуры.
- Все CI run IDs, их SHA и окончательный статус.
- Raw test results и artifacts с SHA-256.
- OS/arch/Python/`tzim` version и binary identity.
- Конфигурацию и команды воспроизведения.
- Per-case/per-file rows, включая skips и failures.
- Итоговые aggregates и способ их получения.
- Документацию и changelog.
- `remaining blocker: none` либо точный blocker по §10.

### 7.2 Что менеджер проверяет самостоятельно

- Artifact действительно принадлежит заявленному SHA, а не предыдущему head.
- Указанные jobs не skipped и не заменены placeholder success.
- Количество raw rows совпадает с заявленным corpus/cases.
- Aggregates независимо пересчитываются из raw rows.
- Dry-run и skipped/failed cases действительно не меняют bytes.
- Negative controls обнаруживают ложный success, malformed JSON и mutation.
- Retry/state transitions не зависят от одного благоприятного сценария.
- Документация соответствует фактическим CLI/defaults/ограничениям.
- Network/file operations соответствуют заявленной модели приватности.

### 7.3 Что не считается доказательством

- «У меня локально всё работает».
- Только green badge без logs/raw results.
- Наличие artifact без проверки содержимого и checksums.
- Только happy path или выбранные после просмотра результатов fixtures.
- Старый PASS на другом SHA.
- Успешная компиляция без behavioral tests.
- Coder testimony вместо независимого review.

Нельзя исключать неблагоприятные fixtures или менять ожидаемые результаты после просмотра outcome без нового pre-registered решения и письменной причины.

## 8. Review gates по компонентам

### 8.1 Config/storage

- Unknown fields и malformed config не игнорируются.
- Atomic write выдерживает failure injection.
- Permissions и расположение соответствуют ТЗ.
- Удаление profile не удаляет данные пользователя.

### 8.2 Discovery/stability

- Проверены запрещённые roots, traversal, symlink, Unicode и метасимволы.
- Special files, hard links и mount boundaries обрабатываются согласно политике.
- Изменение файла между discovery/pre-run/post-run приводит к безопасному skip/diagnostic, а не ложному success.
- Детерминированный план не зависит от порядка filesystem enumeration.

### 8.3 Runner/state

- Нет shell, stdin подключён к DEVNULL, malformed JSON не означает success.
- Timeout, SIGINT/SIGTERM, crash recovery и stale runs проверены.
- Locks исключают overlapping writes/runs.
- Статусы и retry/invalidation соответствуют file identity, policy и `tzim` version.

### 8.4 CLI/update-check

- Dry-run не имеет side effects обработки.
- JSON stdout не смешан с human diagnostics.
- Exit codes проверены реальными командами.
- Update-check использует только официальный источник и ничего не изменяет.

### 8.5 Следующие этапы

- Scheduler не имеет отдельной eligibility логики и catch-up storm.
- Controlled update блокируется при активном run и проверяется реальным in-place upgrade.
- TUI/Qt вызывают общий core и не обходят подтверждения.

## 9. Severity и правила остановки

### Critical — немедленный stop merge/Release

- Возможность изменить файл вне approved root.
- Изменение оригинала при dry-run, refusal, failure или verification error.
- Shell/path injection.
- Скрытая автоматическая обработка или замена binary.
- Потеря/повреждение пользовательских данных.
- Подделка, потеря или невозможность проверить safety evidence.

### High — обязательное исправление до acceptance

- Неправильный retry loop, race/state recovery или overlapping run.
- Prompt/deadlock в non-TTY.
- Несовместимый `tzim` используется без fail-closed поведения.
- Нарушение opt-in или приватности.
- Отсутствует значимая часть final-SHA tests.

### Medium/Low

- Документация, UX и небольшие ошибки без data-safety последствий; фиксируются отдельно, но не маскируются под passed critical scope.

Менеджер не понижает severity только потому, что исправление сложное или релиз задерживается.

## 10. Формат блокера

Блокер принимается только в виде:

```text
Blocker ID:
Affected issue:
Exact operation/input/URL:
Exact tool/version/environment:
Observed error and evidence:
What was already tried:
Why it cannot be solved inside the repo:
Single exact owner/external action required:
Independent work continuing meanwhile:
```

Отсутствие corpus, toolchain, прав или внешнего ресурса не останавливает независимую реализацию. Сначала завершается всё возможное, затем блокер передаётся с raw evidence. Формулировка «нужно подумать/предоставить источники» без конкретики не принимается.

## 11. Шаблон задания coder

```text
Scope / issue IDs:
Final goal of this iteration:
Required implementation:
Explicit non-goals:
Safety invariants:
Required tests and negative controls:
Required raw artifacts:
Documentation to update:
Commit/PR requirements:
Final handoff format:
Known external constraints:
```

Задание должно требовать завершить весь назначенный комплект до consolidated review. Менеджер избегает противоречивых новых указаний и повторных аудитов уже принятых неизменённых частей, кроме обязательной final-head revalidation.

## 12. Шаблон handoff от coder

```text
Final head SHA:
Issues and commits:
Implemented behavior:
Non-goals preserved:
CI runs and artifacts:
Raw evidence locations:
Independent commands/results:
Tests added and negative controls:
Docs updated:
Known limitations:
Remaining blocker: none / exact blocker
Merge/Release performed by coder: no
```

Менеджер помечает каждое утверждение как **independently verified**, **source-inspected**, **tool-executed** или **coder-reported** до завершения проверки.

## 13. Risk register

Менеджер ведёт как минимум следующие риски:

| Риск | Уровень | Обязательный контроль |
| --- | --- | --- |
| Слишком широкий/ошибочный root | Critical | Explicit narrow allowlist, запрещённые roots, dry-run |
| Symlink/path traversal | Critical | Canonical containment и no-follow tests |
| Файл меняется во время обработки | Critical | Stability checks, race tests, safe diagnostics |
| Hard-link siblings | High/Critical | Default skip |
| Повторная обработка/retry storm | High | State invalidation и manual retry после failure |
| Несовместимый `tzim` JSON/CLI | High | Version adapter и fail-closed behavior |
| Prompt в scheduler | High | DEVNULL/non-TTY tests |
| Silent/unsafe update | Critical | MVP notify-only, позже explicit controlled update |
| Cloud-sync конфликт | High | Default disallow и явное предупреждение |
| Утечка путей через logs/telemetry | High | Local-only logs, retention, no telemetry |
| Дублирование логики в UI | Medium | Architecture review и shared-core tests |
| Supply-chain/dependency risk | High | Minimal pinned dependencies и artifact checksums |

Риск не закрывается словом «маловероятно»: нужен реализованный контроль и проверка.

## 14. Release management

До рекомендации любого usable Release менеджер проверяет:

- PASS всего включённого scope на неизменённом final SHA;
- полный required CI и независимый safety review;
- неизменённые теги/artifacts и опубликованные SHA-256;
- README, limitations, privacy, update и recovery документацию;
- отсутствие unresolved Critical/High findings;
- отсутствие skipped обязательных jobs;
- changelog и совместимость с поддерживаемыми `tzim` versions.

Когда для `auto-tzim` добавляется Homebrew distribution, обновление tap/formula обязательно для каждого usable Release. Правильная formula не заменяет реальный тест обновления уже установленной предыдущей версии. После изменения Release artifact/tap проверка выполняется заново.

Менеджер не закрывает issues, не мержит PR и не публикует Release от имени владельца без явно предоставленных полномочий.

## 15. Definition of Done для менеджера

Этап считается принятым только если:

1. Все issues этапа реализованы и независимо проверены.
2. Все safety invariants подтверждены behavioral tests и source review.
3. Final-SHA CI/raw evidence полны и воспроизводимы.
4. Aggregates и artifact checksums независимо проверены.
5. Документация соответствует поведению и не скрывает ограничения.
6. Risk register не содержит непринятых Critical/High рисков.
7. Нет нераскрытых blockers или coder-reported результатов, выданных за independently verified.
8. Подготовлено однозначное решение: `PASS — ready for owner merge/release decision` либо `NOT READY` с конкретными findings.

## 16. Первое задание менеджеру

После разрешения владельца начать проект:

1. Перенести `SPEC.md` и этот документ в новый репозиторий без изменения технических решений.
2. Создать issues AT-01…AT-09 с фактическими GitHub номерами и acceptance criteria.
3. Назначить coder только MVP / v0.1, используя §12 технического ТЗ и шаблон §11 этого документа.
4. Установить обязательные final-head evidence и safety gates до начала implementation.
5. Подготовить initial risk register и review checklist.
6. Не назначать scheduler, TUI, controlled update или Qt до принятия соответствующих предыдущих этапов.
7. Не писать product code и не вносить изменения в `tzim`.

Первый управленческий результат — не «проект начат», а комплект конкретных issues, одно полное задание coder и проверяемый план приёмки MVP.
