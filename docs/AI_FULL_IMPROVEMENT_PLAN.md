# LORGAR MSA10W: полный план улучшения для следующих ИИ

Снимок аудита: 2026-10-13. Рабочая ветка: `main`, HEAD `6a2f3b0` (`v2.0.2`).
План относится к текущей рабочей директории вместе с незакоммиченными правками.

## 1. Цель

Довести Linux-стек мыши до состояния, в котором:

- kernel-модуль не генерирует лишний HID-трафик и не меняет настройки без явного согласия;
- батарея надёжно появляется через `power_supply` после загрузки и после reconnect;
- Rust daemon, CLI, GTK-приложение и kernel-модуль имеют один непротиворечивый контракт;
- все неизвестные байты протокола остаются неизвестными;
- установка, обновление и удаление не оставляют конфликтующих версий и сервисов;
- автоматические тесты доказывают логику, а live-проверки отдельно доказывают работу с физическим устройством.

Не переписывать проект с нуля. Сохранять существующую архитектуру и совместимые CLI-команды, пока конкретная задача не требует контролируемой миграции.

### Статус реализации на 2026-10-13

Локально реализовано и прошло build/test gates:

- единый `tools/check.sh`;
- kernel query больше не запускается от каждого input report;
- query health отделён от `input_seen`, два последовательных timeout переводят
  только state-query path в `unresponsive`;
- kernel feature-report buffer перенесён со stack в heap согласно контракту HID
  raw request;
- startup query ограничен тремя попытками, далее действует 60-second cadence;
- power-supply регистрируется только после подтверждённого state response;
- battery indicator выключен по умолчанию, не переключает DPI stage и пишет
  палитру только по явному включению и смене battery band;
- Rust state query повторяет только timeout, tray не фабрикует `0%`, не сглаживает
  значение и не спамит low-battery notification;
- GTK различает наличие receiver, свежий state response, stale response и ошибку;
- DKMS installer больше не удаляет ту же версию для всех kernels, не ставит
  пакеты и не отключает services без явного решения пользователя;
- kernel version берётся из `dkms.conf` и сверяется при build/live verification.

Не закрыто без следующего live-сеанса с root: загрузка `2.3.4`, сравнение
kernel/HIDAPI usbmon, `c600`/`c601`, 15-minute cadence, unplug/replug,
suspend/resume и reboot/DKMS proof. Разделы E1–J3 остаются roadmap, кроме явно
перечисленных выше частей. Не считать их выполненными по одному зелёному build.

## 2. Неподвижные правила

1. `protocol/msa10w.md` — источник истины. Повышать confidence только после воспроизводимого A/B/A-эксперимента или подтверждённого vendor capture.
2. Не выводить charging из PID `c600`, USB-кабеля, byte 8 или vendor events. Charging остаётся `Unknown`, пока не появится независимое доказательство.
3. Не отправлять неизвестные feature/output reports. Raw-команды допустимы только в явном debug-режиме после показа точных байтов пользователю.
4. Не считать успешный USB write подтверждением применения настройки. Это подтверждает только передачу в transport.
5. Не выбирать устройство только по VID/PID. Физический config interface: USB interface 2, usage page `0xff10`; vendor events: interface 1, usage page `0xff11`.
6. Не считать зелёные unit-тесты live-доказательством. Разделять build, mock, boot, HID capture, sysfs/UPower и визуальную проверку.
7. Перед любой правкой перечитывать текущий `git diff`: рабочее дерево уже содержит пользовательские изменения.
8. Не делать live setter-тесты без заранее записанного исходного состояния и плана восстановления.
9. Никаких destructive cleanup, массового удаления DKMS или замены системных файлов без точной проверки целей.
10. Один этап — один логический коммит. Не смешивать kernel transport, UI и packaging.

## 3. Текущее состояние и доказательства

### 3.1 Незакоммиченные изменения

На момент аудита изменены:

- `crates/lorgar-app/src/main.rs`;
- `crates/lorgar-core/src/lib.rs`;
- `crates/lorgar-core/src/protocol.rs`;
- `crates/lorgar-core/src/transport.rs`;
- `crates/lorgar-core/tests/reliability.rs`;
- `kernel/README.md`, `kernel/dkms.conf`, `kernel/hid-lorgar-protocol.h`;
- `kernel/hid-lorgar.c`, `kernel/reload.sh`, `kernel/tests/protocol_test.c`;
- `kernel/verify-live.sh`, `protocol/msa10w.md`;
- новый `tools/probe-state.sh`.

Следующий ИИ не должен откатывать или переформатировать их целиком.

### 3.2 Что уже проходит

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`;
- `cargo test --workspace --locked`: 48 успешных тестов, один display-test пропущен штатно;
- `xvfb-run -a cargo test -p lorgar-app ui_smoke --locked -- --ignored`: 1 успешный GTK smoke-test;
- `cargo build --workspace --release --locked`;
- `make -C kernel check`: shell syntax, kernel build с `W=1`, C protocol tests, `modinfo`;
- `desktop-file-validate`;
- `udevadm verify` для обеих rules.

`systemd-analyze verify` в текущей машине останавливается на отсутствии установленного `%h/.local/bin/lorgard`; это не доказанный синтаксический дефект unit-файла. В CI unit следует проверять в подготовленном staging root.

### 3.3 Что сейчас не работает live

Read-only проверка показала:

- built module: `2.3.3`;
- loaded module: `2.3.3`;
- installed module: `2.3.3`;
- DKMS `2.3.3` установлен для текущего и LTS kernel;
- физические interfaces `c600` привязаны к `hid-lorgar`;
- config interface: `/dev/hidraw3`;
- `hardware_status=unresponsive`;
- `dpi`, `dpi_stage`, `polling_rate`, `led_mode`, `led_color` возвращают `ENODATA`;
- `battery_indicator=1`;
- `/sys/class/power_supply/lorgar_mouse_battery_*` отсутствует;
- `make -C kernel live-check` падает: `Expected one LORGAR power supply, found 0`;
- daemon не запущен, session type — Wayland.

Kernel journal показывает повторяющиеся `state query timed out after 2000 ms` каждые 2–5 секунд во время активности, хотя штатный интервал заявлен как 60 секунд.

## 4. Подтверждённые проблемы

### P0-1. Query storm после первого `unresponsive`

Файлы: `kernel/hid-lorgar.c`, прежде всего `lorgar_raw_event`, `lorgar_battery_work`, `lorgar_note_timeout`.

Текущая цепочка:

1. query таймаутится и ставит link `UNRESPONSIVE`;
2. любой input на interface 0 считается physical activity и ставит link `ONLINE`;
3. код сбрасывает `next_query` в `jiffies` и вызывает `lorgar_kick_query`;
4. kernel query снова таймаутится через две секунды;
5. следующий mouse input повторяет цикл.

Дополнительно `query_timeouts` не сбрасывается физической активностью, поэтому после достижения порога каждый следующий query снова переводит состояние в `UNRESPONSIVE` после одного timeout.

Результат: активное движение мыши вызывает непрерывные feature requests, log spam и потенциально усиливает firmware rate-limit.

### P0-2. Kernel query transport всё ещё не получает ответ

Файл: `kernel/hid-lorgar.c`, `lorgar_send_feature`.

Kernel вызывает `hid_hw_raw_request` с report ID `0x00`, buffer length 9 и `HID_FEATURE_REPORT`. HIDAPI успешно отправляет тот же логический 8-byte payload с синтетическим leading zero, но kernel-originated запрос не приводит к 64-byte response. Parser менять до доказательства transport contract нельзя.

Нужны сравнительные traces фактических USB control transfers, а не ещё одна догадка про state prefix.

### P0-3. Battery indicator опасен по умолчанию

Файлы: `kernel/hid-lorgar.c`, `kernel/hid-lorgar-protocol.h`, `kernel/README.md`.

Сейчас feature включена по умолчанию и:

- перезаписывает цвета всех восьми DPI-стадий;
- каждые 10 секунд выбирает соседнюю DPI-стадию;
- через cooldown пытается вернуть исходную стадию;
- при ошибке restore оставляет реальное состояние неопределённым.

Это фоновое изменение hardware state, а не пассивная battery integration. Оно может кратко менять sensitivity, конфликтовать с игрой, уничтожать пользовательскую палитру и взаимодействовать с profile apply. Текст README о том, что stage «никогда не переключается молча», противоречит реализации.

### P0-4. Переход в `unresponsive` не уведомляется

Файл: `kernel/hid-lorgar.c`, `lorgar_battery_work`.

После `lorgar_note_timeout` переменная `offline` уже true. Условие `if (link_changed && !offline)` поэтому не выполняется именно в момент перехода в `UNRESPONSIVE`. Sysfs/uevent/power-supply consumers могут не получить событие изменения.

### P0-5. Live verifier не проверяет причину отсутствия battery

Файл: `kernel/verify-live.sh`.

Скрипт только сообщает, что power-supply не найден. Он должен до выхода собрать безопасную диагностику: loaded/installed/built version, bound physical interfaces, config interface, `hardware_status`, последние kernel messages, наличие response и query counters. Никаких setter writes.

## 5. Последовательность реализации

Этапы ниже обязательны по порядку. Параллельно можно делать только независимую документацию или CI, не меняющую те же файлы.

## Этап A. Зафиксировать воспроизводимый baseline

### A1. Сохранить состояние без изменения мыши

- Записать `git status --short`, HEAD и SHA-256 всех изменённых source-файлов.
- Записать `uname -r`, `rustc -Vv`, built/loaded/installed/DKMS versions.
- Записать физические HID paths, USB parent, interface number, usage page и текущий driver.
- Сохранить read-only sysfs snapshot и ограниченный kernel log.
- Не запускать `battery`, `capture --query-state`, reload или sysfs setters на этом шаге.

Готово, когда snapshot позволяет доказать, какая версия реально тестировалась.

### A2. Добавить единый локальный gate script

Создать `tools/check.sh`, который последовательно запускает:

- fmt check;
- strict clippy;
- workspace tests;
- GTK smoke при наличии `xvfb-run`;
- release build;
- kernel check при наличии headers;
- desktop/udev validation;
- shell syntax;
- `git diff --check`.

Скрипт не должен загружать module, обращаться к hardware или требовать root. Optional tools должны давать явный `SKIP`, не ложный `PASS`.

## Этап B. Починить kernel state machine до transport-экспериментов

### B1. Извлечь чистую state-machine логику

Вынести решения «когда query», «как считать consecutive timeouts», «какой link transition» в функции, тестируемые host C tests или KUnit. Не переносить USB I/O в helper header.

Обязательные таблицы тестов:

- boot → query → response;
- boot → timeout ×3 → backoff;
- online → одиночный timeout → online/stale;
- online → timeout threshold → unresponsive;
- unresponsive → physical input без немедленного query storm;
- unresponsive → valid state response → online и reset counter;
- periodic 60-second cadence;
- userspace-caused response postpones kernel query;
- jiffies wraparound;
- disconnect/remove while work pending.

### B2. Убрать feedback loop

Требуемое поведение:

- physical input доказывает, что HID input path жив, но не доказывает battery query response;
- input не должен каждый раз обнулять 60-second backoff;
- после `UNRESPONSIVE` разрешён максимум один bounded recovery query по отдельному cooldown, а не query на каждое движение;
- consecutive query timeout counter сбрасывается только валидным state response;
- log warning rate ограничен независимо от mouse report rate.

Предпочтительная модель: отдельно хранить `input_seen`/`last_input`, `state_response_seen`/`last_state_response` и `query_health`. Не перегружать один `link_state` тремя разными фактами.

### B3. Исправить notifications

- Уведомлять ровно один раз при каждом реальном state transition.
- Не вызывать `power_supply_changed` без изменения видимого battery state.
- Определить политику stale capacity: либо оставить последнее значение с отдельным stale/health атрибутом, либо после документированного срока сделать `PRESENT=0`. Не смешивать эти варианты.
- Добавить тест на переход `ONLINE → UNRESPONSIVE` и обратный переход.

Готово, когда host tests показывают bounded query rate, а code review не находит пути «input report → немедленный query» без cooldown.

## Этап C. Доказать правильный kernel HID transport

### C1. Сравнить HIDAPI и kernel на одном устройстве

Сделать контролируемый эксперимент:

1. остановить daemon/tray/UPower legacy clients;
2. сохранить исходные versions и bindings;
3. снять usbmon trace одного успешного HIDAPI `0x82`;
4. снять usbmon trace одного kernel `0x82`;
5. сравнить `bmRequestType`, `bRequest`, `wValue`, `wIndex`, report ID и `wLength`;
6. проверить descriptor для config interface;
7. менять только один transport параметр за эксперимент;
8. после каждого опыта ограничивать число queries и ждать cooldown.

### C2. Проверить допустимые kernel API варианты

Исследовать по фактическому kernel HID API и trace:

- нужен ли buffer 8 или 9 bytes;
- входит ли report ID в `len` для unnumbered feature report;
- должен ли `reportnum` быть `0` при payload без synthetic byte;
- корректнее ли `hid_hw_raw_request`, `hid_hw_output_report` или доступ через parsed report field;
- не перехватывает ли custom driver input response до `raw_event`;
- требуется ли PM wake/runtime-PM reference.

Не коммитить перебор вариантов. В коммит идёт только один подтверждённый вариант и regression documentation.

### C3. Acceptance live

- После чистого bind без userspace query power-supply появляется bounded-временем.
- Journal показывает не более трёх startup attempts и затем 60-second cadence.
- `hardware_status=online` только после valid 64-byte response или отдельно документированного input-health статуса.
- `make live-check` проходит без помощи `lorgarctl battery`.
- Повторить отдельно для receiver `c601` и cable `c600`.
- Проверить unplug/replug и один suspend/resume.

## Этап D. Сделать battery indicator безопасной отдельной feature

### D1. Немедленная политика

- Default должен быть `off`.
- Убрать periodic active-stage pulse из kernel-модуля.
- Не перезаписывать все восемь цветов без явной команды пользователя.
- Не включать feature автоматически при module load, response или reconnect.

### D2. Выбрать безопасный UX

Предпочтительный вариант:

- kernel остаётся только battery/provider driver;
- optional userspace action явно применяет battery palette;
- UI показывает точный список изменяемых стадий и предупреждает, что прежние stage colors нельзя прочитать/восстановить;
- update выполняется только при переходе между battery bands, не по таймеру;
- никакого stage switching для «пульса».

Если kernel sysfs feature сохраняется, назвать её честно, например `battery_palette_apply`, сделать one-shot write-only и не изображать readback.

### D3. Тесты

- Unit vectors для `0x27` остаются.
- Test доказывает отсутствие автоматических `0x21` и `0x27` writes после probe/state response.
- Live test применяет palette только после подтверждения пользователя и фиксирует визуальный результат.
- После reload/reboot пользовательская палитра не меняется сама.

## Этап E. Согласовать Rust transport и monitoring

### E1. Query policy

Файлы: `crates/lorgar-core/src/transport.rs`, `monitor.rs`.

- Не держать device-specific retry constants внутри generic transport.
- Создать `QueryPolicy { attempts, per_attempt_timeout, retry_delay, total_deadline }`.
- Ограничивать весь query одной total deadline.
- Добавить jitter только daemon periodic scheduling, не unit tests.
- Не повторять query при disconnect, permission error, invalid packet или parser error.
- Сохранять unrelated reports, но ограничить queue по bytes и count; при overflow выдавать отдельную ошибку/metric.
- Добавить tests с fake clock, без реальных `sleep(500 ms)`.

### E2. Разделить факты состояния

В `CachedState` добавить совместимо-версионированную модель:

- receiver/config interface present;
- physical input recently observed;
- state response freshness;
- battery value + observed timestamp + stale flag;
- last query error отдельно от disconnect error;
- last successfully requested profile отдельно от hardware readback.

Не называть `device.is_some()` полноценным «Connected to mouse».

### E3. Multi-device foundation

- Заменить глобальный `device.lock` на lock стабильной device identity.
- Identity строить из serial, а при пустом serial — из USB topology + VID/PID с явным caveat.
- `--device` должен принимать path и в будущем identity.
- Daemon config должен хранить выбранное устройство.
- Тестировать два config interfaces и virtual UHID с тем же VID/PID.

### E4. Parser hardening

- Property/fuzz tests для `InputPacket`, feature parse, descriptor parse, capture reader и IPC frames.
- Проверить все длины `0..=4096`, случайные bytes и malformed JSON/captures.
- Сохранить lossless raw bytes для unknown reports.
- State parser должен иметь один общий набор golden vectors для Rust и C; сгенерировать fixtures из нейтрального файла или сверять один JSON corpus двумя test runners.

## Этап F. Daemon и IPC

### F1. Устранить single-thread stalls

Сейчас daemon принимает до четырёх клиентов последовательно, каждый может занять 200 ms, а profile apply синхронно отправляет серию feature writes.

- Оставить ownership/state mutations на одном worker.
- Вынести framing клиентов в bounded worker pool или nonblocking poll loop.
- Ограничить число concurrent clients и размер очереди.
- Один stalled same-user client не должен задерживать monitor tick, battery deadline или shutdown.
- Добавить tests на 20 stalled clients, partial frames, early close и burst status requests.

### F2. Версионировать IPC содержательно

- Сохранить version 1 compatibility либо сделать явный v2 migration.
- Ввести стабильные string error codes вместо `u8`, оставив old numeric field на переходный период.
- Добавить daemon instance ID/boot ID в events, чтобы restart не маскировался только как sequence gap.
- Status должен возвращать capability flags: kernel integration, direct HID, Wayland backend, multi-device, indicator support.

### F3. Config watching

- Заменить полное чтение JSON каждую секунду на inotify с debounce или хотя бы metadata check.
- При invalid external edit продолжать работать на last-known-good config и явно показывать ошибку.
- Не сбрасывать foreground selection из-за изменения нерелевантного extension field.

### F4. Foreground backends

- X11: сохранить строгую проверку local PID и timeout.
- KDE Wayland: отдельный документированный KWin/DBus/script backend.
- GNOME Wayland: отдельная extension/DBus integration, не выдавать её за generic Wayland API.
- Wine/Proton: определять executable по process tree/cgroup, а не только title.
- Каждая platform integration должна быть optional и отключаться без ухудшения manual profiles.

## Этап G. CLI

### G1. Убрать legacy split

`main.rs` и `stack.rs` сейчас делят dispatch на современный и legacy пути.

- Сделать один command dispatcher и единый output contract.
- Все machine-readable команды: stdout содержит только JSON/JSONL, diagnostics идут в stderr.
- Сохранить текущие aliases через clap tests.
- Вынести tray и UHID bridge за Cargo features или в отдельный binary/crate.

### G2. Исправить tray/UPower legacy поведение

- Не показывать `0%`, если initial query failed; модель должна быть `Option<u8>`.
- Не сглаживать percentage без явной продуктовой причины: это искажает подтверждённое значение и задерживает low-battery alert.
- Не повторять low-battery notification каждые 120 секунд; хранить threshold crossing и cooldown.
- При disconnect/stale обновлять tray/virtual battery в `unknown`, а не оставлять бесконечно старое значение.
- Удалить emoji из machine-facing output; short human output можно оставить совместимым.
- Чётко пометить UHID bridge deprecated, если native kernel power_supply стабилен.

### G3. Doctor 2.0

Добавить только read-only проверки:

- built/loaded/installed/DKMS version mismatch;
- driver binding каждой physical interface;
- config usage page/interface;
- daemon ownership и конфликтующие legacy services;
- sysfs `hardware_status`, battery presence/freshness;
- kernel query storm detection по counters, не через бесконечный journal scan;
- udev ACL и group/access source;
- XDG paths permissions;
- Wayland/X11 backend availability.

`doctor --json` должен иметь schema version и deterministic fields.

## Этап H. GTK-приложение

### H1. Безопасно разделить большой `main.rs`

После функциональных P0/P1 fixes, с passing smoke test до и после, вынести:

- reusable widgets/layout;
- profile editor;
- application rules;
- overview/status;
- dialogs;
- model/state reducer.

Не менять одновременно layout и backend semantics.

### H2. Исправить connection semantics

- Различать service unavailable, receiver present, mouse input active, state response fresh и battery stale.
- Показывать timestamp последнего battery response.
- Не писать «Device available» только из `state.device.is_some()`.
- Показывать частичный profile apply с индексом первой failed command и предупреждением, что ранние writes могли примениться.

### H3. DPI presets

Текущие presets меняют draft корректно, но нужно:

- тестировать каждую кнопку и exact resulting profile;
- сделать layout адаптивным: три длинные кнопки не должны обрезаться на узком окне;
- явно указывать stage numbering `0–3` и выбранную active stage `0`;
- не применять preset автоматически;
- добавить keyboard focus и accessible names.

### H4. Доступность и визуальные проверки

- Keyboard-only проход всех действий.
- Label association для switches, spins, dropdowns и color input.
- High contrast, scale factors 1×/1.25×/2×, минимум 1024×600.
- Скриншоты overview/editor/profiles/offline/error/dirty-close.
- Реальная product photo остаётся default; experimental 3D не выдаётся за точную модель.
- Рассмотреть compile-time feature для 3D, чтобы headless/packaging build не тащил mesh/renderer без необходимости.

## Этап I. Packaging, services и installation lifecycle

### I1. Единая версия продукта

Сейчас Rust workspace `0.1.0`, git tag `v2.0.2`, kernel `2.3.3`, plasmoid `1.0`. Это допустимые разные компоненты, но должно быть явно описано.

- Завести release manifest с versions компонентов.
- Генерировать/проверять DKMS, `MODULE_VERSION`, verifier expected version и docs из одного kernel version source.
- CI должен падать при рассинхронизации.

### I2. Установщик

- Не устанавливать package manager dependencies автоматически внутри project script; вывести команду пользователю или использовать distro package.
- Перед DKMS remove проверять точные module/version/kernel targets.
- Не отключать unrelated system/user services без explicit migration flag и отчёта.
- Делать staged install, затем verify, затем switch.
- Добавить idempotent uninstall с сохранением пользовательских profiles.
- Проверить Secure Boot/module signing и сообщать понятную причину load failure.

### I3. Systemd hardening

Для user daemon проверить применимость:

- `ProtectSystem=strict` с точными writable paths;
- `PrivateTmp=true`;
- `RestrictAddressFamilies=AF_UNIX`;
- `RestrictNamespaces=true`;
- `LockPersonality=true`;
- `MemoryDenyWriteExecute=true`, если GTK/DBus dependencies не конфликтуют;
- restart rate limits;
- conditions/asserts для binary/device только там, где они не ломают offline profile editing.

Legacy tray, battery timer и UHID service не должны одновременно владеть HID с `lorgard`.

### I4. Distro packages

Подготовить сначала Arch PKGBUILD, затем Debian packaging:

- Rust binaries;
- desktop file и icon;
- user service;
- udev rules;
- optional DKMS split package;
- optional plasmoid;
- completions/man pages;
- checksums и reproducible build notes.

## Этап J. CI, security и dependency hygiene

### J1. CI minimum

- fmt;
- clippy `-D warnings`;
- tests;
- GTK Xvfb smoke;
- release build locked;
- kernel host protocol tests;
- kernel module build на минимум двух supported header versions;
- udev/desktop/systemd validation в staging root;
- shellcheck;
- cargo audit/deny;
- artifact checksums and SBOM on release.

### J2. Dependency cleanup

- Проверить `ksni 0.2.2`: он тянет Clap 2, старый bitflags и thiserror 1 рядом с новыми версиями.
- Если legacy tray остаётся, обновить backend; иначе feature-gate/remove из default build.
- Перенести общий `rust-version` в workspace и применить ко всем crates.
- Убрать runtime dependency `tempfile` из core, если она нужна только capture/X11 helpers; либо документировать её использование.
- Проверить advisories актуальным инструментом. В текущем окружении `cargo-audit` и `cargo-deny` отсутствовали, поэтому security audit не считается выполненным.

### J3. Fuzz/sanitizers

- `cargo-fuzz` targets: descriptor, input packet, feature packet, capture reader, IPC JSON frame.
- Address/UB sanitizer для C UHID helper и host protocol helpers.
- Miri для pure Rust core tests, исключая hidapi/GTK/FFI paths.
- Kernel static analysis: sparse и clang analyzer/smatch при доступности.

## Этап K. Protocol roadmap

### K1. Уже подтверждённое

- Сохранить `0x20`, `0x21`, `0x22`, `0x23`, `0x26`, `0x82` vectors.
- Сохранить strict state shapes и battery offset 7.
- `0x27` и output packet `0x03` использовать только в явных features, соответствующих доказанному назначению.

### K2. Ещё неизвестное

- Byte 8 state response;
- vendor event `0x0f`;
- vendor event `0x11`;
- точное назначение восьми entries output palette;
- charging state;
- readback DPI/polling/RGB;
- firmware differences `c600`/`c601`.

Для каждого исследования создать отдельную запись: hypothesis, fixed variables, exact packets, captures, physical observation, counterexample, confidence change. Не смешивать reverse engineering с production patch.

## 6. Обязательная матрица тестов перед релизом

### Build/mocks

- Clean checkout plus intended patch only.
- Debug/release, locked dependencies.
- All Rust/C/kernel gates.
- No ignored test кроме явно запущенного GTK smoke.

### Runtime без физической мыши

- Daemon disconnected status.
- Malformed/stalled IPC clients.
- Offline profile create/edit/delete.
- App starts/stops owned daemon и не убивает чужой daemon.
- Config concurrent edit detection.

### Physical receiver `c601`

- Cold bind;
- battery registration;
- 15-minute idle query cadence;
- continuous movement без query storm;
- sleep/wake;
- receiver unplug/replug;
- daemon start/stop ownership;
- physical DPI button event;
- no unexpected setting writes.

### Cable `c600`

- Re-enumeration identities;
- battery response accepted with byte 8 `0`;
- charging remains Unknown;
- cable disconnect back to `c601`;
- no duplicate power-supply;
- no stale old device nodes.

### Power lifecycle

- suspend/resume;
- module unload/reload;
- DKMS reboot into current kernel;
- boot into LTS kernel;
- failed bind rollback to `hid-generic`;
- uninstall restores normal input.

### UI

- Wayland manual controls;
- X11 auto profile switching;
- KDE/GNOME integrations только там, где реализованы;
- scale/accessibility/screenshots;
- low battery, stale battery, offline, daemon error, partial apply.

## 7. Как делить работу между ИИ

Рекомендуемый порядок владельцев:

1. `kernel-state-agent`: только этап B, без transport guesses.
2. `kernel-transport-agent`: только этап C после merge B.
3. `indicator-agent`: этап D после стабильного transport/state machine.
4. `core-agent`: этап E и shared golden corpus.
5. `daemon-agent`: этап F после нового core state contract.
6. `cli-agent`: этап G после IPC contract.
7. `gtk-agent`: этап H после stable daemon response.
8. `packaging-agent`: этап I.
9. `ci-security-agent`: этап J, синхронно обновляет gates после каждого merge.
10. `protocol-agent`: этап K отдельными экспериментальными ветками.

Запрещено двум ИИ одновременно менять `kernel/hid-lorgar.c`, `monitor.rs`, IPC schema или GTK state model. Если параллельность нужна, второй ИИ делает read-only review и отдаёт findings владельцу.

## 8. Формат задания следующему ИИ

Каждому ИИ передавать:

```text
Checkout: /home/sqrilizz/Documents/ZZZZ
Task ID: <например B2>
Scope: <точные файлы и поведение>
Do not touch: <соседние подсистемы>
Protocol truth: protocol/msa10w.md
Baseline: git HEAD + dirty files + relevant hashes
Required tests: <точные команды>
Live writes allowed: no / exact confirmed command only
Acceptance: <наблюдаемый результат>
Rollback: <как вернуть прежний module/binding/config>
Evidence required: file:line diff, test output, live output separately
```

ИИ должен сначала выдать короткий evidence-backed diagnosis, затем узкий patch, затем полный набор gates. «Скомпилировалось» не является завершением live-задачи.

## 9. Definition of Done всего проекта

Проект можно назвать готовым только когда одновременно выполнено:

- нет query storm при continuous mouse input;
- kernel сам получает state response и регистрирует battery после boot/rebind;
- нет фоновых DPI/RGB writes по умолчанию;
- loaded/installed/built/DKMS versions совпадают;
- c600 и c601 live matrix пройдена;
- reconnect и suspend/resume пройдены;
- один owner HID writes в каждый момент;
- stale/unknown состояния честно видны в CLI/GUI/UPower;
- multi-device либо реализован, либо явно и безопасно отклонён;
- все автоматические gates есть в CI;
- installer/uninstaller проверены в чистом staging/VM;
- docs не противоречат implementation;
- release содержит checksums, SBOM, known limitations и точный список live-проверок.

До выполнения P0-1…P0-4 версию `2.3.3` нельзя считать production-ready, несмотря на зелёные build и unit tests.
