# Инструкция для тестировщика — The Herta Voice Assistant (Linux)

Цель: обкатать сборку на Linux и проверить, что всё запускается и работает. Прогоняй шаги по порядку. К каждому шагу указан **ожидаемый результат** и что делать/сообщить, если он не совпал.

Если шаг провалился — **не останавливайся**, отметь его в чек-листе в конце и иди дальше (кроме шагов 1–4: без них остальное не поедет).

---

## 0. Что понадобится

- Linux с рабочим звуком (PipeWire / PulseAudio / ALSA)
- Python 3.11 или новее
- Микрофон и колонки/наушники
- Хотя бы один LLM-провайдер:
  - **проще всего** — ключ Google AI Studio (для текста, `--voice` и `--live-voice`): https://aistudio.google.com/
  - либо локальный Ollama, либо Cerebras / DeepSeek / OpenRouter
- Интернет (для Edge TTS, облачных LLM и первой загрузки Whisper-модели)

Проверь версию Python:

```bash
python3 --version
```

Ожидается `Python 3.11.x` или выше. Если ниже — поставь новее.

---

## 1. Системные пакеты

```bash
sudo apt update
sudo apt install -y python3-venv ffmpeg xdg-user-dirs libportaudio2
```

(Fedora: `sudo dnf install python3-virtualenv ffmpeg xdg-user-dirs portaudio`; Arch: `sudo pacman -S python-virtualenv ffmpeg xdg-user-dirs portaudio`)

Проверка, что ffmpeg на месте:

```bash
ffmpeg -version | head -1
```

**Ожидается:** строка вида `ffmpeg version ...`. Если `command not found` — Edge TTS не сможет проигрывать звук, отметь это.

---

## 2. Получить проект и окружение

```bash
git clone https://github.com/phaeton-oq/The-Herta-voice-assistant.git
cd The-Herta-voice-assistant

python3 -m venv .venv
source .venv/bin/activate
```

**Ожидается:** слева в строке терминала появилось `(.venv)`.

> Важно: `source .venv/bin/activate` нужно выполнять в **каждом новом** окне терминала перед запуском Герты.

---

## 3. Установить зависимости

```bash
python -m pip install --upgrade pip
python -m pip install -r requirements.txt
```

**Ожидается:** в конце `Successfully installed ...`. Установка тяжёлая (torch, faster-whisper) — может занять несколько минут и скачать сотни МБ.

**Если ошибка** — скопируй последние ~20 строк вывода в отчёт.

---

## 4. Конфигурация (.env)

```bash
cp .env.example .env
nano .env      # или любой редактор
```

Минимально впиши ключ провайдера. Пример для Google AI Studio — найди и заполни строки:

```env
LLM_PROVIDER=google_ai
GOOGLE_AI_API_KEY=сюда_свой_ключ
GOOGLE_AI_MODEL=gemma-3-27b-it
```

Сохрани файл (`Ctrl+O`, `Enter`, `Ctrl+X` в nano).

> `.env` в `.gitignore` и не коммитится — ключ в репозиторий не попадёт.

---

## 5. Диагностика окружения

```bash
python main.py --doctor
```

**Ожидается:** отчёт со строками `[OK]` и в конце `Summary: ok=..., warn=..., fail=0`.

- `fail=0` — отлично.
- `[WARN]` по выключенным фичам (web search, RVC, system actions, отсутствие части ключей) — это нормально.
- **Любой `[FAIL]`** — выпиши строку целиком в отчёт.

---

## 6. Аудиоустройства

```bash
python main.py --list-devices
python main.py --list-output-devices
```

**Ожидается:** списки вида `[7] Микрофон ... | input_channels=...` и `[9] Динамики ... | outputs=...`.

Запиши:
- индекс своего **микрофона** → пойдёт в `AUDIO_DEVICE`
- индекс своих **колонок/наушников** → пойдёт в `AUDIO_OUTPUT_DEVICE`

Впиши их в `.env`:

```env
AUDIO_DEVICE=7
AUDIO_OUTPUT_DEVICE=9
```

(подставь свои числа)

**Если списки пустые или микрофон/выход не видны** — проверь `pactl list sources short` и `pactl list sinks short`, отметь в отчёте.

---

## 7. Тест вывода звука

```bash
python main.py --output-test
```

**Ожидается:** короткий тон (писк) из колонок/наушников.

**Нет звука** — проверь, что `AUDIO_OUTPUT_DEVICE` указывает на реальный выход (из шага 6), повтори. Отметь результат.

---

## 8. Текстовый режим (проверка LLM)

Один вопрос:

```bash
python main.py --text "Привет, кто ты?" --no-tts
```

**Ожидается:** Герта отвечает текстом в характере (высокомерно, лаконично, как Великая Герта).

Интерактивный чат:

```bash
python main.py --no-tts
```

Пообщайся пару реплик, выйди через `Ctrl+C`.

**Ошибки:**
- `GOOGLE_AI_API_KEY is not configured` → ключ не вписан в `.env`
- `401` → неверный ключ
- `429` → лимит провайдера, подожди или смени модель

---

## 9. Тест озвучки (Edge TTS)

```bash
python main.py --tts-test
```

**Ожидается:** Герта произносит короткую фразу голосом (Edge TTS через ffmpeg на твоё устройство вывода).

**Возможные исходы:**
- Голос звучит → ✅
- `No MP3 player available ...` → не установлен ffmpeg (вернись к шагу 1)
- `No audio was received` → нет интернета или Edge TTS недоступен в твоём регионе; отметь
- Звук есть, но из «не того» устройства → проверь `AUDIO_OUTPUT_DEVICE`

---

## 10. Голосовой режим (legacy `--voice`)

Полный локальный пайплайн: микрофон → VAD → Whisper → LLM → TTS.

```bash
python main.py --voice
```

**Ожидается:**
- на первом запуске Whisper скачает модель (это нормально, подожди)
- появится готовность к прослушиванию
- Герта реагирует **только после имени**: скажи «**Герта**, привет, как дела?»
- после ответа ~60 секунд можно говорить без имени (follow-up окно)

Выход — `Ctrl+C`.

Проверь:
- слышит ли микрофон (распознаётся ли речь)
- срабатывает ли wake-word по имени «Герта»
- отвечает ли голосом

Отметь, что из этого работает.

---

## 11. Live-режим (основной, `--live-voice`) — если есть ключ Google AI

Нативное аудио Gemini. Впиши в `.env`:

```env
GOOGLE_AI_API_KEY=сюда_свой_ключ
GOOGLE_AI_LIVE_MODEL=gemini-3.1-flash-live-preview
GOOGLE_AI_LIVE_API_VERSION=v1beta
GOOGLE_AI_LIVE_THINKING_LEVEL=minimal
GOOGLE_AI_LIVE_VOICE=Kore
GOOGLE_AI_LIVE_PLAYBACK=google
```

Запуск:

```bash
python main.py --live-voice
```

**Ожидается:** строка `Voice mode ready`, после чего можно говорить в микрофон и слышать ответ голосом Gemini. Выход — `Ctrl+C`.

**Если модель недоступна/нестабильна** — попробуй альтернативную:

```env
GOOGLE_AI_LIVE_MODEL=gemini-2.5-flash-native-audio-preview-12-2025
GOOGLE_AI_LIVE_API_VERSION=v1alpha
GOOGLE_AI_LIVE_AFFECTIVE_DIALOG=true
```

Отметь, какая модель завелась.

---

## 12. (Опционально) Безопасные системные действия

Включи в `.env`:

```env
SYSTEM_ACTIONS_ENABLED=true
SYSTEM_ACTIONS_DOCUMENT_DIR=desktop
```

Запусти любой голосовой/текстовый режим и попробуй команды:
- «Герта, открой ютуб» → должен открыться браузер на YouTube
- «Герта, открой VS Code» → должен запуститься VS Code (нужна команда `code` в PATH)
- «Герта, создай папку тест Герты» → папка появляется на рабочем столе
- «Герта, создай текстовый документ с названием план и текстом купить чай» → `.txt` на рабочем столе

Проверь, что файлы реально появились (на Linux рабочий стол определяется через `xdg-user-dir DESKTOP`):

```bash
xdg-user-dir DESKTOP    # покажет путь к рабочему столу
ls "$(xdg-user-dir DESKTOP)"
```

**Должно быть запрещено** (Герта откажется): удалять, перемещать, перезаписывать файлы, форматировать, выполнять произвольные команды.

---

## 13. (Опционально) Долговременная память

В текстовом режиме:
- «Герта, запомни что меня зовут Влад»
- «Герта, что ты обо мне помнишь» → должна назвать факт
- перезапусти Герту и снова спроси «что ты обо мне помнишь» → факт должен сохраниться (`data/long_memory.json`)
- «Герта, забудь что меня зовут Влад» → удаляет

---

## 14. (Опционально) Web search и Code tools

Web search (нужен ключ Tavily, https://tavily.com/):

```env
WEB_SEARCH_ENABLED=true
TAVILY_API_KEY=tvly-...
```
Команда: «Герта, найди новости про Anthropic» → пересказ результатов голосом Герты.

Code tools:

```env
CODE_TOOLS_ENABLED=true
```
Команды: «проверь типы в файле main.py» (mypy), «линтуй main.py» (ruff).

---

## 15. (Опционально) RVC-голос Герты

Только если будешь проверять оригинальный голос. Требует отдельной установки Applio:

```bash
cd ~
git clone https://github.com/IAHispano/Applio.git
cd Applio
chmod +x run-install.sh
./run-install.sh
```

В `.env` проекта Герты:

```env
RVC_TTS_ENABLED=true
RVC_BACKEND=persistent
RVC_APPLIO_ROOT=/home/USER/Applio
RVC_APPLIO_PYTHON=/home/USER/Applio/.venv/bin/python
RVC_MODEL_PATH=/home/USER/HertaVoice/model.pth
RVC_INDEX_PATH=
RVC_F0_METHOD=rmvpe
```

(замени `USER` и пути на свои; `model.pth` — модель голоса Герты)

Проверка CUDA в окружении Applio:

```bash
~/Applio/.venv/bin/python -c "import torch; print(torch.cuda.is_available(), torch.version.cuda)"
```

Затем:

```bash
python main.py --tts-test     # озвучка голосом Герты
```

RVC заметно медленный — это ожидаемо.

---

## Чек-лист (заполни и верни)

```
Система (дистрибутив, версия):        _______________
Python:                                _______________
GPU (если есть):                       _______________
LLM-провайдер для теста:               _______________

[ ] 1. Системные пакеты (ffmpeg и пр.)
[ ] 3. pip install -r requirements.txt
[ ] 5. --doctor (fail=0?)              ok=__ warn=__ fail=__
[ ] 6. --list-devices / список устройств виден
[ ] 7. --output-test (слышен тон)
[ ] 8. --text (Герта отвечает)
[ ] 9. --tts-test (Edge TTS звучит)
[ ] 10. --voice (микрофон + wake-word + голос)
[ ] 11. --live-voice (какая модель: __________)
[ ] 12. системные действия (опц.)
[ ] 13. долговременная память (опц.)
[ ] 14. web search / code tools (опц.)
[ ] 15. RVC (опц.)
```

## Как сообщать о проблемах

Для каждой ошибки укажи:
1. Номер шага.
2. Точную команду, которую запускал.
3. Полный текст ошибки (последние ~20 строк вывода).
4. Что показывает `python main.py --doctor` (если относится к запуску).

Спасибо за обкатку. — phaeton (https://t.me/cmd_phaeton_oq)
