# sound-trigger

맥에서 실행 중인 특정 앱(게임)의 소리만 캡처해서, 미리 녹음한 "목표 소리"가 나오면 지정한 키를 자동으로 누릅니다.
모든 처리는 로컬에서 실시간(10 ms 단위)으로 돌아갑니다.

- 오디오 캡처: ScreenCaptureKit (앱별 오디오, BlackHole 같은 가상 드라이버 불필요)
- 감지: 로그-멜 스펙트로그램 템플릿 매칭 (정규화 상관, 0~1 점수)
- 키 입력: CoreGraphics 키 이벤트 (랜덤 지연 포함)

## 1. 권한 (최초 1회)

시스템 설정 → 개인정보 보호 및 보안 에서 **이 명령을 실행하는 터미널 앱**에 권한을 줍니다. 권한을 준 뒤 터미널을 재시작하세요.

- **화면 및 시스템 오디오 녹음**: 게임 소리 캡처에 필요
- **손쉬운 사용(Accessibility)**: 키 입력에 필요

## 2. 빌드

```bash
cargo build --release
```

바이너리: `target/release/sound-trigger`

## 3. 사용 순서

```bash
# 실행 중인 앱 이름 확인
./target/release/sound-trigger apps

# 목표 소리가 나는 동안 녹음 (예: 15초)
./target/release/sound-trigger record --app "World of Warcraft" --secs 15 --out rec.wav

# 녹음에서 목표 소리만 잘라 템플릿으로 만들기 (자동: 가장 큰 소리 구간)
./target/release/sound-trigger trim rec.wav --out template.wav
#   직접 구간 지정: --start 4.2 --end 4.9
afplay template.wav   # 제대로 잘렸는지 귀로 확인

# 녹음 파일로 오프라인 검증 (점수/오탐 확인)
./target/release/sound-trigger test rec.wav --template template.wav

# 실시간 실행 (먼저 --dry-run -v 로 점수만 보면서 threshold 조정 권장)
./target/release/sound-trigger run --app "World of Warcraft" --template template.wav --dry-run -v
./target/release/sound-trigger run --app "World of Warcraft" --template template.wav --actions "f:300-700"
```

키 입력은 **포커스된(맨 앞) 앱**으로 갑니다. 게임 창을 앞에 두세요.

## run 옵션

| 옵션 | 기본값 | 설명 |
|---|---|---|
| `--template` | template.wav | 기준 소리. 쉼표로 여러 개, 또는 폴더(안의 .wav/.ogg 전부). 가장 잘 맞는 것으로 판정 |
| `--threshold` | 0.75 | 감지 기준 0~1. 놓치면 내리고, 엉뚱한 소리에 반응하면 올림 |
| `--near` | threshold-0.1 | 이 점수 이상인데 기준 미달인 소리를 `near miss`로 출력 (튜닝용) |
| `--min-db` | -70 | 이보다 작은 소리(dBFS)는 무시. 멀리서 나는 소리를 무시하려면 올림(예: -55) |
| `--peak-ms` | 60 | 점수가 이 시간 동안 더 안 오르면 판정. 작을수록 빠르고, 클수록 정확 |
| `--hold-ms` | 80-150 | 키를 누르고 있는 시간 |
| `--actions` | `f:250-600` | 감지 시 누를 키들. `키:최소ms-최대ms` (대기 후 누름), 쉼표로 여러 개 |
| `--start-actions` | – | 시작할 때 한 번 누를 키들 |
| `--timeout` + `--timeout-actions` | – | N초간 감지 없으면 실행할 키들 |
| `--cooldown` | 1.5 | 키 누른 뒤 이 시간 동안 소리 무시 (자기 동작 소리로 재감지 방지) |
| `--dry-run` | – | 감지만 하고 키는 안 누름 |
| `-v` | – | 실시간 점수 미터 표시 |

키 이름: `a`–`z`, `0`–`9`, `f1`–`f12`, `space`, `return`, `tab`, `esc`, 방향키(`up` 등), 기호(`-` `=` `[` `]` `;` `'` `,` `.` `/` `` ` ``).

### 예시: 소리 → 상호작용 키, 잠시 후 다시 시작 키

```bash
./target/release/sound-trigger run --app "World of Warcraft" --template template.wav \
  --start-actions "1" \
  --actions "f:300-700,1:1500-2500" \
  --timeout 25 --timeout-actions "1:500-1000" \
  --cooldown 2
```

## 낚시찌 소리 템플릿 (준비됨)

`FishingBobber_ver2_1.ogg`(Ogg Vorbis도 바로 읽음)의 첨벙 소리 본체 0.05~0.75초를 잘라 `bobber_template.wav`로 만들어 두었습니다.

```bash
./target/release/sound-trigger trim FishingBobber_ver2_1.ogg --start 0.05 --end 0.75 --out bobber_template.wav
```

모의 테스트(잡음 + 다른 효과음 12개 + 찌 소리 7번, 볼륨 25~100%, 음높이 ±4%): 7/7 감지, 오탐 0.
찌 소리 점수 0.82~0.95, 다른 효과음은 0.4 미만 → `--threshold 0.7` 권장.

```bash
./target/release/sound-trigger run --app "World of Warcraft" --template bobber_template.wav \
  --threshold 0.7 \
  --start-actions "1" \
  --actions "f:300-700,1:1500-2500" \
  --timeout 21 --timeout-actions "1:500-1000" \
  --cooldown 2
```

(`1` = 낚시 시전 키, `f` = 상호작용 키로 가정. 게임 키 설정에 맞게 바꾸세요.)

## 기준 소리 폴더 (templates/)

WoW 낚시찌 소리는 **3가지 버전**(FishingBobber_ver2_1~3.ogg)이 무작위로 재생됩니다. 셋 다 넣어야 합니다.

| 파일 | 내용 |
|---|---|
| `0_bobber_ogg1~3.wav` | 원본 ogg 3종의 앞부분 0.7초 |
| `splash_a~c.wav` | 게임에서 녹음한 실제 첨벙 소리 (1번 버전) |

폴더를 지정하면 전부 사용하고 가장 잘 맞는 것으로 판정합니다.
1번만 쓸 때는 2·3번 첨벙을 0.6~0.7점으로 놓쳤지만, 3종을 모두 쓰면 0.82~1.00으로 감지됩니다.

```bash
./target/release/sound-trigger run --app "World of Warcraft" --template templates --threshold 0.8 --cooldown 3 ...
```

새 장소에서 놓치면 `record`로 녹음 → `test`로 첨벙 시점 확인 → `trim`으로 잘라 `templates/`에 추가하세요.

## 튜닝 팁

- 템플릿은 목표 소리의 앞부분(어택)을 포함해서 0.3~1초 정도가 좋습니다.
- 게임 내 배경음악/환경음을 끄면 정확도가 크게 올라갑니다. 효과음 볼륨을 일정하게 유지하세요.
- 비슷한 효과음이 많으면 `test`로 오탐 점수를 확인하고 threshold를 그보다 위로 잡으세요.
