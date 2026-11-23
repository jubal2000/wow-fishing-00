# sound-trigger

실행 중인 게임의 **소리만 골라 듣다가**, 미리 준비한 "목표 소리"가 나오면 **지정한 키를 자동으로 누르는** 프로그램입니다.
macOS와 Windows에서 동작하며, 모든 처리는 내 컴퓨터 안에서 실시간(10 ms 단위)으로 돌아갑니다. 인터넷이나 AI 서버를 쓰지 않습니다.

대표 용도는 월드 오브 워크래프트(WoW) 낚시입니다. 찌가 첨벙하는 소리를 듣고 상호작용 키로 건진 뒤 다시 던집니다.

> ⚠️ 게임 자동화는 대부분의 온라인 게임 이용약관(WoW 포함)에 위배되며 계정 제재를 받을 수 있습니다. 사용 여부와 책임은 사용자에게 있습니다.

---

## 목차

1. [동작 원리](#1-동작-원리)
2. [지원 환경](#2-지원-환경)
3. [macOS: 빌드와 사용법](#3-macos)
4. [Windows: 빌드와 사용법](#4-windows)
5. [WoW 게임 설정 (공통)](#5-wow-게임-설정-공통)
6. [명령어 설명](#6-명령어-설명)
7. [run 옵션 전체](#7-run-옵션-전체)
8. [키 지정 문법](#8-키-지정-문법)
9. [기준 소리(templates)](#9-기준-소리-templates)
10. [감지 튜닝 가이드](#10-감지-튜닝-가이드)
11. [문제 해결](#11-문제-해결)
12. [프로젝트 구조](#12-프로젝트-구조)

---

## 1. 동작 원리

```
 게임 소리 ──▶ 앱별 오디오 캡처 ──▶ 16 kHz 변환 ──▶ 로그-멜 스펙트로그램 ──▶ 기준 소리들과 비교
 (WoW만)       (48 kHz, 모노)                    (10 ms마다 40개 주파수 대역)       │
                                                                                   ▼
                                       키 입력 ◀── 동작 실행 ◀── 점수가 기준값 이상이면 "감지"
                                     (랜덤 지연)   (--actions)
```

- **앱별 캡처**: 시스템 전체가 아니라 지정한 게임 프로세스의 소리만 받습니다. 음악이나 영상 소리는 섞이지 않습니다.
- **비교 방식**: 소리의 *크기*가 아니라 *모양*(어떤 높이의 소리가 어떤 순서로 나는지)을 비교해 0~1 점수를 냅니다. 게임 볼륨이 바뀌어도 점수는 거의 변하지 않습니다.
- **여러 기준 소리**: 같은 효과음도 여러 버전이 있을 수 있어서, 기준 소리를 여러 개 두고 그중 가장 잘 맞는 점수를 씁니다.
- **한 번만 반응**: 점수가 최고점에 닿은 뒤 한 번만 판정하고, 키를 누른 직후(`--cooldown`)에는 소리를 무시합니다. 내가 누른 동작의 소리에 다시 반응하지 않게 하기 위해서입니다.

## 2. 지원 환경

| | macOS | Windows |
|---|---|---|
| OS | macOS 14 이상 (Apple Silicon에서 확인, Intel은 미확인) | Windows 10 2004 이상, Windows 11 |
| 오디오 캡처 | ScreenCaptureKit (앱별) | WASAPI 프로세스 루프백 (앱별) |
| 키 입력 | CoreGraphics 키 이벤트 | SendInput (하드웨어 스캔코드) |
| `--background` (뒤에서 동작) | CGEventPostToPid | 게임 창에 PostMessage |
| 필요한 권한 | 화면 및 시스템 오디오 녹음, 손쉬운 사용 | 없음 (게임이 관리자면 이 프로그램도 관리자로) |
| 실행 파일 | `target/release/sound-trigger` | `target\release\sound-trigger.exe` |
| 검증 상태 | 실제 WoW에서 동작 확인 | 컴파일만 확인, **실기 테스트 전** |

명령어, 옵션, 기준 소리 파일은 두 플랫폼이 완전히 같습니다.

---

## 3. macOS

### 3-1. 준비물

1. **Xcode Command Line Tools**
   ```bash
   xcode-select --install
   ```
2. **Rust**
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```
   설치 후 터미널을 새로 열고 `cargo --version`이 나오는지 확인합니다.

### 3-2. 소스 받기와 빌드

```bash
git clone https://github.com/jubal2000/wow-fishing-00.git
cd wow-fishing-00
cargo build --release
```

- 처음 빌드는 의존성을 받느라 1~3분 걸립니다.
- 결과물은 `target/release/sound-trigger`입니다.
- 코드를 받은 뒤 다시 빌드할 때도 `cargo build --release`만 실행하면 됩니다.

### 3-3. 권한 설정 (최초 1회)

macOS는 다른 앱의 소리를 듣거나 키를 보내려면 권한이 필요합니다. 권한은 **이 프로그램을 실행하는 터미널 앱**(터미널, iTerm 등)에 줍니다.

```bash
./target/release/sound-trigger perms
```

1. macOS 권한 요청 창이 뜨면 **설정 열기**를 누릅니다.
2. 시스템 설정 → 개인정보 보호 및 보안에서 아래 두 항목에 쓰시는 터미널 앱을 켭니다.
   - **화면 및 시스템 오디오 녹음**: 게임 소리 캡처용
   - **손쉬운 사용**(버전에 따라 "기기 제어 및 데이터 접근"으로 보일 수 있음): 키 입력용
3. 터미널을 **완전히 종료(Cmd+Q)**했다가 다시 엽니다.
4. `perms`를 다시 실행해 두 줄 모두 `OK`인지 확인합니다.

```
키 입력 권한 (Accessibility / 기기 제어): OK
화면 및 시스템 오디오 녹음:              OK
```

### 3-4. 사용 방법 (WoW 낚시)

먼저 [5. WoW 게임 설정](#5-wow-게임-설정-공통)을 마치세요.

**① 게임이 보이는지 확인**
```bash
./target/release/sound-trigger apps
```
목록에 `Wow` (com.blizzard.worldofwarcraft)가 나오면 됩니다.

**② 키가 게임에 들어가는지 확인.** 3초 안에 WoW 창을 클릭하면 낚시가 던져져야 합니다.
```bash
./target/release/sound-trigger press "." --delay 3
```

**③ 감지만 시험 (키는 안 누름).** 낚시를 직접 던지고, 첨벙할 때 `DETECTED`가 찍히는지 봅니다.
```bash
./target/release/sound-trigger run --app "World of Warcraft" --template templates --threshold 0.8 --dry-run -v
```

**④ 실제 실행.** 명령을 실행한 뒤 3초 안에 WoW 창을 앞으로 띄우세요.
```bash
./target/release/sound-trigger run --app "World of Warcraft" --template templates --threshold 0.8 \
  --start-actions "." \
  --actions ",:300-700,.:1500-2500" \
  --timeout 21 --timeout-actions ".:500-1000" \
  --cooldown 3
```

| 부분 | 뜻 |
|---|---|
| `--start-actions "."` | 시작할 때 `.`(낚시 매크로)로 던짐 |
| `--actions ",:300-700,.:1500-2500"` | 첨벙 감지 → 0.3~0.7초 뒤 `,`(상호작용)로 건짐 → 1.5~2.5초 뒤 `.`으로 다시 던짐 |
| `--timeout 21 --timeout-actions ".:500-1000"` | 21초간 첨벙이 없으면 다시 던짐 |
| `--cooldown 3` | 키를 누른 뒤 3초 동안은 물소리를 무시 (찌가 떨어지는 소리 등) |

**⑤ 다른 일을 하면서 돌리기.** `--background`를 붙이면 WoW 창이 뒤에 있어도 키가 WoW로만 갑니다. WoW의 배경 소리 설정이 꼭 켜져 있어야 합니다(5번 항목 참고).
```bash
./target/release/sound-trigger run --app "World of Warcraft" --template templates --threshold 0.8 \
  --start-actions "." --actions ",:300-700,.:1500-2500" \
  --timeout 21 --timeout-actions ".:500-1000" --cooldown 3 --background
```

`--background` 없이 실행하면 **WoW가 맨 앞에 있을 때만** 키를 누릅니다. 다른 창에서 작업하는 중이면 키 입력을 건너뛰어서, 엉뚱한 창에 글자가 입력되지 않습니다.

멈출 때는 터미널에서 **Ctrl+C**를 누릅니다.

---

## 4. Windows

> Windows 버전은 컴파일까지만 확인했고, 실제 PC에서는 아직 시험하지 않았습니다. 문제가 생기면 출력 내용과 함께 알려 주세요.

### 4-1. 준비물

1. **Visual Studio Build Tools**
   - https://visualstudio.microsoft.com/visual-cpp-build-tools/ 에서 받아 설치합니다.
   - 설치 화면에서 **"C++를 사용한 데스크톱 개발"**을 체크합니다.
   - Rust가 실행 파일을 만들 때 쓰는 링커가 여기 들어 있습니다.
2. **Rust**
   - https://rustup.rs 에서 `rustup-init.exe`를 받아 실행하고 기본값(1번)으로 설치합니다.
   - 설치 후 PowerShell을 새로 열고 `cargo --version`이 나오는지 확인합니다.
3. **Git** (소스를 받을 때): https://git-scm.com/download/win
   - Git 대신 GitHub에서 ZIP으로 받아 압축을 풀어도 됩니다.

### 4-2. 소스 받기와 빌드

PowerShell에서 실행합니다.
```powershell
git clone https://github.com/jubal2000/wow-fishing-00.git
cd wow-fishing-00
cargo build --release
```

- 결과물은 `target\release\sound-trigger.exe`입니다.
- 이 `.exe`와 `templates` 폴더만 다른 곳에 복사해서 써도 됩니다. 이때는 `--template` 경로를 맞춰 주세요.

### 4-3. 권한

Windows는 별도 권한 설정이 필요 없습니다. 다만 아래 경우는 주의하세요.

- **WoW나 Battle.net을 관리자 권한으로 실행했다면**, 이 프로그램도 **관리자 권한 PowerShell**에서 실행해야 키가 들어갑니다. Windows는 일반 권한 프로그램이 관리자 프로그램에 키를 보내는 것을 막습니다.
- 백신이나 SmartScreen이 직접 빌드한 `.exe`를 경고할 수 있습니다. 직접 빌드한 파일이라면 허용하면 됩니다.

### 4-4. 사용 방법 (WoW 낚시)

먼저 [5. WoW 게임 설정](#5-wow-게임-설정-공통)을 마치세요. 아래 명령은 모두 저장소 폴더에서 **PowerShell**로 실행합니다.

**① 게임이 보이는지 확인**
```powershell
.\target\release\sound-trigger.exe apps
```
목록에 `Wow.exe`와 창 제목 `World of Warcraft`가 나오면 됩니다. `--app`에는 실행 파일 이름, 경로, 창 제목의 일부를 쓰면 됩니다. 예: `"World of Warcraft"`, `"wow.exe"`, `pid:1234`.

**② 키가 게임에 들어가는지 확인.** 3초 안에 WoW 창을 클릭하세요.
```powershell
.\target\release\sound-trigger.exe press "." --delay 3
```

**③ 감지만 시험 (키는 안 누름)**
```powershell
.\target\release\sound-trigger.exe run --app "World of Warcraft" --template templates --threshold 0.8 --dry-run -v
```

**④ 실제 실행**
```powershell
.\target\release\sound-trigger.exe run --app "World of Warcraft" --template templates --threshold 0.8 `
  --start-actions "." `
  --actions ",:300-700,.:1500-2500" `
  --timeout 21 --timeout-actions ".:500-1000" `
  --cooldown 3
```
- PowerShell에서 줄을 나눌 때는 줄 끝에 백틱(`` ` ``)을 붙입니다. 한 줄로 써도 됩니다.
- 쉼표가 들어간 값(`--actions` 등)은 반드시 큰따옴표로 감싸세요. 감싸지 않으면 PowerShell이 쉼표를 배열로 해석합니다.
- 명령 프롬프트(cmd)에서는 줄 끝에 `` ` `` 대신 `^`를 씁니다.

**⑤ 다른 일을 하면서 돌리기.** 맨 끝에 `--background`를 붙입니다. 이 모드에서는 키 메시지를 WoW 창에 직접 보냅니다.
```powershell
.\target\release\sound-trigger.exe run --app "World of Warcraft" --template templates --threshold 0.8 --start-actions "." --actions ",:300-700,.:1500-2500" --timeout 21 --timeout-actions ".:500-1000" --cooldown 3 --background
```
먼저 `press "." --background --app "World of Warcraft" --delay 3`으로 WoW가 뒤에 있을 때도 낚시가 던져지는지 확인하세요. 안 되면 WoW가 이 방식의 키를 받지 않는 것이니, `--background` 없이 WoW를 앞에 두고 사용해야 합니다.

멈출 때는 **Ctrl+C**를 누릅니다.

---

## 5. WoW 게임 설정 (공통)

### 5-1. 콘솔 설정 (채팅창에 한 줄씩 입력, 1회)

```
/console SoftTargetInteract 3
/console SoftTargetInteractArc 2
/console SoftTargetInteractRange 30
/console autoLootDefault 1
/console Sound_EnableSoundWhenGameIsInBG 1
```

| 설정 | 이유 |
|---|---|
| `SoftTargetInteract 3` | 상호작용 키로 낚시찌를 바로 집을 수 있게 함 |
| `SoftTargetInteractArc 2` | 캐릭터가 어느 방향을 보든 찌를 대상으로 잡음 |
| `SoftTargetInteractRange 30` | 멀리 떨어진 찌도 잡을 수 있게 거리를 늘림 |
| `autoLootDefault 1` | 획득 창 없이 바로 주움. **꺼져 있으면 다시 던질 때 획득 창이 닫혀 물고기를 못 주움** |
| `Sound_EnableSoundWhenGameIsInBG 1` | 창이 뒤에 있어도 소리를 냄. **꺼져 있으면 `--background`에서 소리가 0이 되어 감지 불가** |

### 5-2. 키 설정

| 키 | 할당 | 설정 위치 |
|---|---|---|
| `.` | 낚시 매크로 (아래) | 매크로를 만들어 행동 단축바에 놓고, 그 칸의 단축키를 `.`으로 |
| `,` | 대상과 상호작용 (Interact with Target) | 설정(ESC) → 키 설정 |

낚시 매크로:
```
#showtooltip 낚시
/cast 낚시
```

다른 키를 쓰려면 명령의 `--start-actions`, `--actions`, `--timeout-actions`에서 키만 바꾸면 됩니다.

### 5-3. 소리 설정 (권장)

- **효과음 켜기**: 찌 소리가 효과음입니다.
- **배경음악, 환경음 끄기**: 다른 소리가 줄어들수록 점수가 안정적입니다.
- 볼륨은 아무 값이나 괜찮습니다. 점수는 볼륨과 거의 무관합니다.

---

## 6. 명령어 설명

아래 예시는 macOS 기준입니다. Windows는 `./target/release/sound-trigger` 대신 `.\target\release\sound-trigger.exe`로 바꾸세요.

| 명령 | 하는 일 |
|---|---|
| `perms` | (macOS) 필요한 권한을 확인하고 요청 |
| `apps` | 소리를 캡처할 수 있는 실행 중인 앱 목록 |
| `press <키>` | 카운트다운 뒤 키를 눌러 봄 (게임이 키를 받는지 확인용) |
| `record` | 게임 소리를 WAV 파일로 녹음 |
| `trim` | 녹음에서 목표 소리 부분만 잘라 기준 소리로 만듦 |
| `test` | 녹음 파일에 감지기를 돌려 결과를 출력 (튜닝용, 키 안 누름) |
| `run` | 실시간으로 듣다가 감지되면 키를 누름 |

### `press`: 키 입력 시험
```bash
./target/release/sound-trigger press "." --delay 3
./target/release/sound-trigger press ",:0,.:500" --delay 5          # , 누르고 0.5초 뒤 .
./target/release/sound-trigger press "." --app "World of Warcraft" --background
```

### `record`: 녹음
```bash
./target/release/sound-trigger record --app "World of Warcraft" --secs 60 --out my_rec.wav
```
녹음하는 동안 게임에서 목표 소리가 나게 하세요. `--app`을 빼면 시스템 전체 소리를 녹음합니다.

### `trim`: 기준 소리 만들기
```bash
./target/release/sound-trigger trim my_rec.wav --out templates/my_splash.wav                    # 가장 큰 소리 자동 선택
./target/release/sound-trigger trim my_rec.wav --start 12.3 --end 13.0 --out templates/my_splash.wav
```
WAV와 Ogg Vorbis(`.ogg`)를 읽을 수 있습니다. 0.5~0.8초 길이로, 소리의 시작 부분을 포함해 자르는 것이 좋습니다.

### `test`: 녹음으로 감지 검증
```bash
./target/release/sound-trigger test my_rec.wav --template templates --threshold 0.8
```
출력 예:
```
Template #1: templates/0_bobber_ogg1.wav (670 ms, -19.8 dBFS)
...
Detect: threshold 0.80, near-miss >= 0.70, min -70 dBFS, peak 60 ms
  HIT         17.81s  score 0.980  (template #6)
  near miss   63.36s  score 0.709  (template #4)
  HIT         92.19s  score 0.973  (template #1)
```
`HIT`는 감지된 시점(녹음 기준 초), `near miss`는 기준값에 아깝게 못 미친 소리입니다.

### `run`: 실시간 실행
출력 예:
```
Template #1: templates/0_bobber_ogg1.wav (670 ms, -19.8 dBFS)
...
Listening to Wow (com.blizzard.worldofwarcraft, pid 12360). Ctrl+C to stop.
시작!
Start actions:
  -> press '.' (after 0 ms)
[17:21:55] DETECTED (score 0.968, template #3)
  -> press ',' (after 661 ms)
  -> press '.' (after 1927 ms)
[17:22:19] no detection for 21s -> timeout actions
  -> press '.' (after 734 ms)
```

---

## 7. run 옵션 전체

### 감지 관련 (`test`에도 동일하게 적용)

| 옵션 | 기본값 | 설명 |
|---|---|---|
| `--template` | `template.wav` | 기준 소리. 파일 여러 개를 쉼표로 나열하거나 폴더를 지정하면 안의 `.wav`/`.ogg`를 전부 사용. 가장 잘 맞는 것으로 판정 |
| `--threshold` | `0.75` | 감지 기준값(0~1). 놓치면 내리고, 엉뚱한 소리에 반응하면 올림. **`templates` 폴더를 쓸 때는 0.8 권장** |
| `--near` | 기준값 - 0.1 | 이 점수 이상인데 기준 미달인 소리를 `near miss`로 표시. `run`에서는 `-v`나 `--near`를 줄 때만 표시 |
| `--min-db` | `-70` | 이보다 작은 소리(dBFS)는 무시. 먼 곳의 소리를 거르려면 `-55`처럼 올리고, 너무 작아 놓치면 `-80`처럼 내림 |
| `--peak-ms` | `60` | 점수가 이 시간(ms) 동안 더 오르지 않으면 판정. 작을수록 빠르고, 클수록 정확 |

### 동작 관련

| 옵션 | 기본값 | 설명 |
|---|---|---|
| `--app` | (없음: 시스템 전체) | 대상 앱. 이름, 번들 ID(맥), 실행 파일 이름이나 경로, 창 제목(윈도우)의 일부, 또는 `pid:1234` |
| `--actions` | `f:250-600` | 감지 시 누를 키들 ([8. 키 지정 문법](#8-키-지정-문법)) |
| `--start-delay` | `3` | 실행 후 첫 동작까지 기다리는 시간(초). 게임 창으로 전환할 시간 |
| `--start-actions` | – | 시작할 때 한 번 누를 키들 |
| `--timeout` | – | 이 시간(초) 동안 감지가 없으면 `--timeout-actions` 실행. 둘은 함께 써야 함 |
| `--timeout-actions` | – | 시간 초과 시 누를 키들 |
| `--cooldown` | `1.5` | 키를 누른 뒤 소리를 무시하는 시간(초). 낚시는 `3` 권장 |
| `--hold-ms` | `80-150` | 키를 누르고 있는 시간(ms). 게임이 키를 놓치면 `150-250`처럼 늘림 |
| `--background` | 꺼짐 | 게임이 뒤에 있어도 게임에만 키를 보냄. 끄면 게임이 맨 앞일 때만 키를 누름 |
| `--dry-run` | 꺼짐 | 감지만 하고 키는 누르지 않음 |
| `-v` | 꺼짐 | 실시간 점수 막대와 near miss 표시 |

`-v` 점수 막대 예시. `|`가 기준값 위치입니다.
```
  score 0.412 [################................|.......]  -48.2 dB
```

---

## 8. 키 지정 문법

`--actions`, `--start-actions`, `--timeout-actions`, `press`에서 같은 문법을 씁니다.

```
키:최소ms-최대ms, 키:최소ms-최대ms, ...
```

- 각 단계는 **최소~최대 ms 사이에서 무작위로 기다린 뒤** 키를 누릅니다.
- 지연을 생략하면 바로 누릅니다: `"."` = `".:0"`
- 고정 지연: `"f:500"`
- `,` 키도 쓸 수 있습니다. 단계 맨 앞의 쉼표는 키로 읽습니다: `",:300-700,.:1500"` = `,` 누르고 → `.` 누름
- 헷갈리면 이름으로 써도 됩니다: `"comma:300-700,period:1500"`

| 사용 가능한 키 | 이름 |
|---|---|
| 알파벳, 숫자 | `a`~`z`, `0`~`9` |
| 기능키 | `f1`~`f12` |
| 특수 | `space`, `return`(`enter`), `tab`, `esc`, `up`, `down`, `left`, `right` |
| 기호 | `,` `.` `/` `;` `'` `[` `]` `\` `-` `=` `` ` `` |
| 기호 별칭 | `comma`, `period`(`dot`), `slash`, `semicolon`, `quote`, `lbracket`, `rbracket`, `backslash`, `minus`, `equal`, `grave` |

---

## 9. 기준 소리 (templates)

WoW 낚시찌 소리는 **3가지 버전**이 무작위로 재생됩니다. 하나만 쓰면 셋 중 둘을 놓칩니다.

| 파일 | 내용 |
|---|---|
| `templates/0_bobber_ogg1.wav` | `FishingBobber_ver2_1.ogg`의 0.05~0.75초 |
| `templates/0_bobber_ogg2.wav` | `FishingBobber_ver2_2.ogg`의 0.00~0.70초 |
| `templates/0_bobber_ogg3.wav` | `FishingBobber_ver2_3.ogg`의 0.05~0.75초 |
| `templates/splash_a~c.wav` | 게임에서 직접 녹음한 첨벙 소리 (1번 버전) |

`--template templates`처럼 폴더를 지정하면 전부 사용합니다.

**검증 결과** (실제 게임 녹음 3개, 기준값 0.8): 첨벙 15번을 0.82~1.00점으로 감지. 다른 소리는 최대 0.79점.

### 새 기준 소리 추가하기

다른 장소나 다른 게임에서 자꾸 놓친다면:

1. 녹음합니다: `record --app "..." --secs 60 --out new.wav`
2. 어디서 첨벙했는지 찾습니다: `test new.wav --template templates --threshold 0.5`
3. 첨벙 시점 직전 약 0.7초를 잘라 폴더에 넣습니다: `trim new.wav --start 41.3 --end 42.0 --out templates/splash_d.wav`
4. 다시 `test`해서 점수가 올랐는지 확인합니다.

`trim`의 `--start`는 `HIT`에 찍힌 시각보다 약 0.7초 앞입니다. `HIT` 시각은 소리가 끝난 쪽에 가깝습니다.

---

## 10. 감지 튜닝 가이드

| 증상 | 확인 | 해결 |
|---|---|---|
| 첨벙했는데 반응 없음 | `-v`로 실행했을 때 `near miss`가 뜨는지 | 뜨면 그 점수보다 살짝 낮게 `--threshold`를 내림 |
| 첨벙했는데 아무것도 안 뜸 | 점수 막대의 dB 값 | `-70`보다 작으면 `--min-db -80`. `-180`이면 소리가 안 들어오는 것 (11번 항목) |
| 엉뚱한 소리에 반응 | `DETECTED`의 점수 | 그 점수보다 높게 `--threshold`를 올림 |
| 반응이 늦음 | – | `--peak-ms 30`, 또는 기준 소리를 짧게(0.4~0.5초) |
| 키를 누른 직후 또 반응 | – | `--cooldown`을 늘림 |
| 키가 게임에서 무시됨 | `press`로 시험 | `--hold-ms 150-250` |

`near miss`가 많이 떠도 정상입니다. 찌가 떨어지는 소리나 물고기를 건지는 소리처럼, 비슷한 물소리가 기준값 아래에서 걸러진 것입니다.

---

## 11. 문제 해결

### 공통
| 증상 | 원인과 해결 |
|---|---|
| `no running app matches` | 게임이 실행 중인지 확인하고, `apps` 목록에 나오는 이름을 `--app`에 씀 |
| `!! 게임 소리가 완전히 0입니다` | WoW가 뒤에서 소리를 끈 상태. `/console Sound_EnableSoundWhenGameIsInBG 1` |
| 감지는 되는데 물고기를 안 건짐 | `autoLootDefault 1`, 상호작용 키 설정, 찌 위에 손 아이콘이 뜨는지 확인 (`/console SoftTargetIconGameObject 1`) |
| `!! 게임 창이 맨 앞이 아니라서 키 입력을 건너뜀` | 정상적인 안전장치. 게임을 앞에 두거나 `--background` 사용 |
| `unknown key` | [8. 키 지정 문법](#8-키-지정-문법) 참고 |

### macOS
| 증상 | 원인과 해결 |
|---|---|
| `ScreenCaptureKit failed` / `TCC를 거절함` | 화면 및 시스템 오디오 녹음 권한이 없음 → `perms` 실행 후 터미널 재시작 |
| `Accessibility permission is missing` | 손쉬운 사용 권한이 없음 → `perms` 실행 후 터미널 재시작 |
| `Library not loaded: libswift_Concurrency.dylib` | `build.rs`가 빠진 채 빌드됨 → 저장소 전체를 받아 다시 빌드 |

### Windows
| 증상 | 원인과 해결 |
|---|---|
| `link.exe not found` (빌드 시) | Visual Studio Build Tools의 "C++를 사용한 데스크톱 개발"이 설치되지 않음 |
| `process loopback needs Windows 10 2004 or newer` | Windows 업데이트 필요 |
| `SendInput was blocked` | 게임이 관리자 권한으로 실행 중 → PowerShell도 관리자 권한으로 실행 |
| `--background`에서 키가 안 먹힘 | WoW가 PostMessage 입력을 받지 않는 경우. 게임을 앞에 두고 `--background` 없이 사용 |
| `--actions` 값이 이상하게 해석됨 | PowerShell에서 쉼표가 든 값을 큰따옴표로 감싸지 않음 |

---

## 12. 프로젝트 구조

```
├── src/
│   ├── main.rs          명령어(CLI), run 루프
│   ├── features.rs      로그-멜 특징 추출, 템플릿 매칭, 감지기 (공통)
│   ├── capture/
│   │   ├── mac.rs       ScreenCaptureKit 앱별 오디오 캡처
│   │   └── win.rs       WASAPI 프로세스 루프백 캡처
│   ├── keys/
│   │   ├── mod.rs       키 지정 문법, 타이밍, 안전장치 (공통)
│   │   ├── mac.rs       CoreGraphics 키 이벤트
│   │   └── win.rs       SendInput / PostMessage
│   └── win_util.rs      (Windows) 프로세스·창 찾기
├── templates/           기준 소리 (낚시찌 3종 + 실제 녹음)
├── FishingBobber_ver2_*.ogg   원본 낚시찌 효과음
├── bobber_template.wav  초기 단일 기준 소리 (하위 호환용)
└── build.rs             (macOS) Swift 런타임 경로 설정
```
