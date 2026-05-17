# ServerScope

ServerScope는 개발자와 1인 서버 운영자를 위한 가벼운 SSH 기반 Ubuntu 서버 모니터링 데스크탑 앱입니다.

## 해결하는 문제

VPS나 개인 Ubuntu 서버를 운영할 때 상태 확인을 위해 매번 SSH로 접속한 뒤 `uptime`, `free`, `df`, `ss`, `journalctl`, `systemctl` 같은 명령어를 직접 실행해야 합니다. ServerScope는 이 흐름을 데스크탑 GUI에서 한 화면으로 보여줍니다.

## MVP 기능

- 서버 등록: name, host, port, username, SSH private key path
- SSH 연결 테스트
- Dashboard: uptime, CPU, RAM, disk, network RX/TX, load average
- Network: `ss -tunlp` 기반 TCP/UDP 연결 및 listening port 목록
- Services: 사용자가 등록한 systemd service 상태 확인
- Logs: 최근 error journal 50줄 표시
- 자동 새로고침: 5초, 10초, 30초
- 로컬 JSON config 저장

## 설치 및 실행

```bash
cargo run
```

릴리스 빌드:

```bash
cargo build --release
```

## SSH key 설정

ServerScope MVP는 SSH private key 인증만 사용합니다. password 저장은 제외합니다.

```bash
ssh-keygen -t ed25519 -f ~/.ssh/serverscope_demo
ssh-copy-id -i ~/.ssh/serverscope_demo.pub user@your-server
```

앱에는 private key 파일의 내용이 아니라 경로만 입력합니다.

예:

```text
/Users/you/.ssh/serverscope_demo
```

## 지원 OS

- 앱 실행: macOS, Linux
- 모니터링 대상: Ubuntu 서버

## Architecture Diagram

```text
┌────────────────────┐
│ eframe/egui UI     │
│ - state            │
│ - tabs             │
│ - latest snapshot  │
└─────────┬──────────┘
          │ crossbeam-channel
┌─────────▼──────────┐
│ SSH worker thread  │
│ - connect          │
│ - run commands     │
│ - parse output     │
└─────────┬──────────┘
          │ ssh2
┌─────────▼──────────┐
│ Ubuntu server      │
│ uptime/free/df/ss  │
│ journalctl/systemd │
└────────────────────┘
```

## SSH Command Flow

```text
Connect/Test
  -> uptime

Refresh
  -> uptime
  -> top -bn1 | grep "Cpu(s)"
  -> free -m
  -> df -h /
  -> cat /proc/loadavg
  -> cat /proc/net/dev
  -> ss -tunlp
  -> systemctl is-active <service>
  -> journalctl -p err -n 50 --no-pager
```

## UI Screenshot

현재 저장소에는 스크린샷 파일이 포함되어 있지 않습니다. 앱 실행 후 서버를 등록하고 대시보드를 캡처해 이 섹션에 추가하면 됩니다.

## 보안 정책

- private key 파일 내용은 저장하지 않습니다.
- config에는 key path만 저장합니다.
- password 저장은 MVP에서 제외합니다.
- 서버 명령어는 고정된 모니터링 명령어만 실행합니다.
- 사용자가 임의 command를 실행하는 기능은 MVP에서 제외합니다.
- systemd service 이름은 안전한 문자만 허용합니다.

## Config 위치

설정은 OS config directory 아래에 저장됩니다.

```text
serverscope/config.json
```

예:

```text
~/Library/Application Support/serverscope/config.json
~/.config/serverscope/config.json
```

## Roadmap

1. 서버 여러 개 동시 모니터링
2. Docker container 상태 표시
3. systemd restart 버튼
4. 로그 검색/필터
5. 알림 기능
6. SSH tunnel
7. agent 방식 실시간 모니터링
8. packet capture 추가
9. Mac App Store / Gumroad 유료 배포
10. Pro 기능: 서버 개수 제한 해제, 로그 히스토리, export, alert
