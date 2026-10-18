<p align="center">
  <img src="../../docs/brand/concepts/hycli-shima-banner-v4.png" alt="웹사이트를 AI가 사용할 수 있는 도구로 바꾸세요." width="100%" />
</p>

<p align="center">
  <strong>어떤 웹사이트든 자동화하세요.</strong>
</p>

<p align="center">
  <a href="#get-started">시작하기</a> ·
  <a href="#what-your-ai-can-do">AI로 할 수 있는 일</a> ·
  <a href="#common-concerns">우려 사항과 해결 방식</a> ·
  <a href="#ai-connections">AI 연결</a> ·
  <a href="#use-hycli-with-your-ai">AI와 함께 사용</a>
</p>

<details>
<summary>내 언어로 읽기 · 20개 언어</summary>

[English](../../README.md) · [한국어](README.ko.md) · [日本語](README.ja.md) · [简体中文](README.zh-CN.md) · [繁體中文](README.zh-TW.md) · [Español](README.es.md) · [Français](README.fr.md) · [Deutsch](README.de.md) · [Português (Brasil)](README.pt-BR.md) · [Bahasa Indonesia](README.id.md) · [Italiano](README.it.md) · [Nederlands](README.nl.md) · [Polski](README.pl.md) · [Türkçe](README.tr.md) · [Русский](README.ru.md) · [Українська](README.uk.md) · [Tiếng Việt](README.vi.md) · [ไทย](README.th.md) · [العربية](README.ar.md) · [हिन्दी](README.hi.md)

</details>

웹사이트 주소와 원하는 목적을 입력하면 Hycli가 사이트의 인터페이스를 파악해 타입이 지정된 CLI·MCP 도구(읽기 및 쓰기 모두 포함)를 준비하고 안전하게 검증합니다. 웹사이트, 로그인 계정, 쓰기 권한, 실행 결과를 로컬 대시보드 한곳에서 관리하세요.

| 연결 | 준비 | 사용 |
| --- | --- | --- |
| 웹사이트를 추가하고 AI를 선택하세요. | 세 작업자가 탐색·연결·검증하는 과정을 확인하세요. | 대시보드, 터미널, 또는 AI 어시스턴트에서 바로 실행하세요. |

<p align="center">
  <img src="../../docs/images/dashboard.png" alt="웹사이트 카드, 사용 가능한 작업, 작업 진행 상황을 보여주는 Hycli 대시보드." width="100%" />
</p>
<p align="center"><sub>샘플 도구와 계정으로 구성한 예시 화면입니다.</sub></p>

<a id="get-started"></a>
## 시작하기

Hycli는 웹사이트 엔진과 로컬 대시보드를 단일 실행 파일에 내장하고 있습니다. 패키지를 실행할 때 Rust나 Node.js는 필요하지 않습니다.

### 다운로드 및 실행

Windows(`*-setup.exe`), macOS(`.dmg`), Linux(`.deb`, `.tar.gz`) 데스크톱 패키지는 [네이티브 CI](https://github.com/Hybirdss/Hycli/actions/workflows/verify.yml)에서 빌드됩니다([빌드 안내](../BUILD.md) 참고).

**[다운로드 v0.1.0 · Linux x64](https://github.com/Hybirdss/Hycli/releases/tag/v0.1.0)** · glibc ≥ 2.39 · [SHA-256](https://github.com/Hybirdss/Hycli/releases/download/v0.1.0/hycli-0.1.0-linux-x64.tar.gz.sha256)

```sh
sha256sum -c hycli-0.1.0-linux-x64.tar.gz.sha256
tar -xzf hycli-0.1.0-linux-x64.tar.gz
cd hycli-0.1.0-linux-x64
./hycli dashboard
```

<details>
<summary><strong>소스에서 빌드하기</strong></summary>

```sh
node scripts/build.mjs
./dist/hycli open
```

Windows에서는 `./dist/hycli.exe open`으로 실행합니다.

</details>

대시보드는 `http://127.0.0.1:4318`(또는 빈 로컬 포트)에서 열립니다. `hycli open`, `hycli status`, `hycli stop`으로 백그라운드 실행을 제어하거나 `hycli dashboard --no-open --port 4320`을 사용할 수 있습니다.

1. **AI 연결**을 열고 제공업체를 연결하세요.
2. 웹사이트 주소를 추가하고 준비 작업을 수행할 AI를 선택하세요.
3. 준비된 작업을 검토한 뒤 대시보드에서 실행하거나 **코딩 에이전트**에서 MCP 설정을 한 번 복사하세요.

로그인이 필요한 사이트는 **계정**에서 연결하고, 별도 API 키는 **작업 탐색 → 웹사이트 API 키**에 저장하세요.

<a id="what-your-ai-can-do"></a>
## AI로 할 수 있는 일

Hycli는 웹사이트의 다양한 인터페이스를 명확한 입력·출력을 가진 도구로 변환합니다.

| 웹사이트 인터페이스 | 준비되는 기능 |
| --- | --- |
| REST 및 OpenAPI | JSON/YAML 스키마와 문서에 기반한 타입 지정 요청 |
| GraphQL | 파싱된 쿼리·뮤테이션 문서와 JSON 변수 |
| HTML 및 JS 렌더링 페이지 | 관찰된 선택자 기반의 목록·링크·페이지네이션 추출 및 로컬 Chromium 렌더링 조회 |
| JSON 및 URL 인코딩 폼 | 문서화된 입력값, 기본값, 계정 기반 읽기·쓰기 요청 |

<p align="center">
  <img src="../../docs/images/preparation.gif" alt="AI 작업자 셋, 그리고 아주 바쁜 작은 새 한 마리." width="100%" />
</p>
<p align="center"><sub>AI 작업자 셋, 그리고 아주 바쁜 작은 새 한 마리.</sub></p>

세 개의 독립적인 AI 작업자가 읽기·검색, 작업 흐름, 계정 검증을 분담합니다. 연결된 문서, API 스키마, 프론트엔드 JS 번들, 브라우저가 관찰한 요청 구조를 분석해 도구를 구성합니다.

| 작업 | 동작 |
| --- | --- |
| 읽기 또는 검색 | 근거가 확인되고 읽기로 분류된 작업은 자율적으로 실행합니다. |
| 생성, 전송, 수정 또는 삭제 | 준비 과정에서도 스펙에 함께 구성되지만, 준비 중에는 절대 실행되지 않습니다. 각 웹사이트 CLI마다 기본값은 꺼짐(`writes: false`)이며, AI 에이전트에서 최초 1회 **"이 CLI에서 쓰기를 허용할까요?"**라고 물어본 뒤 허용하면(`hycli writes SITE allow` 또는 MCP `hycli_writes`) 이후 바로 실행됩니다. |
| 영향이 불분명한 작업 | 요청을 보내기 전에 검토를 거칩니다. |
| 인증, 챌린지 또는 요청 제한 | 영향을 받은 요청을 일시 중지하거나 전송 전략을 전환하여 계정을 보호합니다. |

지원되는 요청과 추출 규칙은 [작업 규격](../SITESPEC.md)과 [배포 검증](../RELEASING.md)을 참고하세요.

<a id="common-concerns"></a>
## 자주 우려하는 문제와 Hycli의 해결 방식

AI 에이전트를 실제 웹사이트나 로그인된 계정에 연결할 때 사람들이 가장 많이 걱정하는 문제와 Hycli의 기술적 해결 방식입니다.

### 1. 무단 쓰기 및 의도하지 않은 데이터 변경
- **CLI 생성·점검 중 실제 쓰기 0건 보장:** `hycli prepare`, `hycli request`, `hycli recon`, `hycli check`는 읽기와 쓰기 작업을 모두 발굴해 CLI 스펙으로 만들지만, 준비·검증 과정에서는 **실제 서버에 쓰기 요청을 절대 보내지 않습니다.** `GET /delete`처럼 읽기로 위장된 변경 경로, GraphQL `mutation`, HTML `<form method="post">` 제출도 검증 실행에서 원천 차단됩니다.
- **CLI별 최초 1회 사용자 쓰기 허용:** 새로 생성된 모든 웹사이트 CLI는 쓰기가 꺼진 상태(`writes: false`)로 시작합니다. 에이전트가 해당 CLI에서 글 작성·수정·삭제 같은 쓰기 기능을 처음 실행해야 할 때 사용자에게 **CLI별로 최초 1회** *"이 CLI(`<site>`)에서 쓰기를 허용할까요?"*라고 먼저 물어봅니다. 사용자가 동의해 `hycli writes <site> allow`(또는 MCP `hycli_writes`)가 켜진 뒤에만 쓰기 작업이 즉시 실행되며, 언제든 `hycli writes <site> deny`나 대시보드의 **변경 허용** 토글로 다시 끌 수 있습니다.
- **실패한 쓰기 자동 재시도 금지:** 쓰기 요청은 실패하거나 중단되어도 자동으로 재시도하지 않습니다. 전송 후 연결이 끊기면 중복 등록·결제를 막기 위해 `action_outcome_unknown` 상태를 반환하여 웹사이트를 먼저 확인하게 합니다.

### 2. 로그인 세션 및 자격 증명 유출 우려
- **AI 프롬프트에 비밀값 전달 금지:** 브라우저 쿠키, `localStorage`·`sessionStorage` 토큰, API 키는 로컬 암호화 보관소(OS 키링 또는 `0600` 사용자 전용 파일)에만 저장됩니다.
- **심볼릭 연결 레시피(`ConnectionRecipe`):** AI 모델에는 계정 라벨, 스토리지 키 이름, 마스킹된 응답 구조만 전달되며 실제 쿠키값이나 토큰 문자열은 절대 전달되지 않습니다. AI는 로컬 키 이름만 참조하는 연결 레시피를 작성하고, 실제 인증값은 실행 시점에 Hycli가 로컬에서 주입한 뒤 출력과 로그에서 자동 마스킹합니다.
- **안전한 브라우저 세션 가져오기:** 요청한 도메인의 세션만 읽어 오며, 브라우저 프로필의 보안을 약화하거나 다른 사이트의 쿠키를 건드리지 않습니다.

### 3. 스크래핑, 봇 탐지(CAPTCHA) 및 계정 정지 우려
- **무차별 대입(Fuzzing) 및 공격적 크롤링 금지:** 존재하지 않는 엔드포인트 목록을 추측하거나 서버를 퍼징하지 않고, 문서·스키마·정적 JS 번들·실제 브라우저 네트워크 구조에서만 API를 학습합니다.
- **사이트·계정별 요청 속도 조절:** 요청은 웹사이트와 계정 단위로 직렬화되어 전송 속도를 조절하며, `Retry-After` 헤더를 준수하고 읽기 재시도 횟수와 응답 크기를 엄격히 제한합니다.
- **전략 서킷 브레이커와 실제 브라우저 컨텍스트 실행:** 일반 HTTP 요청이 403/429나 봇 챌린지(Cloudflare·Akamai 등)에 막히면 같은 방식으로 무한 재시도하지 않습니다. 서킷 브레이커가 즉시 작동해 로그인된 실제 브라우저 컨텍스트 내 `window.fetch` 실행이나 로컬 스텔스 Chromium 엔진으로 전략을 전환하거나, 안전하게 일시 중지해 계정을 보호합니다.

### 4. 타 도메인 인증 유출 및 로컬 환경 노출
- **엄격한 오리진(Origin) 바인딩:** 인증 헤더와 쿠키는 원래 오리진에만 묶이며, 다른 도메인으로 리다이렉트될 때 즉시 제거됩니다. 사설망·로컬 주소는 `--allow-local`을 명시하지 않는 한 기본 차단됩니다.
- **이식 가능한 스펙:** 로컬 브라우저 실행 경로와 프로필 경로는 기기 내부 상태에만 남으며 생성된 CLI 정의(`SiteSpec`)에 노출되지 않습니다.

### 5. 세션 만료 시 무의미한 AI 재시도
- **AI 없이 수행하는 상태 진단(`hycli check`):** 작업이 실패했을 때 AI가 토큰을 낭비하며 로그인 페이지를 반복 호출하지 않도록, `hycli check`(MCP `hycli_check`)가 검증된 읽기 체인을 재생해 `signed_out`(로그인 만료)과 `site_changed`(사이트 구조 변경), `blocked`, `unreachable`을 정확히 구분합니다.

<a id="ai-connections"></a>
## AI 연결

| 연결 | 로그인 |
| --- | --- |
| ChatGPT · API 키 | OpenAI API 키 |
| Claude | Anthropic API 키 |
| xAI | xAI API 키 |
| Z.ai / Z.ai Coding Plan | Z.ai API 키 |
| ChatGPT · 로그인 | 설치된 Codex app-server에서 관리하는 ChatGPT 로그인 |

Codex 연결은 파일·셸·브라우저·외부 MCP 실행이 비활성화된 임시 스레드에서 공식 app-server 프로토콜을 사용하며, Hycli는 Codex OAuth 토큰을 읽거나 복사하지 않습니다.

<a id="use-hycli-with-your-ai"></a>
## AI와 함께 Hycli 사용하기

**코딩 에이전트 → 연결 설정 보기**에서 MCP 설정을 한 번 복사해 에이전트에 등록하세요.

```json
{
  "mcpServers": {
    "hycli": {
      "command": "hycli",
      "args": ["mcp"]
    }
  }
}
```

설치된 사이트 도구만 노출하려면 `hycli mcp --sites-only`를, 특정 사이트로만 제한하려면 `--only-site SITE`를 추가하세요. MCP 서버는 `hycli_prepare`, `hycli_writes`(CLI별 최초 1회 사용자 확인 후 쓰기 활성화), `hycli_run`, `hycli_check`, `hycli_result`, `hycli_activity`를 제공하며 자격 증명을 읽는 도구는 절대 제공하지 않습니다.

터미널에서도 동일한 워크플로를 바로 사용할 수 있습니다.

```sh
hycli prepare https://your-website.example --intent "저장한 참고 자료 찾기"
hycli request SITE "글 본문 편집, 초안 저장, 발행 기능을 추가해줘"
hycli describe SITE
hycli SITE ACTION --help
hycli writes SITE allow
hycli SITE ACTION --query "design systems"
hycli check SITE
```

[에이전트 안내](../../agent/AGENTS.md) · [Hycli 스킬](../../skills/hycli/SKILL.md)

<a id="browser-sign-in"></a>
## 브라우저 로그인

Hycli는 로컬 Chrome, Chromium, Edge, Brave, Firefox 프로필(및 설치된 로컬 스텔스 Chromium 엔진)을 탐색하여 요청한 웹사이트의 세션만 로컬 암호화 보관소로 가져옵니다. 같은 웹사이트 계정으로 확인된 여러 프로필은 자동으로 하나로 묶입니다.

OS 보호(Windows App-Bound 등)나 컨테이너 세션으로 보호된 경우 **계정 → 계정 연결**에서 브라우저로 로그인하거나 [브라우저 확장 프로그램](../../browser-companion/guide.html) 또는 쿠키 파일(JSON/Netscape) 가져오기를 사용할 수 있습니다(쿠키 내용을 AI 채팅창에 직접 붙여넣지 마세요).

<a id="languages"></a>
## 언어

대시보드와 AI 작업 설명은 아랍어(RTL)를 포함해 상단에 링크된 20개 언어를 지원합니다.

<a id="local-data"></a>
## 로컬 데이터 및 요청 제한

웹사이트 정의, 암호화된 자격 증명, 활동 기록은 로컬 데이터 디렉터리에 보관됩니다(`HYCLI_DATA_DIR`로 변경 가능). **설정**에서 현재 경로와 키링 암호화 상태를 확인할 수 있습니다.

<a id="contributing"></a>
## 개발 및 검증

```sh
npm --prefix dashboard ci
npm --prefix dashboard run check
npm --prefix dashboard run build
cargo test --all-targets --locked
cargo build --locked --bin hycli --example dashboard_fixture
node scripts/test-e2e.mjs --full
```

모든 테스트는 합성된 로컬 웹사이트와 픽스처를 사용합니다. 테스트 목적으로 실제 외부 서비스 계정에서 쓰기 작업을 실행하지 마세요. [시각 자료 출처](../images/README.md)를 참고하세요.

## 라이선스 및 사용 목적

Hycli는 [Apache-2.0](../../LICENSE) 라이선스로 배포됩니다. 접근 권한이 있는 웹사이트와 계정에만 사용하세요. 소프트웨어는 보증 없이 제공되며, 사용으로 인해 발생하는 손실이나 계정 차단·정지·제한에 대해 저작자와 기여자는 책임을 지지 않습니다.

## Star History

<a href="https://www.star-history.com/#Hybirdss/Hycli&Date">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=Hybirdss/Hycli&type=Date&theme=dark" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=Hybirdss/Hycli&type=Date" />
   <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=Hybirdss/Hycli&type=Date" />
 </picture>
</a>
