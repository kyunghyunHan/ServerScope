# ServerScope Project Guide

## 프로젝트 목표
Rust로 macOS/Linux용 데스크탑 앱을 만든다.
이 앱은 외부 Ubuntu 서버를 SSH로 연결해서 서버 상태, 네트워크 연결, 열린 포트, systemd 서비스, 최근 에러 로그를 한 화면에서 보여주는 개발자용 서버/네트워크 모니터링 앱이다.

## 앱 이름
ServerScope

## 한 줄 설명
ServerScope는 개발자와 1인 서버 운영자를 위한 가벼운 SSH 기반 Ubuntu 서버 모니터링 데스크탑 앱이다.

## 핵심 사용자
- VPS 운영자
- Rust/Node/Python 백엔드 개발자
- 개인 서버 운영자
- 사이드프로젝트 배포하는 개발자
- Docker/Ubuntu 서버 쓰는 사람

## 핵심 문제
개발자는 서버 상태를 확인하려고 매번 SSH 접속 후 htop, free, df, ss, journalctl, systemctl 명령어를 직접 실행해야 한다.
ServerScope는 이 과정을 GUI로 보여준다.

## 기술 스택
- Rust
- eframe/egui for desktop UI
- ssh2 crate for SSH connection
- serde/serde_json for config
- tokio or std::thread for background refresh
- crossbeam-channel or std::sync::mpsc for UI communication
- dirs crate for config path
- chrono for timestamp

## MVP 핵심 기능
1. 서버 추가
   - name
   - host
   - port
   - username
   - SSH private key path
   - optional password는 MVP에서는 제외 가능
2. 연결 테스트
   - SSH 접속 성공/실패 표시
3. 서버 대시보드
   - uptime
   - CPU usage
   - RAM usage
   - disk usage
   - network rx/tx
   - load average
4. 네트워크 탭
   - 열린 TCP/UDP 포트
   - 현재 연결 목록
   - listening ports
   - 명령어: `ss -tunlp`
5. 서비스 탭
   - systemd service 상태 확인
   - 자주 보는 서비스 이름을 사용자가 추가
   - 예: nginx, postgresql, docker, myapp
   - 명령어: `systemctl is-active <service>`
6. 로그 탭
   - 최근 에러 로그 표시
   - 명령어: `journalctl -p err -n 50 --no-pager`
7. 자동 새로고침
   - 5초 / 10초 / 30초 선택
8. 로컬 config 저장
   - 서버 목록을 JSON 파일로 저장
   - 민감한 private key 내용은 저장하지 말고 path만 저장

## UI 구성
왼쪽 사이드바:
- Servers
- Dashboard
- Network
- Services
- Logs
- Settings

상단:
- 앱 이름: ServerScope
- 현재 선택된 서버
- Connect / Disconnect 버튼
- Refresh 버튼
- 상태: Connected / Disconnected / Error

Dashboard:
카드 형태:
- CPU
- RAM
- Disk
- Network RX/TX
- Uptime
- Load Average

Network 탭:
테이블:
`Protocol | Local Address | Remote Address | State | Process`

Services 탭:
테이블:
`Service | Status | Last Checked`

Logs 탭:
최근 에러 로그를 색상 구분해서 표시

## 아키텍처
- UI thread: egui 실행
- SSH worker thread: 선택된 서버에 SSH 연결 후 명령어 실행
- Command runner: 서버 명령어 실행
- Parser: 명령어 결과를 구조체로 파싱
- Channel: worker thread에서 UI thread로 ServerSnapshot 전송
- UI state: 최신 ServerSnapshot 저장

## 데이터 구조 예시
```rust
struct ServerConfig {
    name: String,
    host: String,
    port: u16,
    username: String,
    private_key_path: String,
    services: Vec<String>,
}

struct ServerSnapshot {
    timestamp: String,
    uptime: String,
    cpu_usage: f32,
    ram_used_mb: u64,
    ram_total_mb: u64,
    disk_used: String,
    disk_total: String,
    load_average: String,
    network_rx: String,
    network_tx: String,
    connections: Vec<NetworkConnection>,
    services: Vec<ServiceStatus>,
    error_logs: Vec<String>,
}

struct NetworkConnection {
    protocol: String,
    local: String,
    remote: String,
    state: String,
    process: String,
}

struct ServiceStatus {
    name: String,
    status: String,
}
```

## 서버에서 실행할 명령어
- `uptime`
- `free -m`
- `df -h /`
- `cat /proc/loadavg`
- `ss -tunlp`
- `journalctl -p err -n 50 --no-pager`
- `systemctl is-active <service>`

CPU 사용률 계산:
MVP에서는 간단하게 top 사용:
`top -bn1 | grep "Cpu(s)"`

추후에는 `/proc/stat`을 두 번 읽고 차이를 계산하는 방식으로 개선한다.

## 구현 단계
1. cargo new serverscope
2. eframe/egui 기본 창 만들기
3. 서버 추가 UI 만들기
4. ServerConfig JSON 저장/불러오기
5. ssh2로 SSH 연결 테스트 구현
6. SSH 명령어 실행 함수 구현
7. uptime/free/df/ss/journalctl 결과 가져오기
8. 파서 작성
9. ServerSnapshot 생성
10. egui dashboard 표시
11. Network/Services/Logs 탭 구현
12. 자동 refresh thread 구현
13. README 작성

## 보안 주의사항
- private key 파일 내용은 저장하지 않는다.
- config에는 key path만 저장한다.
- password 저장은 MVP에서 제외한다.
- 서버 명령어는 고정된 안전한 명령어만 실행한다.
- 사용자가 임의 command를 실행하는 기능은 MVP에서 제외한다.

## MVP에서 제외할 것
- cloud sync
- 계정 시스템
- agent 설치
- packet capture
- raw socket
- paid plan
- alert push
- Docker deep inspection

## 추후 확장
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

## README에 포함할 내용
- ServerScope가 해결하는 문제
- 설치 방법
- SSH key 설정 방법
- 지원 OS
- architecture diagram
- SSH command flow
- UI screenshot
- 보안 정책
- roadmap

## 중요
이 앱은 단순 포트폴리오가 아니라 실제 개발자가 자기 Ubuntu VPS를 관리할 때 쓸 수 있는 데스크탑 앱처럼 만들어야 한다.
처음 목표는 "SSH 접속 없이 서버 상태를 한눈에 보는 앱"이다.
